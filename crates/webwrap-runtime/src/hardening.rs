//! WebView2 handlers that close the gaps B's review found in wry and Tauri.
//!
//! They are added after wry's own handlers, so they run after them, and they
//! only ever veto: `SetCancel(true)` or `SetHandled(true)`, never the reverse.
//! That matters because wry's navigation handler calls `SetCancel(!allow)`
//! and Tauri allows a URL it cannot parse.

use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2_18};
use webview2_com::{
    take_pwstr, LaunchingExternalUriSchemeEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler,
};
use webwrap_spec::{nav_verdict, NavEvent, Verdict};
use windows::core::{w, Interface, Result, BOOL, HSTRING, PCWSTR, PWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::report;

/// Hand a URL to the system browser. Only reached for verdict
/// `OpenExternally`, which `nav_verdict` returns for http and https alone.
fn open_externally(url: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn install(webview: &ICoreWebView2) -> Result<()> {
    let mut token = 0;
    unsafe {
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                if !nav_verdict(&uri, NavEvent::TopLevel).is_allow() {
                    args.SetCancel(true)?;
                    report::log(format_args!("nav-blocked {uri}"));
                }
                Ok(())
            })),
            &mut token,
        )?;

        webview.add_FrameNavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                if !nav_verdict(&uri, NavEvent::Frame).is_allow() {
                    args.SetCancel(true)?;
                    report::log(format_args!("frame-blocked {uri}"));
                }
                Ok(())
            })),
            &mut token,
        )?;

        // Catches every route to a Windows protocol handler, frames included,
        // so `search-ms:` and its kind never even reach WebView2's prompt.
        webview
            .cast::<ICoreWebView2_18>()?
            .add_LaunchingExternalUriScheme(
                &LaunchingExternalUriSchemeEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    args.SetCancel(true)?;
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    report::log(format_args!("scheme-blocked {}", take_pwstr(uri)));
                    Ok(())
                })),
                &mut token,
            )?;

        // wry has already denied the pop-up (Tauri has no pop-up hook set).
        // A page may only send the user to the browser when the user asked.
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                args.SetHandled(true)?;
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                let mut user = BOOL::default();
                args.IsUserInitiated(&mut user)?;
                if user.as_bool()
                    && nav_verdict(&uri, NavEvent::NewWindow) == Verdict::OpenExternally
                {
                    open_externally(&uri);
                    report::log(format_args!("popup-external {uri}"));
                } else {
                    report::log(format_args!("popup-denied {uri}"));
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}
