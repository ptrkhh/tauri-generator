# Association registration plan (sub-project E)

Spec: [`../specs/2026-11-22-association-registration-design.md`](../specs/2026-11-22-association-registration-design.md). Branch `feat/association-registration`. Runtime-only: no schema, pack, or generator change.

## Tasks

1. **E1 `assoc.rs`.** `Registration { prog_id, display_name, extensions, exe }` with `register`, `unregister`, `registered_here`. It writes REG_SZ values with `RegSetKeyValueW`, which creates keys as needed, and deletes with `RegDeleteTreeW` and `RegDeleteKeyValueW`, where a missing key or value is fine. It compares with `CompareStringOrdinal` (ignore case), and calls `SHChangeNotify(SHCNE_ASSOCCHANGED)` after every change. New `windows` features: `Win32_System_Registry` and `Win32_Globalization`.
   - Unit tests, run under Wine and natively in CI, use a test-only id and extension:
     - register writes the exact command, and a differently-cased path still counts as registered here, but a moved exe does not;
     - unregister removes only this app's keys, while a foreign `OpenWithProgids` value and a seeded `UserChoice` survive;
     - a missing extension value means not registered here.
2. **E1 flags.** `parse_args` accepts `--register-associations` and `--unregister-associations` as the only argument. Each prints one line and exits 0, or prints the error to stderr and exits 1.
3. **E2 offer.** `assoc::offer` runs in `run` right after `single_instance::serve` and before the Tauri builder, for every launch that opens a window.
   - `report::ask` is a `MB_YESNO | MB_ICONQUESTION` box.
   - Yes registers; a failure shows a box without blocking the launch.
   - No creates `<data_dir>\assoc-declined`.
   - Log lines: `assoc: registered`, `assoc: declined`, `assoc: stale`, `assoc: failed`.
4. **E2 inspect.** `--inspect-self` gains `associations: { progId, extensions, registeredHere }`.
5. **E3 CI.**
   - The registry round trip lives in `e2e/photopea.mjs`, the run that already serves the stand-in Photopea for Bridge D, the e2e app with `.psd`. It seeds a foreign `OpenWithProgids` value and a `UserChoice`, then:
     - registers and checks `registeredHere`;
     - checks that the command is exactly `"<exe>" "%1"`;
     - runs that command with a file and sees it delivered;
     - unregisters, and checks that only this app's keys went.
   - Because the offer's box would hold back the window of any app that has associations:
     - `e2e/apps.mjs` `launch` declines the offer up front;
     - the workflow's smoke launch does the same.
6. **E3 PC.** Queue checks T-V in `docs/windows-runs/NEXT.md`, per the spec.
