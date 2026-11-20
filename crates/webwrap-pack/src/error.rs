use std::fmt;
use std::path::PathBuf;

use crate::Arch;

#[derive(Debug)]
pub enum PackError {
    /// The user's input is invalid. The CLI reports these as usage errors.
    Input { field: &'static str, reason: String },
    /// Not a readable PE image, or a resource could not be edited.
    Pe(String),
    /// An embedded or supplied runtime template is unusable.
    Template { arch: Arch, reason: String },
    /// A file at the output path that the generator did not make.
    Foreign(PathBuf),
    Io {
        path: PathBuf,
        action: &'static str,
        error: std::io::Error,
    },
    /// A staged app did not read back as written.
    Verify { path: PathBuf, reason: String },
    /// The first app was published but the second was not.
    PartialPublish {
        updated: PathBuf,
        failed: PathBuf,
        error: std::io::Error,
    },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::Pe(reason) => write!(f, "not a usable Windows executable: {reason}"),
            Self::Template { arch, reason } => {
                write!(f, "the {} runtime template is unusable: {reason}", arch.suffix())
            }
            Self::Foreign(path) => write!(
                f,
                "{} already exists and was not made by this generator, so it was left alone; \
                 move it or choose another name",
                path.display()
            ),
            Self::Io {
                path,
                action,
                error,
            } => write!(f, "could not {action} {}: {error}", path.display()),
            Self::Verify { path, reason } => {
                write!(f, "{} did not verify after writing: {reason}", path.display())
            }
            Self::PartialPublish {
                updated,
                failed,
                error,
            } => write!(
                f,
                "{} was updated but {} was not ({error}); generate again to bring them back in step",
                updated.display(),
                failed.display()
            ),
        }
    }
}

impl std::error::Error for PackError {}
