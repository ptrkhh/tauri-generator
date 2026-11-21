//! Moves opened files into the page and saved bytes back to disk, through
//! WebView2 shared memory, under the rules in `webwrap_bridge::Session`.
//!
//! All COM objects live on the UI thread. Workers read and write files and
//! are given only a buffer's address and length; they report back through
//! `run_on_main_thread`, and only then is the buffer closed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::json;
use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment12, ICoreWebView2SharedBuffer, ICoreWebView2_17,
    COREWEBVIEW2_SHARED_BUFFER_ACCESS, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
};
use webview2_com::{
    take_pwstr, ContentLoadingEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, WebMessageReceivedEventHandler,
};
use webwrap_bridge::{
    check_file, handle_refusal, parse_page_message, protected_folders, resolve, save_signature_ok,
    transport, AfterRead, Delivery, PageMessage, Refusal, Session, Tier, Transport, Want,
    LEAVING_TIMEOUT, MAX_FILES, MAX_FILE_BYTES,
};
use webwrap_spec::{Adapter, Assoc};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};

use crate::{photopea, vscode};
use crate::{report, save};

/// The UI thread's WebView2 objects and the shared buffers in use, by token.
struct Ui {
    webview: ICoreWebView2,
    webview17: ICoreWebView2_17,
    environment: ICoreWebView2Environment12,
    buffers: HashMap<u64, ICoreWebView2SharedBuffer>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|ui| ui.borrow_mut().as_mut().map(f))
}

fn live_source(webview: &ICoreWebView2) -> String {
    let mut source = PWSTR::null();
    if unsafe { webview.Source(&mut source) }.is_err() {
        return String::new();
    }
    take_pwstr(source)
}

/// Release our side of a buffer. A page keeps its own view of a buffer that
/// was posted to it; closing here only unmaps the host's.
fn close_buffer(token: u64) {
    if let Some(buffer) = with_ui(|ui| ui.buffers.remove(&token)).flatten() {
        let _ = unsafe { buffer.Close() };
    }
}

/// The address of a buffer, as an integer so a worker thread may carry it.
fn address(buffer: &ICoreWebView2SharedBuffer) -> Result<usize, String> {
    let mut pointer = std::ptr::null_mut();
    unsafe { buffer.Buffer(&mut pointer) }.map_err(|error| error.to_string())?;
    Ok(pointer as usize)
}

fn read_exactly(path: &Path, address: usize, len: u64) -> Result<(), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    // SAFETY: the buffer is at least `len` bytes, nothing else writes to it
    // before it is posted, and it stays mapped until the UI thread closes it,
    // which happens only after this function has returned.
    let target = unsafe { std::slice::from_raw_parts_mut(address as *mut u8, len as usize) };
    file.read_exact(target).map_err(|error| error.to_string())?;
    // A file that grew since its size was taken would be delivered cut short,
    // and saving it would then truncate the original.
    if file.read(&mut [0]).map_err(|error| error.to_string())? != 0 {
        return Err("the file changed while it was being opened".into());
    }
    Ok(())
}

#[derive(Clone)]
pub struct Bridge {
    window: WebviewWindow,
    /// Locked only on the UI thread; the Mutex makes the bridge `Send` for
    /// Tauri's main-thread dispatch.
    session: Arc<Mutex<Session>>,
    /// Saves being written by a worker. Closing the window waits for them:
    /// the process exits when the window goes, which would kill the write.
    writing: Arc<AtomicUsize>,
    close_when_written: Arc<AtomicBool>,
    recheck_scheduled: Arc<AtomicBool>,
    adapter: Adapter,
    /// The app's file types, for the open dialog.
    associations: Arc<Vec<Assoc>>,
    /// The open dialog is showing; a second request is refused.
    picking: Arc<AtomicBool>,
    /// How the current document asked for files (`adapter: none` only).
    /// Cleared with each new document, as the session clears readiness.
    want: Arc<Mutex<Option<Want>>>,
    /// The app's name, for messages about the site.
    site: Arc<str>,
    /// Exists once the open-only notice has been shown (D4).
    notice_flag: Arc<PathBuf>,
}

/// What the page is told when a write fails. The reason, which names local
/// paths, goes only to the user's message box: a page never learns a path.
const WRITE_FAILED: &str = "the file could not be written; the app has told the user why";

impl Bridge {
    /// Install the safety handlers and the bridge, and queue the files the
    /// app was launched with. Failing to install aborts: without the handlers
    /// the app would run with B's known gaps.
    pub fn install(
        window: &WebviewWindow,
        trusted_origins: Vec<String>,
        adapter: Adapter,
        associations: Vec<Assoc>,
        files: Vec<PathBuf>,
        site: &str,
        data_dir: &Path,
    ) -> tauri::Result<Self> {
        let bridge = Bridge {
            window: window.clone(),
            session: Arc::new(Mutex::new(Session::new(trusted_origins))),
            writing: Arc::new(AtomicUsize::new(0)),
            close_when_written: Arc::new(AtomicBool::new(false)),
            recheck_scheduled: Arc::new(AtomicBool::new(false)),
            adapter,
            associations: Arc::new(associations),
            picking: Arc::new(AtomicBool::new(false)),
            want: Arc::new(Mutex::new(None)),
            site: site.into(),
            notice_flag: Arc::new(data_dir.join("open-only-notice")),
        };
        let installer = bridge.clone();
        window.with_webview(move |platform| {
            let installed = unsafe { platform.controller().CoreWebView2() }.and_then(|webview| {
                crate::hardening::install(&webview)?;
                installer.add_handlers(&webview)?;
                Ok(Ui {
                    webview17: webview.cast()?,
                    environment: platform.environment().cast()?,
                    webview,
                    buffers: HashMap::new(),
                })
            });
            match installed {
                Ok(ui) => {
                    UI.with(|slot| *slot.borrow_mut() = Some(ui));
                    installer.open(files);
                }
                Err(error) => report::abort(
                    &format!("this app's safety checks could not be installed: {error}"),
                    true,
                ),
            }
        })?;
        Ok(bridge)
    }

    fn want(&self) -> MutexGuard<'_, Option<Want>> {
        self.want
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn session(&self) -> MutexGuard<'_, Session> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Called when the user closes the window. Returns true if the close must
    /// wait for a save; the window then closes itself when the save is done.
    pub fn hold_close(&self) -> bool {
        if self.writing.load(Ordering::SeqCst) == 0 {
            return false;
        }
        self.close_when_written.store(true, Ordering::SeqCst);
        true
    }

    fn add_handlers(&self, webview: &ICoreWebView2) -> windows::core::Result<()> {
        let mut token = 0;
        let bridge = self.clone();
        unsafe {
            // Added after the hardening handler, so a navigation it or wry
            // cancelled is already marked and is not counted.
            webview.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut cancelled = BOOL::default();
                    args.Cancel(&mut cancelled)?;
                    if cancelled.as_bool() {
                        return Ok(());
                    }
                    let (mut id, mut uri) = (0, PWSTR::null());
                    args.NavigationId(&mut id)?;
                    args.Uri(&mut uri)?;
                    bridge.session().navigation_starting(id, &take_pwstr(uri));
                    Ok(())
                })),
                &mut token,
            )?;
            let bridge = self.clone();
            webview.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                    let (Some(sender), Some(args)) = (sender, args) else {
                        return Ok(());
                    };
                    let mut id = 0;
                    args.NavigationId(&mut id)?;
                    let live = live_source(&sender);
                    let started = bridge.session().navigation_completed(id, &live);
                    bridge.start(started);
                    Ok(())
                })),
                &mut token,
            )?;
            let bridge = self.clone();
            // A new document, before any of its scripts run.
            webview.add_ContentLoading(
                &ContentLoadingEventHandler::create(Box::new(move |sender, args| {
                    let (Some(sender), Some(args)) = (sender, args) else {
                        return Ok(());
                    };
                    let mut id = 0;
                    args.NavigationId(&mut id)?;
                    let source = live_source(&sender);
                    let navigated = bridge.session().document_changed(id, &source);
                    *bridge.want() = None;
                    for token in navigated.dropped_saves {
                        close_buffer(token);
                    }
                    if navigated.revoked > 0 {
                        report::log(format_args!(
                            "revoked {} file tokens; document {source}",
                            navigated.revoked
                        ));
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            let bridge = self.clone();
            webview.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
                    let (Some(sender), Some(args)) = (sender, args) else {
                        return Ok(());
                    };
                    // Bridge messages are JSON text posted as a string. wry's
                    // own handler, which runs first, fails on anything else,
                    // and CI saw object messages never arrive here.
                    let mut text = PWSTR::null();
                    if args.TryGetWebMessageAsString(&mut text).is_err() {
                        return Ok(());
                    }
                    let Some(message) = parse_page_message(&take_pwstr(text)) else {
                        return Ok(());
                    };
                    let mut source = PWSTR::null();
                    args.Source(&mut source)?;
                    bridge.on_message(message, &take_pwstr(source), &live_source(&sender));
                    Ok(())
                })),
                &mut token,
            )?;
        }
        Ok(())
    }

    /// Files from a later launch: bring the window forward and open them.
    pub fn open_forwarded(&self, files: Vec<PathBuf>) {
        let _ = self.window.unminimize();
        let _ = self.window.show();
        let _ = self.window.set_focus();
        let bridge = self.clone();
        let _ = self.window.run_on_main_thread(move || bridge.open(files));
    }

    fn open(&self, files: Vec<PathBuf>) {
        let Some(live) = with_ui(|ui| live_source(&ui.webview)) else {
            return;
        };
        let mut bad = Vec::new();
        for path in files {
            // Checked again here: a forwarded file may have gone meanwhile.
            if let Err(reason) = check_file(&path) {
                bad.push(format!("{}: {reason}", path.display()));
                continue;
            }
            let started = self.session().open(path, &live);
            self.start(started);
        }
        if !bad.is_empty() {
            report::error_box_async(format!(
                "These files could not be opened:\n\n{}",
                bad.join("\n")
            ));
        }
    }

    fn on_message(&self, message: PageMessage, source: &str, live: &str) {
        match message {
            PageMessage::Ready { want } => {
                let ready = self.session().ready(source, live);
                match ready {
                    Ok(deliveries) => {
                        if self.adapter == Adapter::None {
                            *self.want() = want;
                        }
                        report::log(format_args!(
                            "page-ready {} queued; {source}",
                            deliveries.len()
                        ));
                        self.start(deliveries)
                    }
                    Err(refusal) => {
                        report::log(format_args!("delivery-refused {refusal}; {source}"))
                    }
                }
            }
            PageMessage::Save { token, size } => {
                let begun = self.session().begin_save(source, live, &token, size);
                match begun {
                    Ok((id, nonce)) => {
                        if let Err(error) = self.post_save_buffer(id, nonce, &token, size) {
                            self.finish_save(id, token, Err(error));
                        }
                    }
                    Err(refusal) => self.refuse_save(&token, refusal),
                }
            }
            PageMessage::SaveCommit { token, save } => {
                let committed = self.session().commit_save(source, live, &token, save);
                match committed {
                    Ok((id, path)) => self.write(id, token, path),
                    Err(refusal) => self.refuse_save(&token, refusal),
                }
            }
            PageMessage::OpenFailed { tokens } => {
                let (site, advice) = match self.adapter {
                    Adapter::VscodeDev => (
                        "vscode.dev",
                        "They are waiting. Choose File > Open File... in vscode.dev to open them.",
                    ),
                    Adapter::Photopea => ("Photopea", "It may not support their format."),
                    Adapter::None => (
                        &*self.site,
                        "This website may not support opening files from Windows.",
                    ),
                };
                let named: Result<Vec<String>, Refusal> = {
                    let session = self.session();
                    tokens
                        .iter()
                        .take(MAX_FILES)
                        .map(|token| {
                            session
                                .delivered_name(source, live, token)
                                .map(str::to_owned)
                        })
                        .collect()
                };
                match named {
                    Ok(names) if !names.is_empty() => {
                        report::log(format_args!(
                            "open-failed {}: {site} did not open it",
                            names.join(", ")
                        ));
                        report::error_box_async(format!(
                            "{site} did not open these files:\n\n{}\n\n{advice}",
                            names.join("\n")
                        ));
                    }
                    Ok(_) => report::log("open-failed-refused no file was named"),
                    Err(refusal) => {
                        report::log(format_args!("open-failed-refused {refusal}; {source}"))
                    }
                }
            }
            PageMessage::FileTier { token, tier } => {
                if self.adapter != Adapter::None {
                    return report::log("file-tier-refused this app has an adapter");
                }
                let named = self
                    .session()
                    .file_tier(source, live, &token, tier)
                    .map(str::to_owned);
                match named {
                    Ok(name) => {
                        let number = if tier == Tier::Upload { 3 } else { 1 };
                        report::log(format_args!("file-tier {number} {name} ({})", tier.label()));
                        if tier == Tier::Upload {
                            self.open_only_notice(&name);
                        }
                    }
                    Err(refusal) => {
                        report::log(format_args!("file-tier-refused {refusal}; {source}"))
                    }
                }
            }
            PageMessage::PickFile => {
                if self.adapter != Adapter::Photopea {
                    return report::log("pick-file-refused this app has no Photopea adapter");
                }
                if !(self.session().trusts(source) && self.session().trusts(live)) {
                    return report::log(format_args!(
                        "pick-file-refused {}; {source}",
                        Refusal::Untrusted
                    ));
                }
                if self.picking.swap(true, Ordering::SeqCst) {
                    return report::log("pick-file-refused the open dialog is already showing");
                }
                // Shown once this event has returned, not from inside it. On
                // the main thread, run_on_main_thread would run it right here,
                // so the request comes from another thread.
                let bridge = self.clone();
                std::thread::spawn(move || {
                    let (window, picking) = (bridge.window.clone(), bridge.picking.clone());
                    if window
                        .run_on_main_thread(move || bridge.pick_files())
                        .is_err()
                    {
                        picking.store(false, Ordering::SeqCst);
                    }
                });
            }
        }
    }

    // ---- Opening ---------------------------------------------------------

    /// The first file a site takes as an upload explains, once per app, that
    /// saving will not reach the original. Creating the flag file decides it,
    /// so two uploads at once show one notice; if it cannot be created, the
    /// notice shows again next time.
    fn open_only_notice(&self, name: &str) {
        let created = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&*self.notice_flag);
        if matches!(&created, Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists) {
            return;
        }
        report::log("open-only-notice shown");
        let site = &self.site;
        report::info_box_async(format!(
            "{site} opened {name} as an upload. Saving in {site} downloads a copy; \
             it does not change the original file.\n\nThis notice won't show again."
        ));
    }

    fn start(&self, deliveries: Vec<Delivery>) {
        for delivery in deliveries {
            let allowed = handle_refusal(&delivery.path, &protected_folders()).is_none();
            // Bound first: a lock taken in the match would be held through it.
            let want = *self.want();
            let started = match transport(self.adapter, want, allowed) {
                Transport::Handle => self.post_handle(&delivery),
                Transport::Buffer => self.begin_read(&delivery),
            };
            if let Err(error) = started {
                self.delivery_failed(&delivery, error);
            }
        }
        self.recheck_if_held();
    }

    /// If a pending untrusted navigation holds files back, look again once it
    /// must have expired, in case WebView2 never reports that it ended.
    fn recheck_if_held(&self) {
        if !self.session().held() || self.recheck_scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        report::log("deliveries-held an untrusted navigation is pending");
        let bridge = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(LEAVING_TIMEOUT + Duration::from_secs(1));
            let window = bridge.window.clone();
            let _ = window.run_on_main_thread(move || {
                bridge.recheck_scheduled.store(false, Ordering::SeqCst);
                let Some(live) = with_ui(|ui| live_source(&ui.webview)) else {
                    return;
                };
                let released = bridge.session().release_held(&live);
                bridge.start(released);
            });
        });
    }

    fn begin_read(&self, delivery: &Delivery) -> Result<(), String> {
        let len = std::fs::metadata(&delivery.path)
            .map_err(|error| error.to_string())?
            .len();
        if len > MAX_FILE_BYTES {
            return Err("it is larger than 4 GiB".into());
        }
        let address = with_ui(|ui| {
            // WebView2 refuses an empty buffer; the page is told the real size.
            let buffer = unsafe { ui.environment.CreateSharedBuffer(len.max(1)) }
                .map_err(|error| error.to_string())?;
            let address = address(&buffer)?;
            ui.buffers.insert(delivery.token, buffer);
            Ok::<_, String>(address)
        })
        .ok_or("the window is gone")??;
        let bridge = self.clone();
        let delivery = delivery.clone();
        std::thread::spawn(move || {
            let read = read_exactly(&delivery.path, address, len);
            let window = bridge.window.clone();
            let _ = window.run_on_main_thread(move || bridge.finish_read(delivery, len, read));
        });
        Ok(())
    }

    fn finish_read(&self, delivery: Delivery, len: u64, read: Result<(), String>) {
        if let Err(error) = read {
            close_buffer(delivery.token);
            return self.delivery_failed(&delivery, error);
        }
        let Some(live) = with_ui(|ui| live_source(&ui.webview)) else {
            return;
        };
        // Decided immediately before posting.
        let after = self
            .session()
            .delivery_read(delivery.token, &live, Transport::Buffer);
        match after {
            AfterRead::Post => {
                let info = json!({
                    "type": "webwrap-file",
                    "token": delivery.token.to_string(),
                    "name": delivery.name,
                    "size": len,
                });
                let posted = self.post_buffer(
                    delivery.token,
                    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
                    &info,
                );
                close_buffer(delivery.token);
                match posted {
                    Ok(()) => report::log(format_args!("delivered {}", delivery.name)),
                    Err(error) => self.delivery_failed(&delivery, error),
                }
            }
            AfterRead::Retry(more) => {
                close_buffer(delivery.token);
                self.start(more);
            }
        }
    }

    /// A read-write handle instead of the bytes: always for `vscodeDev`, and
    /// for a generic site whose `launchQueue` consumer asked for one.
    fn post_handle(&self, delivery: &Delivery) -> Result<(), String> {
        // Told now, rather than by a security error inside vscode.dev.
        if let Some(reason) = handle_refusal(&delivery.path, &protected_folders()) {
            return Err(reason.into());
        }
        // Created before the file counts as delivered, so a generic site can
        // still be given the bytes instead.
        let handle = with_ui(|ui| vscode::create_handle(&ui.environment.cast()?, &delivery.path))
            .ok_or("the window is gone")?;
        let handle = match handle {
            Ok(handle) => handle,
            Err(error) if self.adapter == Adapter::None => {
                report::log(format_args!(
                    "handle-refused {}: {error}; sending the bytes",
                    delivery.name
                ));
                return self.begin_read(delivery);
            }
            Err(error) => return Err(error.to_string()),
        };
        let live = with_ui(|ui| live_source(&ui.webview)).ok_or("the window is gone")?;
        // Decided immediately before posting.
        let after = self
            .session()
            .delivery_read(delivery.token, &live, Transport::Handle);
        if let AfterRead::Retry(more) = after {
            self.start(more);
            return Ok(());
        }
        let message = json!({
            "type": "webwrap-file-handle",
            "token": delivery.token.to_string(),
            "name": delivery.name,
        });
        with_ui(|ui| {
            let environment = ui.environment.cast()?;
            let webview = ui.webview.cast()?;
            vscode::post_handle(&environment, &webview, &handle, &message.to_string())
        })
        .ok_or("the window is gone")?
        .map_err(|error| error.to_string())?;
        report::log(format_args!("delivered {}", delivery.name));
        Ok(())
    }

    /// Ctrl+O in Photopea: the native open dialog. Chosen files open exactly
    /// like files from a later launch.
    fn pick_files(&self) {
        report::log("pick-file shown");
        let chosen = self
            .window
            .hwnd()
            .map_err(|error| {
                windows::core::Error::new(windows::core::HRESULT(-1), error.to_string())
            })
            .and_then(|hwnd| photopea::pick_files(hwnd, &self.associations));
        self.picking.store(false, Ordering::SeqCst);
        let chosen = match chosen {
            Ok(chosen) => chosen,
            Err(error) => {
                report::log(format_args!("pick-file-failed {error}"));
                return report::error_box_async(format!(
                    "The open dialog could not be shown.\n\n{error}"
                ));
            }
        };
        report::log(format_args!("pick-file {} chosen", chosen.len()));
        let (mut files, mut bad) = (Vec::new(), Vec::new());
        if chosen.len() > MAX_FILES {
            bad.push(format!(
                "Only the first {MAX_FILES} of the chosen files were opened."
            ));
        }
        for path in chosen.into_iter().take(MAX_FILES) {
            match resolve(path.as_os_str()) {
                Ok(path) => files.push(path),
                Err(reason) => bad.push(format!("{}: {reason}", path.display())),
            }
        }
        if !bad.is_empty() {
            report::error_box_async(format!(
                "These files could not be opened:\n\n{}",
                bad.join("\n")
            ));
        }
        self.open(files);
    }

    fn delivery_failed(&self, delivery: &Delivery, error: String) {
        self.session().delivery_failed(delivery.token);
        report::log(format_args!("open-failed {}: {error}", delivery.name));
        report::error_box_async(format!(
            "{} could not be opened.\n\n{error}",
            delivery.path.display()
        ));
    }

    fn post_buffer(
        &self,
        token: u64,
        access: COREWEBVIEW2_SHARED_BUFFER_ACCESS,
        info: &serde_json::Value,
    ) -> Result<(), String> {
        with_ui(|ui| {
            let buffer = ui.buffers.get(&token).ok_or("the buffer is gone")?;
            unsafe {
                ui.webview17.PostSharedBufferToScript(
                    buffer,
                    access,
                    &HSTRING::from(info.to_string()),
                )
            }
            .map_err(|error| error.to_string())
        })
        .ok_or("the window is gone")?
    }

    // ---- Saving ----------------------------------------------------------

    fn post_save_buffer(&self, id: u64, nonce: u64, token: &str, size: u64) -> Result<(), String> {
        with_ui(|ui| {
            let buffer = unsafe { ui.environment.CreateSharedBuffer(size) }
                .map_err(|error| error.to_string())?;
            ui.buffers.insert(id, buffer);
            Ok::<_, String>(())
        })
        .ok_or("the window is gone")??;
        let info = json!({
            "type": "webwrap-save-buffer",
            "token": token,
            "size": size,
            "save": nonce,
        });
        let posted = self.post_buffer(id, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE, &info);
        if posted.is_err() {
            close_buffer(id);
        }
        posted
    }

    fn write(&self, id: u64, token: String, path: PathBuf) {
        let region = with_ui(|ui| {
            let buffer = ui.buffers.get(&id).ok_or("the save buffer is gone")?;
            let mut size = 0;
            unsafe { buffer.Size(&mut size) }.map_err(|error| error.to_string())?;
            Ok::<_, String>((address(buffer)?, size))
        })
        .unwrap_or(Err("the window is gone".into()));
        let (address, size) = match region {
            Ok(region) => region,
            Err(error) => return self.finish_save(id, token, Err(error)),
        };
        let bridge = self.clone();
        let adapter = self.adapter;
        self.writing.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            // SAFETY: the buffer is `size` bytes and stays mapped until the UI
            // thread closes it in `finish_save`, after this write. The page
            // can still change it meanwhile, which only alters its own save.
            let bytes = unsafe { std::slice::from_raw_parts(address as *const u8, size as usize) };
            // Photopea exports one format per save; it must be the one the
            // file's extension names.
            let checked = match adapter {
                Adapter::Photopea => save_signature_ok(&path, bytes),
                _ => Ok(()),
            };
            let written = checked
                .and_then(|()| save::replace(&path, bytes).map_err(|error| error.to_string()))
                .map_err(|error| format!("{} could not be saved.\n\n{error}", path.display()));
            let window = bridge.window.clone();
            let _ = window.run_on_main_thread(move || {
                if written.is_err() {
                    // Stay open: the user must see the failure, and the page
                    // still holds the unsaved edits.
                    bridge.close_when_written.store(false, Ordering::SeqCst);
                }
                bridge.finish_save(id, token, written);
                let last = bridge.writing.fetch_sub(1, Ordering::SeqCst) == 1;
                if last && bridge.close_when_written.load(Ordering::SeqCst) {
                    let _ = bridge.window.close();
                }
            });
        });
    }

    fn finish_save(&self, id: u64, token: String, result: Result<(), String>) {
        close_buffer(id);
        self.session().save_ended(id);
        let reply = match &result {
            Ok(()) => json!({ "type": "webwrap-save-result", "token": token, "ok": true }),
            Err(error) => {
                report::log(format_args!("save-failed {error}"));
                report::error_box_async(error.clone());
                json!({
                    "type": "webwrap-save-result",
                    "token": token,
                    "ok": false,
                    "error": WRITE_FAILED,
                })
            }
        };
        self.reply(&reply);
    }

    fn refuse_save(&self, token: &str, refusal: Refusal) {
        report::log(format_args!("save-refused {refusal}; token {token}"));
        // An untrusted page learns nothing, not even that the token existed.
        if refusal == Refusal::Untrusted {
            return;
        }
        if refusal == Refusal::Size {
            let name = token
                .parse()
                .ok()
                .and_then(|id| self.session().name(id).map(str::to_owned));
            if let Some(name) = name {
                report::error_box_async(format!("{name} could not be saved: {refusal}."));
            }
        }
        self.reply(&json!({
            "type": "webwrap-save-result",
            "token": token,
            "ok": false,
            "error": refusal.to_string(),
        }));
    }

    /// Post a message to the page, only while it is on a trusted origin.
    fn reply(&self, message: &serde_json::Value) {
        let Some(live) = with_ui(|ui| live_source(&ui.webview)) else {
            return;
        };
        // Replies carry no file data, so a pending navigation does not hold
        // them; holding could leave a page waiting forever for its result.
        if !self.session().trusts(&live) {
            return;
        }
        with_ui(|ui| unsafe {
            let _ = ui
                .webview
                .PostWebMessageAsJson(&HSTRING::from(message.to_string()));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::read_exactly;

    #[test]
    fn a_file_that_changed_size_is_refused_rather_than_delivered_short() {
        let path = std::env::temp_dir().join(format!("webwrap-read-{}.bin", std::process::id()));
        std::fs::write(&path, b"0123456789").unwrap();
        let mut target = vec![0u8; 10];
        read_exactly(&path, target.as_mut_ptr() as usize, 10).unwrap();
        assert_eq!(target, b"0123456789");
        // Grew since its size was taken: the tail would be lost.
        assert!(read_exactly(&path, target.as_mut_ptr() as usize, 8)
            .unwrap_err()
            .contains("changed"));
        // Shrank: there is not enough to fill the buffer.
        let mut bigger = vec![0u8; 12];
        assert!(read_exactly(&path, bigger.as_mut_ptr() as usize, 12).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
