//! The platform-independent half of the file bridge: what a second instance
//! may send, which paths may be opened, and which page may receive or save a
//! file. The runtime keeps only the Win32 and WebView2 calls.
#![forbid(unsafe_code)]

pub mod ipc;
pub mod paths;
pub mod session;

pub use ipc::{encode_request, read_request, IpcError, MAX_MESSAGE_BYTES};
pub use paths::{check_file, display_name, resolve, MAX_FILES, MAX_FILE_BYTES};
pub use session::{
    parse_page_message, AfterRead, Delivery, Navigated, PageMessage, Refusal, Session,
    LEAVING_TIMEOUT,
};
