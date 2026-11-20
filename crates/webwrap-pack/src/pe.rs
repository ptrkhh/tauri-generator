//! PE resource primitives, ported from `spikes/windows-pe-specialization`
//! to work on bytes in memory so nothing touches disk until it verifies.

use editpe::constants::{CODE_PAGE_ID_EN_US, LANGUAGE_ID_EN_US, RT_RCDATA};
use editpe::{
    Image, ResourceData, ResourceEntry, ResourceEntryName, ResourceTable, VersionStringTable,
};
use image::ImageFormat;

use crate::PackError;

/// Fixed by the runtime's `embedded_spec.rs`: type RT_RCDATA, this string
/// name, language en-US.
const SPEC_RESOURCE: &str = "WEBWRAP_APP_SPEC";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
}

impl Arch {
    pub const ALL: [Arch; 2] = [Arch::X64, Arch::Arm64];

    /// The file-name suffix: `<slug>-x64.exe`, `<slug>-arm64.exe`.
    pub fn suffix(self) -> &'static str {
        match self {
            Arch::X64 => "x64",
            Arch::Arm64 => "arm64",
        }
    }

    /// The COFF machine type in the PE header.
    pub fn machine(self) -> u16 {
        match self {
            Arch::X64 => 0x8664,
            Arch::Arm64 => 0xAA64,
        }
    }

    /// The name the runtime reports as `runtimeArch`.
    pub fn runtime_arch(self) -> &'static str {
        match self {
            Arch::X64 => "x86_64",
            Arch::Arm64 => "aarch64",
        }
    }

    fn from_machine(machine: u16) -> Option<Arch> {
        Arch::ALL.into_iter().find(|arch| arch.machine() == machine)
    }
}

/// The COFF machine type of a PE image, read from the header `e_lfanew`
/// points at. `None` when the bytes are not a PE image.
pub fn machine_of(bytes: &[u8]) -> Option<u16> {
    let at = |offset: usize, len: usize| bytes.get(offset..offset.checked_add(len)?);
    if at(0, 2)? != b"MZ" {
        return None;
    }
    let header = u32::from_le_bytes(at(0x3c, 4)?.try_into().ok()?) as usize;
    if at(header, 4)? != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes(
        at(header.checked_add(4)?, 2)?.try_into().ok()?,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    /// `None` for a machine type other than x64 or ARM64.
    pub arch: Option<Arch>,
    /// The raw embedded specification; `None` for an unspecialized template
    /// or an executable the generator did not make.
    pub spec: Option<Vec<u8>>,
    pub icon_present: bool,
    pub file_description: Option<String>,
    pub product_version: Option<String>,
}

fn pe_error(error: impl std::fmt::Display) -> PackError {
    PackError::Pe(error.to_string())
}

/// Embed a specification, icon, and version strings into a template.
///
/// This is the unchecked primitive: it embeds whatever specification bytes
/// it is given, which is what lets tests build deliberately invalid apps.
/// The product path is `generate`, which validates first.
pub fn specialize_raw(
    template: &[u8],
    spec: &[u8],
    icon_png: &[u8],
    display_name: &str,
    version: &str,
) -> Result<Vec<u8>, PackError> {
    let icon =
        image::load_from_memory_with_format(icon_png, ImageFormat::Png).map_err(|error| {
            PackError::Input {
                field: "icon",
                reason: error.to_string(),
            }
        })?;
    let mut image = Image::parse(template).map_err(pe_error)?;
    let mut resources = image.resource_directory().cloned().unwrap_or_default();

    let mut data = ResourceData::default();
    data.set_data(spec.to_vec());
    data.set_codepage(CODE_PAGE_ID_EN_US as u32);
    let mut languages = ResourceTable::default();
    languages.insert(
        ResourceEntryName::ID(LANGUAGE_ID_EN_US as u32),
        ResourceEntry::Data(data),
    );
    let mut names = match resources
        .root()
        .get(ResourceEntryName::ID(RT_RCDATA as u32))
    {
        Some(ResourceEntry::Table(table)) => table.clone(),
        Some(ResourceEntry::Data(_)) => return Err(pe_error("RCDATA is not a table")),
        None => ResourceTable::default(),
    };
    names.insert(
        ResourceEntryName::from_string(SPEC_RESOURCE),
        ResourceEntry::Table(languages),
    );
    resources.root_mut().insert(
        ResourceEntryName::ID(RT_RCDATA as u32),
        ResourceEntry::Table(names),
    );

    // editpe names the new group MAINICON, which the runtime loads for its
    // window icon.
    resources.remove_main_icon().map_err(pe_error)?;
    resources.set_main_icon(&icon).map_err(pe_error)?;

    let mut info = resources
        .get_version_info()
        .map_err(pe_error)?
        .unwrap_or_default();
    if info.strings.is_empty() {
        info.strings.push(VersionStringTable {
            key: "040904b0".into(),
            strings: Default::default(),
        });
    }
    // shortcut: only strings[0] is updated; a template with more than one
    // VersionStringTable would keep stale values in the rest. The runtime
    // template has one (B's evidence); revisit if that changes.
    let strings = &mut info.strings[0].strings;
    for (key, value) in [
        ("FileDescription", display_name),
        ("ProductName", display_name),
        ("FileVersion", version),
        ("ProductVersion", version),
    ] {
        strings.insert(key.into(), value.into());
    }
    resources.set_version_info(&info).map_err(pe_error)?;
    image.set_resource_directory(resources).map_err(pe_error)?;

    let mut out = Vec::new();
    image.write_writer(&mut out).map_err(pe_error)?;
    Ok(out)
}

/// Read back what `specialize_raw` embeds.
pub fn inspect(bytes: &[u8]) -> Result<Inspection, PackError> {
    let image = Image::parse(bytes).map_err(pe_error)?;
    let arch = machine_of(bytes).and_then(Arch::from_machine);
    let Some(resources) = image.resource_directory() else {
        return Ok(Inspection {
            arch,
            spec: None,
            icon_present: false,
            file_description: None,
            product_version: None,
        });
    };
    let spec = resources
        .root()
        .get(ResourceEntryName::ID(RT_RCDATA as u32))
        .and_then(ResourceEntry::as_table)
        .and_then(|table| table.get(ResourceEntryName::from_string(SPEC_RESOURCE)))
        .and_then(ResourceEntry::as_table)
        .and_then(|table| table.get(ResourceEntryName::ID(LANGUAGE_ID_EN_US as u32)))
        .and_then(ResourceEntry::as_data)
        .map(|data| data.data().to_vec());
    let icon_present = resources
        .get_main_icon()
        .map_err(pe_error)?
        .is_some_and(|icon| !icon.is_empty());
    let info = resources.get_version_info().map_err(pe_error)?;
    let find = |key: &str| {
        info.as_ref()?
            .strings
            .iter()
            .find_map(|table| table.strings.get(key))
            .cloned()
    };
    Ok(Inspection {
        arch,
        spec,
        icon_present,
        file_description: find("FileDescription"),
        product_version: find("ProductVersion"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_of_rejects_non_pe_and_truncated_input() {
        assert_eq!(machine_of(b""), None);
        assert_eq!(machine_of(b"MZ"), None);
        assert_eq!(machine_of(b"not a pe at all, just text"), None);
        // e_lfanew pointing past the end must not panic.
        let mut bytes = vec![0u8; 0x40];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(machine_of(&bytes), None);
    }

    #[test]
    fn machine_of_reads_the_coff_machine_field() {
        let mut bytes = vec![0u8; 0x48];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        bytes[0x44..0x46].copy_from_slice(&0xAA64u16.to_le_bytes());
        assert_eq!(machine_of(&bytes), Some(0xAA64));
        assert_eq!(Arch::from_machine(0xAA64), Some(Arch::Arm64));
        assert_eq!(Arch::from_machine(0x014c), None);
    }
}
