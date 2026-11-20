//! The generation pipeline: validate everything, specialize both
//! architectures in memory, stage and verify both, then publish both.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::spec::APP_VERSION;
use crate::{build_spec, check_icon, inspect, specialize_raw, AppInput, Arch, PackError};

/// An existing file larger than this is not a generated app, so it is never
/// read whole to find out.
const MAX_APP_BYTES: u64 = 256 * 1024 * 1024;

/// The unspecialized runtime for each architecture.
#[derive(Debug, Clone, Copy)]
pub struct Templates<'a> {
    pub x64: &'a [u8],
    pub arm64: &'a [u8],
}

impl Templates<'_> {
    fn get(&self, arch: Arch) -> &[u8] {
        match arch {
            Arch::X64 => self.x64,
            Arch::Arm64 => self.arm64,
        }
    }
}

/// What was at an output path before this generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replaced {
    None,
    SameApp,
    OtherApp,
}

impl Replaced {
    pub fn as_str(self) -> &'static str {
        match self {
            Replaced::None => "none",
            Replaced::SameApp => "sameApp",
            Replaced::OtherApp => "otherApp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub arch: Arch,
    pub path: PathBuf,
    pub replaced: Replaced,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub app_id: String,
    pub slug: String,
    pub outputs: Vec<Output>,
}

/// Generate `<slug>-x64.exe` and `<slug>-arm64.exe` in `out_dir`.
///
/// On any error before publishing, the folder is left exactly as it was.
pub fn generate(
    input: &AppInput,
    icon_png: &[u8],
    out_dir: &Path,
    templates: Templates,
) -> Result<Generated, PackError> {
    let (spec, spec_bytes) = build_spec(input)?;
    check_icon(icon_png)?;
    if !out_dir.is_dir() {
        return Err(PackError::Input {
            field: "output folder",
            reason: format!("{} does not exist or is not a folder", out_dir.display()),
        });
    }
    let mut outputs: Vec<Output> = Vec::new();
    for arch in Arch::ALL {
        let path = out_dir.join(format!("{}-{}.exe", spec.slug, arch.suffix()));
        let replaced = existing(&path, &spec.app_id)?;
        outputs.push(Output {
            arch,
            path,
            replaced,
        });
    }

    let mut apps = Vec::new();
    for arch in Arch::ALL {
        let template = templates.get(arch);
        let unusable = |reason: String| PackError::Template { arch, reason };
        let report = inspect(template).map_err(|error| unusable(error.to_string()))?;
        if report.arch != Some(arch) {
            return Err(unusable(format!(
                "it is not an {} executable",
                arch.suffix()
            )));
        }
        if report.spec.is_some() {
            return Err(unusable("it is already specialized".into()));
        }
        apps.push(specialize_raw(
            template,
            &spec_bytes,
            icon_png,
            &spec.display_name,
            APP_VERSION,
        )?);
    }

    let mut staged = Staged(Vec::new());
    for (output, app) in outputs.iter().zip(&apps) {
        let path = stage(out_dir, app, &mut staged.0)?;
        verify(&path, output, &spec_bytes, &spec.display_name)?;
    }

    // Check again: specializing and verifying leave time for a file to
    // appear at a target, and only generated apps may be replaced.
    // shortcut: a file created in the instant between this check and the
    // rename would still be replaced; closing that needs a no-replace rename
    // (MoveFileExW without MOVEFILE_REPLACE_EXISTING, renameat2 NOREPLACE),
    // which std lacks. Upgrade if other programs write into output folders.
    for output in &mut outputs {
        output.replaced = existing(&output.path, &spec.app_id)?;
    }
    for (index, (output, path)) in outputs.iter().zip(&staged.0).enumerate() {
        fs::rename(path, &output.path).map_err(|error| match index {
            0 => PackError::Io {
                path: output.path.clone(),
                action: "replace",
                error,
            },
            _ => PackError::PartialPublish {
                updated: outputs[0].path.clone(),
                failed: output.path.clone(),
                error,
            },
        })?;
    }
    staged.0.clear(); // all published; nothing left to remove
    Ok(Generated {
        app_id: spec.app_id,
        slug: spec.slug,
        outputs,
    })
}

/// Classify what is at an output path. Only a generated app, one that
/// carries an embedded specification, may be replaced.
fn existing(path: &Path, app_id: &str) -> Result<Replaced, PackError> {
    let io_error = |error| PackError::Io {
        path: path.to_path_buf(),
        action: "check",
        error,
    };
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Replaced::None),
        Err(error) => return Err(io_error(error)),
    };
    // Directories and symlinks are never ours; nor is anything this large.
    if !metadata.is_file() || metadata.len() > MAX_APP_BYTES {
        return Err(PackError::Foreign(path.to_path_buf()));
    }
    let bytes = fs::read(path).map_err(io_error)?;
    let Some(spec) = inspect(&bytes).ok().and_then(|report| report.spec) else {
        return Err(PackError::Foreign(path.to_path_buf()));
    };
    // Windows refuses to replace a read-only file, which would otherwise
    // surface only after the other architecture was already published.
    if metadata.permissions().readonly() {
        return Err(PackError::Io {
            path: path.to_path_buf(),
            action: "replace",
            error: io::Error::new(io::ErrorKind::PermissionDenied, "it is read-only"),
        });
    }
    // A running app cannot be replaced: Windows refuses the rename (pinned by
    // tests/running_app.rs). Opening for write, without truncating, fails the
    // same way and catches it before the other architecture is published.
    if let Err(error) = OpenOptions::new().write(true).open(path) {
        return Err(PackError::InUse {
            path: path.to_path_buf(),
            error,
        });
    }
    // Read leniently: an app from an older or newer generator still counts.
    let existing_id = serde_json::from_slice::<Value>(&spec)
        .ok()
        .and_then(|value| value.get("appId")?.as_str().map(str::to_owned));
    Ok(if existing_id.as_deref() == Some(app_id) {
        Replaced::SameApp
    } else {
        Replaced::OtherApp
    })
}

/// Write `bytes` to a new, uniquely named file in `dir` and flush it to
/// disk. The path is recorded in `staged` before writing, so a failed write
/// is still cleaned up.
fn stage(dir: &Path, bytes: &[u8], staged: &mut Vec<PathBuf>) -> Result<PathBuf, PackError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    for _ in 0..32 {
        let path = dir.join(format!(
            ".webwrap-{}-{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                staged.push(path.clone());
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| PackError::Io {
                        path: path.clone(),
                        action: "write",
                        error,
                    })?;
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(PackError::Io {
                    path,
                    action: "create",
                    error,
                })
            }
        }
    }
    Err(PackError::Io {
        path: dir.to_path_buf(),
        action: "create a staging file in",
        error: io::Error::other("32 candidate names were already taken"),
    })
}

/// Read a staged app back from disk and check it carries what was written.
fn verify(
    staged: &Path,
    output: &Output,
    spec: &[u8],
    display_name: &str,
) -> Result<(), PackError> {
    let fail = |reason: String| PackError::Verify {
        path: output.path.clone(),
        reason,
    };
    let bytes = fs::read(staged).map_err(|error| fail(error.to_string()))?;
    let report = inspect(&bytes).map_err(|error| fail(error.to_string()))?;
    if report.arch != Some(output.arch) {
        return Err(fail("its architecture changed".into()));
    }
    if report.spec.as_deref() != Some(spec) {
        return Err(fail("the embedded specification differs".into()));
    }
    if !report.icon_present {
        return Err(fail("it has no icon".into()));
    }
    if report.file_description.as_deref() != Some(display_name) {
        return Err(fail("its file description differs".into()));
    }
    if report.product_version.as_deref() != Some(APP_VERSION) {
        return Err(fail("its product version differs".into()));
    }
    Ok(())
}

/// Staging files, removed when dropped: covers every error path and panics.
/// Published files are cleared from the list first.
struct Staged(Vec<PathBuf>);

impl Drop for Staged {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}
