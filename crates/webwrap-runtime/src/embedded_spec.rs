use std::slice;

use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::{
    FindResourceExW, GetModuleHandleW, LoadResource, LockResource, SizeofResource,
};

/// Resource type RT_RCDATA, expressed the way Win32 wants it: a small integer
/// smuggled through a string pointer.
const RT_RCDATA: PCWSTR = PCWSTR(10 as *const u16);
/// The resource name and language the generator writes. Fixed by
/// `spikes/windows-pe-specialization/src/pe.rs`.
const RESOURCE_NAME: &str = "WEBWRAP_APP_SPEC";
const LANGUAGE_EN_US: u16 = 0x0409;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Read the embedded specification from this process's own loaded image.
///
/// Reading the loaded image rather than reopening the executable file keeps
/// renaming, moving, and file locking harmless, and needs no file I/O at
/// startup. The language is requested explicitly so the lookup does not
/// depend on the machine's locale.
pub fn read() -> Result<Vec<u8>, String> {
    let name = wide(RESOURCE_NAME);
    unsafe {
        let module =
            GetModuleHandleW(PCWSTR::null()).map_err(|error| format!("module handle: {error}"))?;
        let found = FindResourceExW(
            Some(module),
            RT_RCDATA,
            PCWSTR(name.as_ptr()),
            LANGUAGE_EN_US,
        );
        if found.is_invalid() {
            return Err(
                "this application has no embedded configuration, so it was never specialized by the generator"
                    .into(),
            );
        }
        let size = SizeofResource(Some(module), found);
        if size == 0 {
            return Err("the embedded configuration is empty".into());
        }
        let handle =
            LoadResource(Some(module), found).map_err(|error| format!("load resource: {error}"))?;
        let pointer = LockResource(handle);
        if pointer.is_null() {
            return Err("the embedded configuration could not be read".into());
        }
        Ok(slice::from_raw_parts(pointer as *const u8, size as usize).to_vec())
    }
}
