//! One running window per app, and a way for a later launch to hand it files.
//!
//! Every generated executable shares one compiled runtime, so uniqueness comes
//! from the embedded `appId`, never from Tauri's identifier. Both architecture
//! builds of an app share the `appId`, so they share the pipe too.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::mem::ManuallyDrop;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use webwrap_bridge::{encode_request, read_request, MAX_MESSAGE_BYTES};
use windows::core::{Error, HSTRING, PWSTR};
use windows::Win32::Foundation::{
    LocalFree, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use crate::report;

const TIMEOUT: Duration = Duration::from_secs(5);
const ACCEPTED: u8 = 1;
const REJECTED: u8 = 0;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY_CODE: i32 = 231;
const ERROR_ACCESS_DENIED_CODE: i32 = 5;

pub enum Role {
    /// This process owns the app. The handle is the listening pipe.
    Primary(OwnedHandle),
    /// Another process owns it; forward to it.
    Secondary,
}

/// A SID as text, such as `S-1-5-21-…`.
unsafe fn sid_string(sid: PSID) -> Result<String, Error> {
    let mut text = PWSTR::null();
    ConvertSidToStringSidW(sid, &mut text)?;
    let result = text.to_string();
    LocalFree(Some(HLOCAL(text.0.cast())));
    result.map_err(|_| Error::from_win32())
}

/// The current user's SID, such as `S-1-5-21-…`.
fn user_sid() -> Result<String, Error> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let token = OwnedHandle::from_raw_handle(token.0);
        let token = HANDLE(token.as_raw_handle());
        let mut len = 0;
        // The first call only reports the size, so its error is expected.
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        // u64 storage keeps TOKEN_USER's pointer field aligned.
        let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &mut len,
        )?;
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        sid_string(user.User.Sid)
    }
}

/// Whether the pipe's owner is this user. The owner is set explicitly when
/// the pipe is created, and another user cannot make themselves look like us,
/// so this tells our own app from a pipe someone else created first.
fn owned_by(pipe: &File, sid: &str) -> bool {
    unsafe {
        let mut owner = PSID::default();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let status = GetSecurityInfo(
            HANDLE(pipe.as_raw_handle()),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            Some(&mut descriptor),
        );
        if status.is_err() {
            return false;
        }
        let same = sid_string(owner).is_ok_and(|owner| owner == sid);
        LocalFree(Some(HLOCAL(descriptor.0)));
        same
    }
}

/// Per user and per app, so different users and different apps never meet.
fn pipe_name(app_id: &str, sid: &str) -> String {
    format!(r"\\.\pipe\WebWrap.{app_id}.{sid}")
}

/// Become the app's one running instance, or learn that another one is.
pub fn claim(app_id: &str) -> Result<Role, Error> {
    let sid = user_sid()?;
    let name = HSTRING::from(pipe_name(app_id, &sid));
    unsafe {
        // Owned by this user, so a later launch can verify it, even when
        // elevated (an elevated token's default owner is Administrators).
        // Protected DACL: full access for this user, nobody else.
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(format!("O:{sid}D:P(A;;GA;;;{sid})")),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        // FIRST_PIPE_INSTANCE makes creation fail for every process but the
        // first, atomically, so the pipe is also the lock.
        let pipe = CreateNamedPipeW(
            &name,
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            MAX_MESSAGE_BYTES + 4,
            0,
            Some(&attributes),
        );
        let error = Error::from_win32();
        LocalFree(Some(HLOCAL(descriptor.0)));
        if !pipe.is_invalid() {
            return Ok(Role::Primary(OwnedHandle::from_raw_handle(pipe.0)));
        }
        if error.code() == ERROR_ACCESS_DENIED.to_hresult()
            || error.code() == ERROR_PIPE_BUSY.to_hresult()
        {
            return Ok(Role::Secondary);
        }
        Err(error)
    }
}

/// Answer later launches on a background thread. Accepted requests go to
/// `sink`, which buffers them until the window exists.
pub fn serve(pipe: OwnedHandle, app_id: String, sink: Sender<Vec<PathBuf>>) {
    std::thread::spawn(move || loop {
        let handle = HANDLE(pipe.as_raw_handle());
        if let Err(error) = unsafe { ConnectNamedPipe(handle, None) } {
            // A client that connected before this call is already connected.
            if error.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                // A client that came and went leaves the pipe needing a
                // disconnect; without one, every later connect fails too.
                report::log(format_args!("ipc-connect-failed {error}"));
                let _ = unsafe { DisconnectNamedPipe(handle) };
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        }
        // Borrow the pipe as a File for std's Read and Write; never close it.
        let mut stream = ManuallyDrop::new(unsafe { File::from_raw_handle(handle.0) });
        // shortcut: a same-user client that connects and never finishes its
        // message stalls this thread, and with it later launches; they time
        // out with a message. Add overlapped I/O with a deadline if that is
        // ever seen outside deliberate misuse.
        let reply = match read_request(&mut *stream, &app_id) {
            Ok(files) => {
                let _ = sink.send(files);
                ACCEPTED
            }
            Err(error) => {
                report::log(format_args!("ipc-rejected {error}"));
                REJECTED
            }
        };
        let _ = stream.write_all(&[reply]);
        // Wait for the client to read the reply; disconnecting discards it.
        let _ = stream.sync_all();
        let _ = unsafe { DisconnectNamedPipe(handle) };
    });
}

/// Connect to the pipe, waiting while it is not created yet or is busy with
/// another launch.
fn open_pipe(name: &str, deadline: Instant) -> std::io::Result<File> {
    loop {
        match OpenOptions::new().read(true).write(true).open(name) {
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY_CODE)
                ) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            result => return result,
        }
    }
}

pub enum Forwarded {
    Done,
    /// The pipe belongs to someone else: nothing was sent, and this launch
    /// should run on its own.
    NotOurs,
}

/// Hand files to the running instance and let it take the foreground.
/// No files means only "come to the front".
pub fn forward(app_id: &str, files: &[PathBuf]) -> Result<Forwarded, String> {
    let frame = encode_request(app_id, files).map_err(|error| error.to_string())?;
    let sid = user_sid().map_err(|error| error.to_string())?;
    let name = pipe_name(app_id, &sid);
    let mut pipe = match open_pipe(&name, Instant::now() + TIMEOUT) {
        Ok(pipe) => pipe,
        // Our own pipe always admits us; one that refuses us is someone else's.
        Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED_CODE) => {
            return Ok(Forwarded::NotOurs);
        }
        Err(error) => return Err(format!("the running app could not be reached: {error}")),
    };
    if !owned_by(&pipe, &sid) {
        return Ok(Forwarded::NotOurs);
    }
    let mut server = 0;
    unsafe {
        if GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut server).is_ok() {
            let _ = AllowSetForegroundWindow(server);
        }
    }
    // On a thread, so a running app that hangs cannot hang this launch too.
    let (done, outcome) = mpsc::channel();
    std::thread::spawn(move || {
        let mut ack = [REJECTED];
        let result = pipe
            .write_all(&frame)
            .and_then(|()| pipe.read_exact(&mut ack))
            .map(|()| ack[0]);
        let _ = done.send(result);
    });
    match outcome.recv_timeout(TIMEOUT) {
        Ok(Ok(ACCEPTED)) => Ok(Forwarded::Done),
        Ok(Ok(_)) => Err("the running app refused the request".into()),
        Ok(Err(error)) => Err(format!("the running app did not answer: {error}")),
        Err(_) => Err("the running app did not answer within 5 seconds".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_claim_of_an_app_is_primary() {
        let first = claim("0000000000d1test").unwrap();
        assert!(matches!(first, Role::Primary(_)));
        assert!(matches!(
            claim("0000000000d1test").unwrap(),
            Role::Secondary
        ));
        assert!(matches!(
            claim("0000000000d1othr").unwrap(),
            Role::Primary(_)
        ));
        drop(first);
    }

    #[test]
    fn a_forwarded_request_reaches_the_primary_and_is_acknowledged() {
        let app = "0000000000d1fwd0";
        let Role::Primary(pipe) = claim(app).unwrap() else {
            panic!("expected to be primary");
        };
        let (sink, received) = mpsc::channel();
        serve(pipe, app.into(), sink);
        let file = std::env::temp_dir().join("forwarded.psd");
        assert!(matches!(
            forward(app, std::slice::from_ref(&file)),
            Ok(Forwarded::Done)
        ));
        assert_eq!(received.recv_timeout(TIMEOUT).unwrap(), [file]);
        assert!(matches!(forward(app, &[]), Ok(Forwarded::Done)));
        assert!(received.recv_timeout(TIMEOUT).unwrap().is_empty());
    }

    #[test]
    fn the_owner_check_accepts_our_pipe_and_nobody_else() {
        let app = "0000000000d1own0";
        let Role::Primary(_pipe) = claim(app).unwrap() else {
            panic!("expected to be primary");
        };
        let sid = user_sid().unwrap();
        let client = open_pipe(&pipe_name(app, &sid), Instant::now() + TIMEOUT).unwrap();
        assert!(owned_by(&client, &sid));
        assert!(!owned_by(&client, "S-1-5-18"), "LocalSystem is not us");
    }

    #[test]
    fn a_malformed_request_is_refused_and_the_server_keeps_serving() {
        let app = "0000000000d1bad0";
        let Role::Primary(pipe) = claim(app).unwrap() else {
            panic!("expected to be primary");
        };
        let (sink, received) = mpsc::channel();
        serve(pipe, app.into(), sink);
        let name = pipe_name(app, &user_sid().unwrap());
        let send = |bytes: &[u8]| {
            let mut raw = open_pipe(&name, Instant::now() + TIMEOUT).unwrap();
            raw.write_all(bytes).unwrap();
            let mut ack = [9];
            raw.read_exact(&mut ack).unwrap();
            ack[0]
        };
        assert_eq!(send(b"\x05\0\0\0oops!"), REJECTED, "malformed JSON");
        // A request for another app on this app's pipe (gate 7).
        assert_eq!(
            send(&encode_request("ffffffffffffffff", &[]).unwrap()),
            REJECTED,
            "wrong app"
        );
        assert!(matches!(forward(app, &[]), Ok(Forwarded::Done)));
        assert!(received.recv_timeout(TIMEOUT).unwrap().is_empty());
    }
}
