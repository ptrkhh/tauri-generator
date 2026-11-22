//! Which page may receive which file, and which saves may happen.
//!
//! A token is created only for a path the user opened. A page sees tokens and
//! display names, never paths. Delivery and saving need both the message's own
//! source and the live top-level source to be trusted origins, and a document
//! must announce itself ready before anything is posted to it.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Deserialize;
use webwrap_spec::is_trusted_origin;

use crate::paths::{display_name, MAX_FILE_BYTES};
use crate::tier::{Tier, Transport, Want};

/// How long a pending untrusted navigation may hold deliveries back. It ends
/// sooner when WebView2 reports the navigation finished or a new document; this
/// is the backstop if that report never comes.
pub const LEAVING_TIMEOUT: Duration = Duration::from_secs(30);

/// Page messages are tiny; anything larger is not ours and is not parsed.
const MAX_PAGE_MESSAGE_BYTES: usize = 4096;

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "type")]
pub enum PageMessage {
    /// `want` is honoured only for `adapter: none`; missing means a buffer.
    #[serde(rename = "webwrap-ready")]
    Ready { want: Option<Want> },
    #[serde(rename = "webwrap-save")]
    Save { token: String, size: u64 },
    /// `save` echoes the number in `webwrap-save-buffer`, so a late commit
    /// can never fill in for a different save of the same file.
    #[serde(rename = "webwrap-save-commit")]
    SaveCommit { token: String, save: u64 },
    /// The `vscodeDev` adapter could not get vscode.dev to open these
    /// delivered files. The app tells the user.
    #[serde(rename = "webwrap-open-failed")]
    OpenFailed { tokens: Vec<String> },
    /// The `photopea` shell asks for the native open dialog (Ctrl+O).
    #[serde(rename = "webwrap-pick-file")]
    PickFile,
    /// A generic page's script says which tier took a delivered file.
    #[serde(rename = "webwrap-file-tier")]
    FileTier { token: String, tier: Tier },
}

/// Parse the JSON text a page posted as a string with
/// `chrome.webview.postMessage(JSON.stringify(message))`. Anything else a site
/// posts is not addressed to the bridge and yields `None`.
pub fn parse_page_message(json: &str) -> Option<PageMessage> {
    if json.len() > MAX_PAGE_MESSAGE_BYTES {
        return None;
    }
    serde_json::from_str(json).ok()
}

/// A file to read into a shared buffer and post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub token: u64,
    pub path: PathBuf,
    pub name: String,
}

/// What to do once a delivery's bytes have been read.
#[derive(Debug, PartialEq, Eq)]
pub enum AfterRead {
    /// Post it now.
    Post,
    /// Do not post: the document changed, or is about to change to an
    /// untrusted origin. The file is queued again, and these
    /// deliveries (possibly none) should be started instead.
    Retry(Vec<Delivery>),
}

/// The effect of a new top-level document.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Navigated {
    /// Delivered tokens that no longer work.
    pub revoked: usize,
    /// Save buffers that were never committed; the runtime closes them.
    pub dropped_saves: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Untrusted,
    UnknownToken,
    Size,
    InFlight,
    NotAwaiting,
    WrongTier,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Untrusted => "the page is not on a trusted origin",
            Self::UnknownToken => "the file is not open in this page, or access was revoked",
            Self::Size => "the file must be between 1 byte and 4 GiB",
            Self::InFlight => "a save of this file is already in progress",
            Self::NotAwaiting => "no save of this file is waiting for its bytes",
            Self::WrongTier => "the file was not delivered that way",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Save {
    Idle,
    AwaitingCommit { generation: u64, nonce: u64 },
    Writing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Queued,
    Delivering { generation: u64 },
    Delivered(Save),
}

struct Entry {
    path: PathBuf,
    name: String,
    state: State,
    /// How it was last posted, once delivered.
    via: Option<Transport>,
}

pub struct Session {
    trusted_origins: Vec<String>,
    entries: BTreeMap<u64, Entry>,
    next_token: u64,
    /// Counts top-level navigations, so work started for one document is
    /// never finished into the next.
    generation: u64,
    /// The current document said `webwrap-ready`.
    ready: bool,
    next_save: u64,
    /// Allowed top-level navigations to untrusted origins that have neither
    /// produced a document nor ended, with when each started. Nothing is
    /// posted while one younger than `leaving_timeout` exists.
    leaving: BTreeMap<u64, Instant>,
    leaving_timeout: Duration,
}

impl Session {
    pub fn new(trusted_origins: Vec<String>) -> Self {
        Self {
            trusted_origins,
            entries: BTreeMap::new(),
            next_token: 1,
            generation: 0,
            ready: false,
            next_save: 1,
            leaving: BTreeMap::new(),
            leaving_timeout: LEAVING_TIMEOUT,
        }
    }

    /// Whether a page at `url` may receive files or be told about them.
    pub fn trusts(&self, url: &str) -> bool {
        is_trusted_origin(&self.trusted_origins, url)
    }

    /// Whether anything may be posted to a page at `url` right now.
    pub fn may_post(&self, url: &str) -> bool {
        self.trusts(url) && !self.leaving_pending()
    }

    fn leaving_pending(&self) -> bool {
        self.leaving
            .values()
            .any(|started| started.elapsed() < self.leaving_timeout)
    }

    /// Files queued for a ready page and held back by a pending navigation.
    /// The runtime checks again after `LEAVING_TIMEOUT`.
    pub fn held(&self) -> bool {
        self.ready
            && self.leaving_pending()
            && self
                .entries
                .values()
                .any(|entry| entry.state == State::Queued)
    }

    /// Start whatever a pending navigation held back, if nothing holds it now.
    pub fn release_held(&mut self, live: &str) -> Vec<Delivery> {
        if self.ready && self.may_post(live) {
            self.leaving.clear();
            self.start_queued()
        } else {
            Vec::new()
        }
    }

    fn check_sources(&self, source: &str, live: &str) -> Result<(), Refusal> {
        if self.trusts(source) && self.trusts(live) {
            Ok(())
        } else {
            Err(Refusal::Untrusted)
        }
    }

    /// Queue a file the user opened. Returns deliveries to start now.
    pub fn open(&mut self, path: PathBuf, live: &str) -> Vec<Delivery> {
        let token = self.next_token;
        self.next_token += 1;
        let name = display_name(&path);
        self.entries.insert(
            token,
            Entry {
                path,
                name,
                state: State::Queued,
                via: None,
            },
        );
        if self.ready && self.may_post(live) {
            self.start_queued()
        } else {
            Vec::new()
        }
    }

    /// The current document is listening. Returns every queued file.
    pub fn ready(&mut self, source: &str, live: &str) -> Result<Vec<Delivery>, Refusal> {
        self.check_sources(source, live)?;
        self.ready = true;
        Ok(if self.leaving_pending() {
            Vec::new()
        } else {
            self.start_queued()
        })
    }

    fn start_queued(&mut self) -> Vec<Delivery> {
        let generation = self.generation;
        self.entries
            .iter_mut()
            .filter(|(_, entry)| entry.state == State::Queued)
            .map(|(&token, entry)| {
                entry.state = State::Delivering { generation };
                Delivery {
                    token,
                    path: entry.path.clone(),
                    name: entry.name.clone(),
                }
            })
            .collect()
    }

    /// A delivery is about to be posted `via` a handle or buffer. Decide
    /// immediately before posting.
    pub fn delivery_read(&mut self, token: u64, live: &str, via: Transport) -> AfterRead {
        let current = self.generation;
        let postable = self.may_post(live);
        let Some(entry) = self.entries.get_mut(&token) else {
            return AfterRead::Retry(Vec::new());
        };
        match entry.state {
            State::Delivering { generation } if generation == current && postable => {
                entry.state = State::Delivered(Save::Idle);
                entry.via = Some(via);
                AfterRead::Post
            }
            State::Delivering { .. } => {
                entry.state = State::Queued;
                if self.ready && postable {
                    AfterRead::Retry(self.start_queued())
                } else {
                    AfterRead::Retry(Vec::new())
                }
            }
            _ => AfterRead::Retry(Vec::new()),
        }
    }

    /// A delivery could not be read; the file is dropped.
    pub fn delivery_failed(&mut self, token: u64) {
        self.entries.remove(&token);
    }

    /// A top-level navigation was allowed to start. WebView2 holds it until
    /// every `NavigationStarting` handler returns, so this runs before the new
    /// document can exist, while the live source may still read as trusted.
    /// That closes the gap between checking the source and posting.
    pub fn navigation_starting(&mut self, navigation: u64, url: &str) {
        if !self.trusts(url) {
            self.leaving.insert(navigation, Instant::now());
        }
    }

    /// A navigation ended: loaded, failed, cancelled, or turned into a
    /// download. Returns deliveries that were held back and may start now.
    pub fn navigation_completed(&mut self, navigation: u64, live: &str) -> Vec<Delivery> {
        self.leaving.remove(&navigation);
        self.release_held(live)
    }

    /// A new top-level document is loading (WebView2 `ContentLoading`), before
    /// any of its scripts run. It must announce itself again, uncommitted saves
    /// are dropped, and a document outside the trusted origins revokes every
    /// delivered token. Queued files were never exposed and stay queued.
    ///
    /// This is keyed on the document actually changing, not on a navigation
    /// starting: a navigation that becomes a download, or is cancelled, leaves
    /// the trusted document in place and must not revoke its files.
    pub fn document_changed(&mut self, navigation: u64, url: &str) -> Navigated {
        // This navigation, and any older one it superseded, is over.
        self.leaving.retain(|&id, _| id > navigation);
        self.generation += 1;
        self.ready = false;
        let trusted = self.trusts(url);
        let mut navigated = Navigated::default();
        self.entries.retain(|&token, entry| {
            let State::Delivered(save) = &mut entry.state else {
                return true;
            };
            if matches!(save, Save::AwaitingCommit { .. }) {
                navigated.dropped_saves.push(token);
                *save = Save::Idle;
            }
            if !trusted {
                navigated.revoked += 1;
            }
            trusted
        });
        navigated
    }

    fn delivered(&mut self, token: &str) -> Result<(u64, &mut Entry), Refusal> {
        let id: u64 = token.parse().map_err(|_| Refusal::UnknownToken)?;
        if id.to_string() != token {
            return Err(Refusal::UnknownToken);
        }
        match self.entries.get_mut(&id) {
            Some(entry) if matches!(entry.state, State::Delivered(_)) => Ok((id, entry)),
            _ => Err(Refusal::UnknownToken),
        }
    }

    /// The page asks to save `size` bytes to a delivered file.
    pub fn begin_save(
        &mut self,
        source: &str,
        live: &str,
        token: &str,
        size: u64,
    ) -> Result<(u64, u64), Refusal> {
        self.check_sources(source, live)?;
        let generation = self.generation;
        let nonce = self.next_save;
        self.next_save += 1;
        let (id, entry) = self.delivered(token)?;
        if size == 0 || size > MAX_FILE_BYTES {
            return Err(Refusal::Size);
        }
        if entry.state != State::Delivered(Save::Idle) {
            return Err(Refusal::InFlight);
        }
        entry.state = State::Delivered(Save::AwaitingCommit { generation, nonce });
        Ok((id, nonce))
    }

    /// The page filled the save buffer. Returns where to write.
    pub fn commit_save(
        &mut self,
        source: &str,
        live: &str,
        token: &str,
        save: u64,
    ) -> Result<(u64, PathBuf), Refusal> {
        self.check_sources(source, live)?;
        let current = self.generation;
        let (id, entry) = self.delivered(token)?;
        match entry.state {
            State::Delivered(Save::AwaitingCommit { generation, nonce })
                if generation == current && nonce == save =>
            {
                entry.state = State::Delivered(Save::Writing);
                Ok((id, entry.path.clone()))
            }
            _ => Err(Refusal::NotAwaiting),
        }
    }

    /// A save finished or was abandoned, either way. A revoked token stays
    /// revoked.
    pub fn save_ended(&mut self, token: u64) {
        if let Some(entry) = self.entries.get_mut(&token) {
            if let State::Delivered(save) = &mut entry.state {
                *save = Save::Idle;
            }
        }
    }

    pub fn name(&self, token: u64) -> Option<&str> {
        self.entries.get(&token).map(|entry| entry.name.as_str())
    }

    /// The name of a file delivered to the trusted page that names it.
    pub fn delivered_name(&self, source: &str, live: &str, token: &str) -> Result<&str, Refusal> {
        self.check_sources(source, live)?;
        let id: u64 = token.parse().map_err(|_| Refusal::UnknownToken)?;
        match self.entries.get(&id) {
            Some(entry)
                if id.to_string() == token && matches!(entry.state, State::Delivered(_)) =>
            {
                Ok(&entry.name)
            }
            _ => Err(Refusal::UnknownToken),
        }
    }

    /// A generic page reports which tier took a delivered file. Tier 1 is
    /// accepted only after a handle and tier 3 only after a buffer. Returns
    /// the file's name.
    pub fn file_tier(
        &self,
        source: &str,
        live: &str,
        token: &str,
        tier: Tier,
    ) -> Result<&str, Refusal> {
        let name = self.delivered_name(source, live, token)?;
        let expected = match tier {
            Tier::LaunchQueue => Transport::Handle,
            Tier::Upload => Transport::Buffer,
            _ => return Err(Refusal::WrongTier),
        };
        let via = token.parse().ok().and_then(|id| self.entries.get(&id)?.via);
        if via == Some(expected) {
            Ok(name)
        } else {
            Err(Refusal::WrongTier)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "https://app.test/";
    const AWAY: &str = "https://evil.test/";
    /// A navigation id; these tests have no navigation in progress.
    const NAV: u64 = 1;

    /// Begin a save and return its token and nonce as the page would see them.
    fn begin(s: &mut Session, token: &str, size: u64) -> Result<(u64, u64), Refusal> {
        s.begin_save(HOME, HOME, token, size)
    }

    fn session() -> Session {
        Session::new(vec!["https://app.test".into()])
    }

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    /// Open a file and deliver it to a ready trusted page.
    fn delivered(session: &mut Session, name: &str) -> u64 {
        let started = session.open(path(name), HOME);
        let started = if started.is_empty() {
            session.ready(HOME, HOME).unwrap()
        } else {
            started
        };
        let token = started.last().unwrap().token;
        assert_eq!(
            session.delivery_read(token, HOME, Transport::Buffer),
            AfterRead::Post
        );
        token
    }

    #[test]
    fn page_messages_parse_and_anything_else_is_ignored() {
        assert_eq!(
            parse_page_message(r#"{"type":"webwrap-ready"}"#),
            Some(PageMessage::Ready { want: None })
        );
        assert_eq!(
            parse_page_message(r#"{"type":"webwrap-ready","want":"handle"}"#),
            Some(PageMessage::Ready {
                want: Some(Want::Handle)
            })
        );
        assert_eq!(
            parse_page_message(r#"{"type":"webwrap-ready","want":"buffer"}"#),
            Some(PageMessage::Ready {
                want: Some(Want::Buffer)
            })
        );
        for (tier, expected) in [(1, Tier::LaunchQueue), (3, Tier::Upload)] {
            assert_eq!(
                parse_page_message(&format!(
                    r#"{{"type":"webwrap-file-tier","token":"3","tier":{tier}}}"#
                )),
                Some(PageMessage::FileTier {
                    token: "3".into(),
                    tier: expected
                })
            );
        }
        assert_eq!(
            parse_page_message(r#"{"type":"webwrap-save","token":"3","size":12}"#),
            Some(PageMessage::Save {
                token: "3".into(),
                size: 12
            })
        );
        assert_eq!(
            parse_page_message(r#"{"type":"webwrap-save-commit","token":"3","save":9}"#),
            Some(PageMessage::SaveCommit {
                token: "3".into(),
                save: 9
            })
        );
        assert_eq!(
            parse_page_message(r#"{"type":"webwrap-pick-file"}"#),
            Some(PageMessage::PickFile)
        );
        assert_eq!(
            parse_page_message(r#"{"type":"webwrap-open-failed","tokens":["3","4"]}"#),
            Some(PageMessage::OpenFailed {
                tokens: vec!["3".into(), "4".into()]
            })
        );
        for other in [
            r#"{"type":"site-message"}"#,
            r#""webwrap-ready""#,
            r#"{"type":"webwrap-save","token":"3","size":-1}"#,
            r#"{"type":"webwrap-save","token":"3","size":1.5}"#,
            r#"{"type":"webwrap-save-commit","token":"3"}"#,
            r#"{"type":"webwrap-ready","want":"bytes"}"#,
            r#"{"type":"webwrap-file-tier","token":"3","tier":0}"#,
            r#"{"type":"webwrap-file-tier","token":"3","tier":2}"#,
            r#"{"type":"webwrap-file-tier","token":"3","tier":4}"#,
            r#"{"type":"webwrap-file-tier","token":"3","tier":"1"}"#,
            r#"{"type":"webwrap-file-tier","token":"3"}"#,
            "not json",
        ] {
            assert_eq!(parse_page_message(other), None, "{other}");
        }
        let padded = format!(r#"{{"type":"webwrap-ready","pad":"{}"}}"#, "x".repeat(5000));
        assert_eq!(parse_page_message(&padded), None);
    }

    #[test]
    fn a_tier_is_accepted_only_for_how_the_file_was_delivered() {
        let mut s = session();
        let buffer = delivered(&mut s, "a.png");
        // The page is ready now, so opening starts the delivery at once.
        let handle = s.open(path("b.excalidraw"), HOME)[0].token;
        assert_eq!(
            s.delivery_read(handle, HOME, Transport::Handle),
            AfterRead::Post
        );
        let (buffer, handle) = (buffer.to_string(), handle.to_string());

        assert_eq!(s.file_tier(HOME, HOME, &buffer, Tier::Upload), Ok("a.png"));
        assert_eq!(
            s.file_tier(HOME, HOME, &handle, Tier::LaunchQueue),
            Ok("b.excalidraw")
        );
        assert_eq!(
            s.file_tier(HOME, HOME, &buffer, Tier::LaunchQueue),
            Err(Refusal::WrongTier)
        );
        assert_eq!(
            s.file_tier(HOME, HOME, &handle, Tier::Upload),
            Err(Refusal::WrongTier)
        );
        assert_eq!(
            s.file_tier(HOME, HOME, &buffer, Tier::Unsupported),
            Err(Refusal::WrongTier)
        );
        assert_eq!(
            s.file_tier(AWAY, HOME, &buffer, Tier::Upload),
            Err(Refusal::Untrusted)
        );

        // Started but not yet delivered.
        let queued = s.open(path("c.png"), HOME)[0].token.to_string();
        assert_eq!(
            s.file_tier(HOME, HOME, &queued, Tier::Upload),
            Err(Refusal::UnknownToken)
        );
        // Revoked by an untrusted document.
        s.document_changed(NAV, AWAY);
        assert_eq!(
            s.file_tier(HOME, HOME, &buffer, Tier::Upload),
            Err(Refusal::UnknownToken)
        );
    }

    #[test]
    fn nothing_is_delivered_before_the_page_is_ready() {
        let mut s = session();
        assert!(s.open(path("a.psd"), HOME).is_empty());
        assert!(s.open(path("b.psd"), HOME).is_empty());
        let started = s.ready(HOME, HOME).unwrap();
        assert_eq!(
            started.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["a.psd", "b.psd"]
        );
        // Once ready, a new file starts at once.
        assert_eq!(s.open(path("c.psd"), HOME).len(), 1);
    }

    #[test]
    fn readiness_needs_both_sources_trusted() {
        let mut s = session();
        s.open(path("a.psd"), HOME);
        assert_eq!(s.ready(AWAY, HOME), Err(Refusal::Untrusted));
        assert_eq!(s.ready(HOME, AWAY), Err(Refusal::Untrusted));
        assert_eq!(s.ready("garbage", HOME), Err(Refusal::Untrusted));
        assert_eq!(s.ready(HOME, HOME).unwrap().len(), 1);
    }

    #[test]
    fn a_read_finished_after_navigation_is_requeued_not_posted() {
        let mut s = session();
        s.open(path("a.psd"), HOME);
        let token = s.ready(HOME, HOME).unwrap()[0].token;
        s.document_changed(NAV, "https://app.test/reload");
        assert_eq!(
            s.delivery_read(token, HOME, Transport::Buffer),
            AfterRead::Retry(vec![])
        );
        // The new document announces itself and gets the file.
        assert_eq!(s.ready(HOME, HOME).unwrap()[0].token, token);
        assert_eq!(
            s.delivery_read(token, HOME, Transport::Buffer),
            AfterRead::Post
        );
    }

    #[test]
    fn a_read_finished_on_an_untrusted_live_source_is_not_posted() {
        let mut s = session();
        s.open(path("a.psd"), HOME);
        let token = s.ready(HOME, HOME).unwrap()[0].token;
        assert_eq!(
            s.delivery_read(token, AWAY, Transport::Buffer),
            AfterRead::Retry(vec![])
        );
    }

    #[test]
    fn leaving_the_trusted_origin_revokes_delivered_tokens_but_keeps_queued_files() {
        let mut s = session();
        let token = delivered(&mut s, "a.psd");
        let navigated = s.document_changed(NAV, AWAY);
        assert_eq!(navigated.revoked, 1);
        // Gate 8: neither the untrusted page nor a later trusted one can use it.
        assert_eq!(
            s.begin_save(AWAY, AWAY, &token.to_string(), 4),
            Err(Refusal::Untrusted)
        );
        assert_eq!(s.ready(AWAY, AWAY), Err(Refusal::Untrusted));
        assert!(s.open(path("b.psd"), AWAY).is_empty());
        s.document_changed(NAV, HOME);
        assert_eq!(
            s.begin_save(HOME, HOME, &token.to_string(), 4),
            Err(Refusal::UnknownToken)
        );
        // The file opened while away was never exposed and is delivered now.
        let started = s.ready(HOME, HOME).unwrap();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].name, "b.psd");
    }

    #[test]
    fn a_trusted_navigation_keeps_tokens_but_drops_uncommitted_saves() {
        let mut s = session();
        let token = delivered(&mut s, "a.psd");
        let t = token.to_string();
        let (_, nonce) = begin(&mut s, &t, 4).unwrap();
        let navigated = s.document_changed(NAV, "https://app.test/other");
        assert_eq!(
            navigated,
            Navigated {
                revoked: 0,
                dropped_saves: vec![token]
            }
        );
        assert_eq!(
            s.commit_save(HOME, HOME, &t, nonce),
            Err(Refusal::NotAwaiting)
        );
        // The token still works for a fresh save.
        assert_eq!(begin(&mut s, &t, 4).unwrap().0, token);
    }

    #[test]
    fn a_late_commit_cannot_fill_in_for_a_later_save_of_the_same_file() {
        let mut s = session();
        let token = delivered(&mut s, "a.psd");
        let t = token.to_string();
        let (_, first) = begin(&mut s, &t, 4).unwrap();
        s.document_changed(NAV, "https://app.test/other");
        let (_, second) = begin(&mut s, &t, 4).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            s.commit_save(HOME, HOME, &t, first),
            Err(Refusal::NotAwaiting)
        );
        assert_eq!(
            s.commit_save(HOME, HOME, &t, second),
            Ok((token, path("a.psd")))
        );
    }

    #[test]
    fn nothing_is_posted_while_a_navigation_to_an_untrusted_origin_is_pending() {
        let mut s = session();
        s.open(path("a.psd"), HOME);
        let token = s.ready(HOME, HOME).unwrap()[0].token;
        // The live source still reads trusted: the new document does not exist yet.
        s.navigation_starting(7, AWAY);
        assert!(!s.may_post(HOME));
        assert_eq!(
            s.delivery_read(token, HOME, Transport::Buffer),
            AfterRead::Retry(vec![])
        );
        assert!(s.open(path("b.psd"), HOME).is_empty());
        // It turned into a download, or was cancelled: the page stays and gets both.
        let started = s.navigation_completed(7, HOME);
        assert_eq!(started.len(), 2);
        // A trusted navigation never holds anything back.
        s.navigation_starting(8, "https://app.test/next");
        assert!(s.may_post(HOME));
    }

    #[test]
    fn only_a_trusted_page_can_name_a_file_it_was_given() {
        let mut s = session();
        s.open(path("queued.md"), HOME);
        let token = delivered(&mut s, "given.md").to_string();
        assert_eq!(s.delivered_name(HOME, HOME, &token), Ok("given.md"));
        assert_eq!(
            s.delivered_name(AWAY, HOME, &token),
            Err(Refusal::Untrusted)
        );
        assert_eq!(
            s.delivered_name(HOME, AWAY, &token),
            Err(Refusal::Untrusted)
        );
        // Not yet delivered, unknown, or spelled differently.
        assert_eq!(
            s.delivered_name(HOME, HOME, "1"),
            Err(Refusal::UnknownToken)
        );
        assert_eq!(
            s.delivered_name(HOME, HOME, "999"),
            Err(Refusal::UnknownToken)
        );
        assert_eq!(
            s.delivered_name(HOME, HOME, &format!("0{token}")),
            Err(Refusal::UnknownToken)
        );
        s.document_changed(NAV, AWAY);
        assert_eq!(
            s.delivered_name(HOME, HOME, &token),
            Err(Refusal::UnknownToken)
        );
    }

    #[test]
    fn a_navigation_that_never_reports_back_stops_holding_after_the_timeout() {
        let mut s = session();
        s.open(path("a.psd"), HOME);
        s.navigation_starting(7, AWAY);
        assert!(s.ready(HOME, HOME).unwrap().is_empty());
        assert!(s.held());
        assert!(s.release_held(HOME).is_empty());
        // No completion and no new document ever arrive for navigation 7.
        s.leaving_timeout = Duration::ZERO;
        assert!(!s.held());
        assert_eq!(s.release_held(HOME).len(), 1);
    }

    #[test]
    fn a_new_document_ends_the_navigations_it_superseded() {
        let mut s = session();
        s.navigation_starting(3, AWAY);
        s.navigation_starting(4, AWAY);
        s.navigation_starting(6, AWAY);
        s.document_changed(5, HOME);
        // 6 started after the document of 5 and is still pending.
        assert!(!s.may_post(HOME));
        s.navigation_completed(6, HOME);
        assert!(s.may_post(HOME));
    }

    #[test]
    fn save_refusals() {
        let mut s = session();
        let token = delivered(&mut s, "a.psd");
        let t = token.to_string();
        assert_eq!(s.begin_save(AWAY, HOME, &t, 4), Err(Refusal::Untrusted));
        assert_eq!(s.begin_save(HOME, AWAY, &t, 4), Err(Refusal::Untrusted));
        for bad in ["99", "x", "", "01", "+1"] {
            assert_eq!(
                s.begin_save(HOME, HOME, bad, 4),
                Err(Refusal::UnknownToken),
                "{bad}"
            );
        }
        assert_eq!(s.begin_save(HOME, HOME, &t, 0), Err(Refusal::Size));
        assert_eq!(
            s.begin_save(HOME, HOME, &t, MAX_FILE_BYTES + 1),
            Err(Refusal::Size)
        );
        assert_eq!(s.commit_save(HOME, HOME, &t, 1), Err(Refusal::NotAwaiting));
        // A queued, not yet delivered, file cannot be saved.
        s.open(path("b.psd"), "https://elsewhere.test/");
        assert_eq!(s.begin_save(HOME, HOME, "2", 4), Err(Refusal::UnknownToken));
    }

    #[test]
    fn one_save_per_token_at_a_time() {
        let mut s = session();
        let token = delivered(&mut s, "a.psd");
        let t = token.to_string();
        let (_, nonce) = begin(&mut s, &t, MAX_FILE_BYTES).unwrap();
        assert_eq!(begin(&mut s, &t, 4), Err(Refusal::InFlight));
        assert_eq!(
            s.commit_save(HOME, HOME, &t, nonce),
            Ok((token, path("a.psd")))
        );
        assert_eq!(begin(&mut s, &t, 4), Err(Refusal::InFlight));
        assert_eq!(
            s.commit_save(HOME, HOME, &t, nonce),
            Err(Refusal::NotAwaiting)
        );
        s.save_ended(token);
        assert!(begin(&mut s, &t, 4).is_ok());
        // A save whose buffer could not be created is abandoned the same way.
        s.save_ended(token);
        assert!(begin(&mut s, &t, 4).is_ok());
    }

    #[test]
    fn a_write_in_progress_survives_navigation_but_its_token_does_not() {
        let mut s = session();
        let token = delivered(&mut s, "a.psd");
        let t = token.to_string();
        let (_, nonce) = begin(&mut s, &t, 4).unwrap();
        s.commit_save(HOME, HOME, &t, nonce).unwrap();
        // The buffer being written is not reported for closing.
        assert_eq!(
            s.document_changed(NAV, AWAY).dropped_saves,
            Vec::<u64>::new()
        );
        s.save_ended(token);
        s.document_changed(NAV, HOME);
        assert_eq!(s.begin_save(HOME, HOME, &t, 4), Err(Refusal::UnknownToken));
    }

    #[test]
    fn a_failed_delivery_forgets_the_file() {
        let mut s = session();
        s.open(path("a.psd"), HOME);
        let token = s.ready(HOME, HOME).unwrap()[0].token;
        s.delivery_failed(token);
        assert_eq!(s.name(token), None);
        s.document_changed(NAV, HOME);
        assert!(s.ready(HOME, HOME).unwrap().is_empty());
    }
}
