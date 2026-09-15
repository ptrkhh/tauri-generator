use serde_json::Value;
use std::fmt;
use std::path::Path;

use editpe::{
    constants::{CODE_PAGE_ID_EN_US, LANGUAGE_ID_EN_US, RT_RCDATA},
    Image, ResourceData, ResourceEntry, ResourceEntryName, ResourceTable, VersionInfo,
    VersionStringTable,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeError {
    InvalidSpec(String),
    InvalidText(String),
    InvalidPath,
    MissingResource(String),
    Resource(String),
    Image(String),
}

impl fmt::Display for PeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpec(message)
            | Self::InvalidText(message)
            | Self::MissingResource(message)
            | Self::Resource(message)
            | Self::Image(message) => f.write_str(message),
            Self::InvalidPath => f.write_str("invalid path"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub spec: Vec<u8>,
    pub icon_present: bool,
    pub file_description: String,
    pub product_version: String,
}

pub fn edit_resources(
    image: &mut Image<'static>,
    spec: &[u8],
    icon_path: &Path,
    display_name: &str,
    version: &str,
) -> Result<(), PeError> {
    let mut resources = image.resource_directory().cloned().unwrap_or_default();
    let mut data = ResourceData::default();
    data.set_data(spec.to_vec());
    data.set_codepage(CODE_PAGE_ID_EN_US as u32);
    let mut languages = ResourceTable::default();
    languages.insert(
        ResourceEntryName::ID(LANGUAGE_ID_EN_US as u32),
        ResourceEntry::Data(data),
    );
    let mut names = ResourceTable::default();
    names.insert(
        ResourceEntryName::from_string("WEBWRAP_APP_SPEC"),
        ResourceEntry::Table(languages),
    );
    resources.root_mut().insert(
        ResourceEntryName::ID(RT_RCDATA as u32),
        ResourceEntry::Table(names),
    );
    resources
        .remove_main_icon()
        .map_err(|error| PeError::Resource(error.to_string()))?;
    resources
        .set_main_icon_file(icon_path.to_str().ok_or(PeError::InvalidPath)?)
        .map_err(|error| PeError::Resource(error.to_string()))?;
    let mut info = resources
        .get_version_info()
        .map_err(|error| PeError::Resource(error.to_string()))?
        .unwrap_or_default();
    if info.strings.is_empty() {
        info.strings.push(VersionStringTable {
            key: "040904b0".into(),
            strings: Default::default(),
        });
    }
    let strings = &mut info.strings[0].strings;
    for (key, value) in [
        ("FileDescription", display_name),
        ("ProductName", display_name),
        ("FileVersion", version),
        ("ProductVersion", version),
    ] {
        strings.insert(key.into(), value.into());
    }
    resources
        .set_version_info(&info)
        .map_err(|error| PeError::Resource(error.to_string()))?;
    image
        .set_resource_directory(resources)
        .map_err(|error| PeError::Image(error.to_string()))?;
    Ok(())
}

pub fn read_embedded_spec(path: &Path) -> Result<Vec<u8>, PeError> {
    let image = Image::parse_file(path).map_err(|error| PeError::Image(error.to_string()))?;
    let resources = image
        .resource_directory()
        .ok_or_else(|| PeError::MissingResource("resource directory".into()))?;
    let entry = resources
        .root()
        .get(ResourceEntryName::ID(RT_RCDATA as u32))
        .and_then(ResourceEntry::as_table)
        .and_then(|table| table.get(ResourceEntryName::from_string("WEBWRAP_APP_SPEC")))
        .and_then(ResourceEntry::as_table)
        .and_then(|table| table.get(ResourceEntryName::ID(LANGUAGE_ID_EN_US as u32)))
        .and_then(ResourceEntry::as_data)
        .ok_or_else(|| PeError::MissingResource("WEBWRAP_APP_SPEC".into()))?;
    validate_spec(entry.data())
}

pub fn read_metadata(path: &Path) -> Result<Metadata, PeError> {
    let image = Image::parse_file(path).map_err(|error| PeError::Image(error.to_string()))?;
    let resources = image
        .resource_directory()
        .ok_or_else(|| PeError::MissingResource("resource directory".into()))?;
    let spec = read_embedded_spec(path)?;
    let icon_present = resources
        .get_main_icon()
        .map_err(|error| PeError::Resource(error.to_string()))?
        .is_some_and(|icon| !icon.is_empty());
    if !icon_present {
        return Err(PeError::MissingResource("main icon".into()));
    }
    let info: VersionInfo = resources
        .get_version_info()
        .map_err(|error| PeError::Resource(error.to_string()))?
        .ok_or_else(|| PeError::MissingResource("version info".into()))?;
    let strings = info
        .strings
        .iter()
        .flat_map(|table| table.strings.iter())
        .collect::<std::collections::HashMap<_, _>>();
    let file_description = strings
        .get("FileDescription")
        .ok_or_else(|| PeError::MissingResource("FileDescription".into()))?
        .to_string();
    let product_version = strings
        .get("ProductVersion")
        .ok_or_else(|| PeError::MissingResource("ProductVersion".into()))?
        .to_string();
    Ok(Metadata {
        spec,
        icon_present,
        file_description,
        product_version,
    })
}

impl std::error::Error for PeError {}

pub fn validate_text(value: &str, field: &str) -> Result<(), PeError> {
    if value.is_empty() || value.chars().any(char::is_control) || value.len() > 255 {
        return Err(PeError::InvalidText(format!("invalid {field}")));
    }
    Ok(())
}

pub fn validate_spec(bytes: &[u8]) -> Result<Vec<u8>, PeError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| PeError::InvalidSpec(error.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| PeError::InvalidSpec("spec must be a JSON object".into()))?;
    if !object
        .get("schemaVersion")
        .is_some_and(|value| value.as_i64().is_some() || value.as_u64().is_some())
    {
        return Err(PeError::InvalidSpec(
            "schemaVersion must be an integer".into(),
        ));
    }
    for field in ["appId", "displayName", "slug", "launchUrl"] {
        let value = object
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| PeError::InvalidSpec(format!("{field} must be a string")))?;
        validate_text(value, field)
            .map_err(|_| PeError::InvalidSpec(format!("invalid {field}")))?;
    }
    Ok(bytes.to_vec())
}
