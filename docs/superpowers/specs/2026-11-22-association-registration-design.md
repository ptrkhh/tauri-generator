# Association Registration Design (sub-project E)

Date: 2026-11-22. Status: approved by the user 2026-11-24. Plan: [`2026-11-24-association-registration.md`](../plans/2026-11-24-association-registration.md).

## Summary

E makes a generated app appear in Windows **Open with** for its configured extensions, and makes it a handler Windows can pick for double-click. Registration is for the current user only (HKCU), needs no elevation, and never claims a default app: Windows keeps control of default selection, as the base design requires. So a double-click opens the app directly only once the user has made it the default through Windows' own UI: the "How do you want to open this file?" prompt Windows shows for an extension with no default (or after a new handler appears), or **Open with → Choose another app → Always**. Until then a double-click opens Windows' prompt or the existing default (for `.md`, often Notepad).

On first launch an app with associations offers to register them. A declined offer is remembered and never asked again; moving the exe makes the app offer again, because the registration then points at a dead path.

E closes the "appears in Open with" and double-click halves of gates 4 and 5. Everything E adds to the file path after the double-click — arguments, single instance, tokens, tiers — already exists from D and is only reused, not retested, except as a command-string proof (E3).

No schema, pack, or generator change: `fileAssociations` is already in the spec (schema `check_assoc` pins extension shape and uniqueness) and `webwrap-generate` already takes `--assoc`. E is runtime-only.

## Decisions

| Question | Decision | Source |
| --- | --- | --- |
| Scope | HKCU only: `HKCU\Software\Classes\...`, the ProgID plus a value in each extension's `HKCU\Software\Classes\.<ext>\OpenWithProgids`. No elevation, per user, per machine | Base design ("for the current user") |
| Default app | Never write `UserChoice`, `UserPreferencesHash`, or `MRUList`. Only add this app's ProgID value under each extension's `OpenWithProgids` | Base design ("Windows controls default-application selection") |
| Command form | `"<exe path>" "%1"`, one process per file. Explorer's Open with always passes exactly one file; multi-select launches several processes, and D1's single-instance forwarding merges them into the running window (gate 6, proven). This answers D's open item: no multi-file form | D spec open items; D1 |
| Architecture coverage | Each exe registers itself (`GetModuleFileNameW`) under one shared ProgID `webwrap.<app-id>`. One build does not register the other; last registered wins, and the one you run is the one that registered. A moved or regenerated exe re-offers, so the path follows the exe. Accepted risk: someone who runs both architecture builds of one app alternately gets the offer at each switch; one PC runs one build in practice. This answers D's other open item | D spec open items; base design ("moving may require registering again") |
| ProgID | `webwrap.<app-id>`, built from the schema-validated `app_id` and validated extensions (lowercase alnum only), so key and value names cannot contain `\` or quotes | schema `check_assoc`, identity validation |
| What is registered | Exactly the spec's `fileAssociations` extensions. MIME types are not registered with Windows (MIME default mapping is UserChoice territory); they stay what D made them: dialog filters | This design |
| First-launch offer | Offered when the app has associations **and** the registered command is not this exe's path **and** no decline flag exists. Yes registers; No creates `<data_dir>\assoc-declined` (same `create_new` pattern as `open-only-notice`) and is never asked again | Base design ("On first launch, it offers") |
| Decline survives moves | The flag lives in the identity-derived data dir, so one decline silences the offer for this app forever, even after a move. Accepting never creates the flag, so a move re-offers | This design |
| Offer position | Primary instance only, after `single_instance::serve` has started and before the window exists. A forwarded second launch never boxes, and one that arrives while the box is open is accepted and queued in the channel; before `serve` it would wait 5 s for an answer and fail with "the running app did not answer" | This design; D1 (`single_instance.rs` `TIMEOUT`) |
| Manual control | Hidden flags `--register-associations` and `--unregister-associations` (same shape as `--inspect-self`: first argument, then exit). The e2e drives them; F's UI can offer them later. No installer, no uninstaller: keys left by a deleted app point at a missing exe and Explorer hides the entry | This design |

## What is written

All under HKCU, all owned by this app's ProgID name:

```text
HKCU\Software\Classes\webwrap.<app-id>
    (default)                    = "<display name>"
    DefaultIcon                  = "<exe path>",0
    Application\ApplicationName  = "<display name>"
    shell\open\command           = "<exe path>" "%1"

HKCU\Software\Classes\.<ext>\OpenWithProgids
    webwrap.<app-id>             = ""   (REG_SZ, one per association)
```

This is the documented place for an app's Open with entry ("How to include an application in the Open With dialog box"). `HKCU\...\Explorer\FileExts\.<ext>` is Explorer's own per-user state, where `UserChoice` lives, and E writes nothing there. After any change, register or unregister calls `SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, null, null)` so Explorer drops its cached associations without a sign-out.

`registered here` means `shell\open\command` reads back equal to the quoted current exe path plus ` "%1"`, compared case-insensitively (`CompareStringOrdinal`, ignore case), so launching the same exe through a differently-cased path does not re-offer.

The name shown in **Open with** comes from the exe's `FileDescription`, which pack already sets to the display name (`webwrap-pack/src/pe.rs`); the ProgID's `(default)` and `Application\ApplicationName` repeat it for the shell surfaces that read the ProgID instead. Unregistration deletes the ProgID tree, and per extension deletes only the value named `webwrap.<app-id>` — never the `OpenWithProgids` key itself, never a foreign value, never `UserChoice`.

## Phases

| Phase | Content | Gates |
| --- | --- | --- |
| **E1** Registration | `assoc.rs`: register, `registered here` check, unregister, hidden CLI flags | — |
| **E2** First-launch offer | the Yes/No box, the decline flag, `--inspect-self` reports association state | — |
| **E3** Proof | CI e2e for the registry round trip and the command string; PC checks T–V | Open with + double-click halves of 4 and 5 |

### E1: registration (`assoc.rs`)

- `register(spec, exe_path)`: write the tree above, idempotently (values are plain sets, rewriting is fine). On a registry error, return one line naming the key; the caller boxes it but never blocks launch.
- `registered_here(spec, exe_path)`: one `RegGetValueW` of the command string, ordinal case-insensitive comparison, plus one per extension for this ProgID's `OpenWithProgids` value, so an app regenerated with a new extension offers again. Runs at every launch before the window; the cost is one open of a local key.
- `unregister(spec)`: remove exactly the app's own tree and its own-named `OpenWithProgids` values. Uses `RegDeleteTreeW` for the ProgID; `RegDeleteKeyValueW` per extension.
- CLI: in `parse_args`, `--register-associations` and `--unregister-associations` join `--inspect-self` as first-argument-only modes. They load the embedded spec, act, print one line to stdout, and exit. A run with no associations reports that and exits 0.
- Both register and unregister end with `SHChangeNotify(SHCNE_ASSOCCHANGED)`.
- Needs the `Win32_System_Registry` feature of the `windows` crate (`Win32_UI_Shell` is already on).

### E2: first-launch offer

In the primary instance's `run`, right after `single_instance::serve` starts and before the window:

1. no associations → continue;
2. `registered_here` → continue;
3. `<data_dir>\assoc-declined` exists → continue;
4. otherwise `message_box` (the existing `report::message_box`, extended to allow `MB_YESNO`) with the app's display name, the extension list, and the plain promise "This only affects your own account, and does not change any default app." Yes → register (failure boxes the reason, launch continues either way). No → create the flag with `create_new` (a second race loser is fine, same as `open-only-notice`). Log `assoc: registered`, `assoc: declined`, or `assoc: stale` (case 2 failed after a decline) to the `webwrap:` stderr channel.

`--inspect-self` gains an `associations` object: the ProgID, the extensions, and `registeredHere` true/false.

### E3: proof

**CI (`e2e/assoc.mjs`, generated apps, no new apps needed — Photopea carries `.psd`, VS Code carries `.md`):**

1. `--register-associations` on the Photopea app; assert with `reg.exe query` that the ProgID tree matches the exe's path and that `.psd` `OpenWithProgids` lists the ProgID.
2. Read the command back from the registry and launch it with a test `.psd`; assert the delivery log lines the D bridge already emits. This proves the stored command string is exactly right — the only thing double-clicking adds is Explorer running that string.
3. Pre-seed foreign registry values (a dummy ProgID in the same `OpenWithProgids`, a `UserChoice` key), register and unregister, and assert the foreign values are byte-identical afterwards and only this app's keys came and went.
4. `--inspect-self` reports `registeredHere` correctly before and after.

**PC checks (next letters after S, queued in `docs/windows-runs/NEXT.md` when E is implemented):**

- **T** first-launch offer: a fresh app boxes once; **No** means never again; **Yes** registers (verify via the menu in U);
- **U** Explorer shows the app by display name in **Open with** for `.psd` and for `.md` (gate 4/5 menu halves);
- **V** double-click a `.psd`: Windows' own prompt lists the app; pick it with **Always** → the Photopea app opens it and Ctrl+S changes the original; double-click it again → opens directly, no prompt. For `.md`, which likely already has a default, make the VS Code app the default through **Open with → Choose another app → Always**, then double-click → it opens and saves (gate 4/5 double-click halves; the default is set by the human through Windows, never by E). Moving the app's folder re-offers registration.

## Crates and code moved

- `crates/webwrap-runtime/src/assoc.rs` — new, the only real addition.
- `crates/webwrap-runtime/src/main.rs` — two `Mode` arms and the offer call in `run`.
- `crates/webwrap-runtime/src/report.rs` — `message_box` gains a button-set parameter.
- `crates/webwrap-runtime/src/inspect.rs` — the `associations` object.
- Nothing in `webwrap-spec`, `webwrap-pack`, or `webwrap-generate`; no schema bump.

## Verification

- **Wine unit tests** (`assoc.rs`): register → `RegGetValueW` reads the expected command; `registered_here` true; foreign `OpenWithProgids` values and a seeded `UserChoice` survive register and unregister byte-identical; unregister leaves no `webwrap.*` key; `registered_here` false after the exe path changes. Wine has HKCU, so this runs in the existing local Wine leg; CI's `cargo test -p webwrap-runtime` runs the same tests natively on both runners. The tests use a test-only app id, so they never touch a real app's ProgID.
- **Windows CI:** the E3 e2e above on the native runners, both architectures.
- **PC run:** checks T–V. Gates 4 and 5 close on the PC result, not before, per the standing rule that "Open with" and double-click claims wait for E — and now for a human.

## Release gates

E closes the remaining halves of gates 4 and 5 (both the "appears in Open with" and the double-click halves). It closes no other gate. Gates still open after E: 1 (needs F and the phase-2 APK), 2, 9, 10, 12, 13.

## Open items for later sub-projects

- **F.**
  - Read a site manifest's `file_handlers` to propose associations; preselect the Photopea (`.psd`) and `vscode.dev` (`.md`, optional text/code formats) presets. The generator UI needs a way to say "don't offer registration" beyond the per-user decline flag, and may surface the register/unregister flags.
  - Regenerating over a moved-then-registered exe: the new file re-offers; F may register silently when the previous build was registered.
- Regenerating changes `app_id`? No: `app_identity` is a hash of the canonical URL, so regenerating the same site keeps the ProgID, the registration, and the decline flag. F must keep URL canonicalization stable or every change re-offers.
