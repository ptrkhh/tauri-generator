//! The request a second instance sends to the running one: a 4-byte
//! little-endian length, then `{ "v": 1, "appId": "...", "files": [...] }`.

use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::MAX_FILES;

pub const MAX_MESSAGE_BYTES: u32 = 64 * 1024;
const VERSION: u64 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u64,
    #[serde(rename = "appId")]
    app_id: String,
    files: Vec<String>,
}

#[derive(Debug)]
pub enum IpcError {
    Io(io::Error),
    TooLarge(u64),
    Malformed(String),
    Version(u64),
    WrongApp,
    TooManyFiles(usize),
    BadPath,
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not read the request: {error}"),
            Self::TooLarge(len) => write!(
                f,
                "the request is {len} bytes, over the {MAX_MESSAGE_BYTES}-byte limit"
            ),
            Self::Malformed(reason) => write!(f, "the request is malformed: {reason}"),
            Self::Version(v) => write!(f, "unsupported request version {v}"),
            Self::WrongApp => f.write_str("the request is for a different app"),
            Self::TooManyFiles(n) => write!(f, "{n} files is over the limit of {MAX_FILES}"),
            Self::BadPath => f.write_str("a path is not absolute or contains NUL"),
        }
    }
}

impl std::error::Error for IpcError {}

/// Frame a request. `files` must already be absolute Unicode paths, which
/// `paths::resolve` guarantees. Zero files means "come to the front".
pub fn encode_request(app_id: &str, files: &[PathBuf]) -> Result<Vec<u8>, IpcError> {
    if files.len() > MAX_FILES {
        return Err(IpcError::TooManyFiles(files.len()));
    }
    let body = serde_json::to_vec(&Request {
        v: VERSION,
        app_id: app_id.to_string(),
        files: files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    })
    .map_err(|error| IpcError::Malformed(error.to_string()))?;
    if body.len() > MAX_MESSAGE_BYTES as usize {
        return Err(IpcError::TooLarge(body.len() as u64));
    }
    let mut frame = (body.len() as u32).to_le_bytes().to_vec();
    frame.extend(body);
    Ok(frame)
}

/// Read and validate one request. Everything about the message is checked
/// before any path is looked at, and the length before anything is allocated.
pub fn read_request(reader: &mut impl Read, app_id: &str) -> Result<Vec<PathBuf>, IpcError> {
    let mut len = [0; 4];
    reader.read_exact(&mut len).map_err(IpcError::Io)?;
    let len = u32::from_le_bytes(len);
    if len > MAX_MESSAGE_BYTES {
        return Err(IpcError::TooLarge(len.into()));
    }
    let mut body = vec![0; len as usize];
    reader.read_exact(&mut body).map_err(IpcError::Io)?;
    let request: Request =
        serde_json::from_slice(&body).map_err(|error| IpcError::Malformed(error.to_string()))?;
    if request.v != VERSION {
        return Err(IpcError::Version(request.v));
    }
    if request.app_id != app_id {
        return Err(IpcError::WrongApp);
    }
    if request.files.len() > MAX_FILES {
        return Err(IpcError::TooManyFiles(request.files.len()));
    }
    if request
        .files
        .iter()
        .any(|file| file.contains('\0') || !Path::new(file).is_absolute())
    {
        return Err(IpcError::BadPath);
    }
    Ok(request.files.into_iter().map(PathBuf::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const APP: &str = "0123456789abcdef";

    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    fn frame(json: &str) -> Vec<u8> {
        let mut bytes = (json.len() as u32).to_le_bytes().to_vec();
        bytes.extend(json.as_bytes());
        bytes
    }

    fn read(bytes: Vec<u8>) -> Result<Vec<PathBuf>, IpcError> {
        read_request(&mut Cursor::new(bytes), APP)
    }

    #[test]
    fn a_request_round_trips_including_an_empty_activation() {
        let files = vec![absolute("a.psd"), absolute("b c.md")];
        assert_eq!(read(encode_request(APP, &files).unwrap()).unwrap(), files);
        assert!(read(encode_request(APP, &[]).unwrap()).unwrap().is_empty());
    }

    #[test]
    fn an_oversized_length_is_refused_before_the_body_is_read() {
        // Only the length is present: reading a body would fail with Io.
        let bytes = (MAX_MESSAGE_BYTES + 1).to_le_bytes().to_vec();
        assert!(matches!(read(bytes), Err(IpcError::TooLarge(_))));
        assert!(matches!(
            read(u32::MAX.to_le_bytes().to_vec()),
            Err(IpcError::TooLarge(_))
        ));
    }

    #[test]
    fn every_malformed_request_is_rejected() {
        let file = serde_json::to_string(&absolute("a.psd")).unwrap();
        let cases = [
            (
                format!(r#"{{"v":2,"appId":"{APP}","files":[]}}"#),
                "version",
            ),
            (
                r#"{"v":1,"appId":"ffffffffffffffff","files":[]}"#.into(),
                "app",
            ),
            (
                format!(r#"{{"v":1,"appId":"{APP}","files":[],"x":1}}"#),
                "unknown field",
            ),
            (format!(r#"{{"v":1,"appId":"{APP}"}}"#), "missing files"),
            (
                format!(r#"{{"v":1,"appId":"{APP}","files":["relative.psd"]}}"#),
                "relative",
            ),
            (
                format!(r#"{{"v":1,"appId":"{APP}","files":["/a\u0000b"]}}"#),
                "nul",
            ),
            (
                format!(
                    r#"{{"v":1,"appId":"{APP}","files":[{}]}}"#,
                    vec![file.as_str(); 17].join(",")
                ),
                "17 files",
            ),
            ("not json".into(), "garbage"),
            ("".into(), "empty"),
        ];
        for (json, label) in cases {
            assert!(read(frame(&json)).is_err(), "{label} was accepted");
        }
    }

    #[test]
    fn a_truncated_body_is_an_error_not_a_hang_or_panic() {
        let mut bytes = encode_request(APP, &[absolute("a.psd")]).unwrap();
        bytes.truncate(bytes.len() - 3);
        assert!(matches!(read(bytes), Err(IpcError::Io(_))));
    }

    #[test]
    fn the_sender_refuses_what_the_receiver_would() {
        assert!(encode_request(APP, &vec![absolute("a"); 17]).is_err());
        let long = absolute(&"x".repeat(MAX_MESSAGE_BYTES as usize));
        assert!(encode_request(APP, &[long]).is_err());
    }
}
