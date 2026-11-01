# Hotkeys v0.1 candidate

Windows-only, feature-owned native service and UI. Supported actions are global Shortcut → Shortcut and Shortcut → Disable. Each mapping and the feature can be enabled independently. Macros, text, scripts, app launching, per-app scope, elevated helpers and non-Windows backends are outside this slice.

Hotkeys and Start with Windows default off. Normal launch stays in the tray; `--show-control` opens settings. Closing settings destroys the WebView while the native feature continues. Quit waits for configuration and native cleanup. Core recovery prevents feature access and startup.

The host registry provides `/features/hotkeys`; the static route composes `@qbit/hotkeys-ui`. Frontend calls go exclusively through `@qbit/ipc`. Rust owns DTOs, validation, conflict analysis, semantic/native compilation and safe errors. Feature mappings live in `data/features/qbit.hotkeys/data.db`; desired enablement and app startup settings live in `data/core.db`. Disabled settings operations open a transient feature connection only.

Configuration mutations serialize on the shared bounded persistence lane, compile the entire candidate before its database commit, and publish an already compiled immutable map afterwards. Failed commits do not alter the live map. A crash after commit recovers by compiling persisted truth on restart. At most 512 mappings are accepted by the service. Duplicate triggers, self-maps, cycles, modifier alias collisions, reserved Ctrl+Alt+Delete and unsupported translations are rejected by Rust.

The hook callback handles down/up/system down/up only, updates physical sided modifiers, and does constant-key native map lookups (physical priority). Both Qbit-tagged and third-party injected events bypass remapping and physical state. Qbit's tag identifies recursion, never authorization. One preallocated queue holds 64 requests; overflow fails open without retrying repeats. The worker parks without polling and uses execution-time physical modifier state. All generated events share the recursion tag; a partial SendInput is counted as failure without a blind retry. There is no logging of shortcuts, input text or target apps.

Two feature-owned threads and one hook exist only while enabled. There are no feature timers, networking or persistent feature DB connection while disabled. The shared configuration lane remains application-owned (32 queued jobs). Thread stop acknowledgements are bounded to three seconds each; each host lane close waits at most ten seconds. Quit immediately closes command admission, drains accepted work in FIFO order and exits only after cleanup and connection release are acknowledged. A failure leaves the service and native owners alive and changes the tray action to **Retry Quit (cleanup incomplete)**. Click it to retry deliberately. Configuration remains closed; another Quit cannot overlap cleanup. A timeout is an unknown/incomplete result, not proof a thread stopped: later Quit waits for the same pending attempt before any new cleanup. A failed stop signal or unhook is reported and remains retryable; a late successful acknowledgement is consumed once. The app does not poll, force exit, or retry endlessly in the background. No hook/thread/DB operation executes in the callback. `hook_active` is last acknowledged installation state; Windows can silently remove low-level hooks after timeout, which this app cannot independently detect at idle. Re-enable is an explicit recovery action before shutdown begins.

Windows UIPI may reject output to higher-integrity apps. The app never requests permanent elevation. Ctrl+Alt+Delete is unsupported as trigger or target. Capture is focused UI-only, layout-aware for Latin letters, with generic Ctrl/Shift/Alt/Meta. Dead/composing/AltGraph and ambiguous key identities are rejected. Supported punctuation requires a matching known code/character pair; numpad digits require Num Lock character semantics. Advanced physical/sided capture has no UI in v0.1.

Activation requires all physical modifier keys released. If any Ctrl/Shift/Alt/Windows key is held while enabling or restoring persisted startup intent, startup fails safely with a release-and-retry message rather than guessing an initial modifier state.

## Automated validation

```powershell
cargo +1.97.1 test -p qbit-hotkeys
cargo +1.97.1 clippy -p qbit-hotkeys --all-targets -- -D warnings
cargo +1.97.1 fmt --all -- --check
bun run ipc:generate
bun run ipc:check
bun run check
bun audit --audit-level=high
cargo +1.97.1 audit
bun run tauri:build
```

Tests use temp SQLite, fake injectors and fake OS startup registration. Property/exhaustive planning covers 256 physical states × 81 target models. Ordinary suites never install a real hook or issue SendInput. Component coverage includes focused capture, review-before-save, safe errors, distinct toggles and axe checks; jsdom color contrast is deferred to the real renderer/manual checklist.

The CI-safe Tier A acceptance binary (`tests/acceptance_pipeline.rs`) exercises the real typed event boundary, native compilation/publication, hot path, bounded action queue, output worker/encoder, and bounded fake recorder. Windows `cargo test --workspace` selects it without extra flags. It includes the left-modifier `Ctrl+Shift+F9 → Ctrl+Shift+F10` fixture, all left/right/both/absent modifier-side combinations, disable/unmatched/injected paths, map replacement with a held source key, overflow fail-open, worker execution-time modifier races, short-send/no-retry, cancellation, priority/restoration edges, and service persistence/publication/reload/rollback against real temp SQLite. These results prove the in-process core pipeline only; they do not prove hardware provenance, hook ingress or OS output.

The separate Tier B `acceptance_isolated_output` test is ignored and requires the non-default `isolated-output-acceptance` feature plus explicit `--ignored` selection. It uses the production `SystemInjector` and a test-owned receiver window after validating an administrator-provisioned, machine/user/session-bound marker, protected marker ACL, non-elevated medium-integrity session, unlocked input desktop, and absence of `qbit-desktop.exe`. It checks the receiver HWND/PID before each batch and releases F10 only while that receiver still owns foreground. Its evidence is OS-output-only. Compile without running via:

```powershell
cargo +1.97.1 test -p qbit-hotkeys --features isolated-output-acceptance --test acceptance_isolated_output --no-run
```

Run only on an explicitly authorized disposable Windows VM using the exact marker, runner ID and command in [Hotkeys Automated Acceptance](hotkeys-acceptance.md). Never run it on the developer desktop or routine CI. Tier C remains a future USB HID protocol; report `NO RIG CONFIGURED / NOT RUN` until a verified physical fixture exists. Real hook ingress, hardware provenance, UIPI, sleep/resume, Windows silent hook removal and idle resource behavior remain uncovered by automated acceptance.

Desktop E2E uses WebdriverIO 9, `@wdio/tauri-service` 1.5.1 and tauri-driver 2.1.0. Run in an isolated Windows interactive test session with WebView2 and network access for the matching Microsoft Edge driver. No user data or startup registration is changed. Test shortcuts are sent only to the test application's focused capture control; the saved action is Disable, never OS output injection.

Use Node 22.12 or newer for this test tooling. The pinned `@puppeteer/browsers` 3.2.6 override removes the vulnerable archive/proxy dependency path; its exported functions used by WebdriverIO were checked against installed source. `serialize-javascript` 7.1.4 preserves Mocha's CommonJS serialization API while removing a high-severity advisory. A Mocha 12 override was rejected because WebdriverIO requires Mocha's older helper-module path; Mocha remains within the framework's declared range. The complete E2E run validates this selected dependency combination.

Edge's external WebDriver ends its session/process job when the final WebView closes. The native close/reopen E2E therefore ends that session, launches the same isolated binary directly, sends WM_CLOSE to its settings window, verifies the old HWND is destroyed while the same process remains alive, activates it through a second instance, and verifies graceful Quit. Tauri's single-instance helper window is distinguished from the settings WebView. A narrowly compiled `--e2e-quit` second-instance intent exists only in the E2E binary; no scripting/driver permission or local server is added to production.

```powershell
cargo +1.97.1 install tauri-driver --version 2.1.0 --locked --root .ai-bridge/e2e-tools
bun run e2e:build
bun run e2e:desktop
```

The E2E feature fails closed without `QBIT_E2E_DATA_ROOT` under an existing `qbit-hotkeys-e2e-*` temp directory. The launcher creates it and retains it for diagnosis; the separately identified binary is under `target/e2e`. Production binaries have no environment-based data override.

Optional real hook registration test, only after explicit authorization on an isolated desktop:

```powershell
$env:QBIT_INTERACTIVE_HOTKEYS_TEST = '1'
cargo +1.97.1 test -p qbit-hotkeys --test windows_engine real_hook_lifecycle_opt_in -- --ignored --exact
```

## Human acceptance checklist (not performed by automated gates)

- Fresh tray/headless launch: disabled feature, no hook/output threads/feature DB connection. Open, minimize/restore, close/reopen settings; verify single-instance activation.
- Enable, verify Running and canonical feature database. Use a harmless remap in an ordinary non-elevated app; verify source suppression, output, repeats, key-up and no stuck Ctrl/Shift/Alt/Win.
- Disable mapping suppresses down/repeat/up; disable row restores source. Edit/save hot-reloads, deletion takes effect immediately, held consumed key-up remains suppressed.
- Close settings while enabled and verify native behavior; reopen and restart to verify persistence. Disable feature and inspect resource cleanup. Quit should exit only after cleanup; if it reports incomplete cleanup, configuration stays closed and the tray offers a deliberate Retry Quit. Deterministic failure/timeout recovery is covered by fake regression tests; do not manufacture OS hook failures in a normal desktop session.
- Start with Windows: enable/remove registration, verify saved/actual consistency and optional next-logon headless startup. Keep this distinct from Hotkeys startup.
- Test keyboard-only navigation, capture cancel/focus, conflict/error announcements, screen reader, light/dark/system themes, real-renderer contrast and CSP style insertion. Verify UIPI limitation messaging.
- Sleep/resume, lock/unlock and extended input/idle soak; recover by explicit disable/re-enable if Windows removes the hook. Check modifiers after transitions.
- On documented reference hardware, record OS/build/hardware, enabled mappings, process ID, UI closed/open state and sampling tools. Measure CPU over an idle window, working/private memory, thread/handle counts, wakeups, enable/disable and shortcut latency; compare repeated start/stop cycles for growth. No numerical budget is claimed before a measured baseline.

Review, complete local automated revalidation, explicit human local acceptance, Stage validation on exact candidate artifacts, versioning/signing and release are separate later gates. No automated result establishes human validation or release approval.

## Upstream API evidence

- [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)
- [SendInput and UIPI](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [Tauri autostart](https://v2.tauri.app/plugin/autostart/)
- [Tauri desktop WebDriver](https://v2.tauri.app/develop/tests/webdriver/)

Windows FFI was checked against installed `windows` 0.61.3 source, not newer master examples. Autostart 2.5.1 and its compatible build helper preserve the existing Tauri/windows graph; E2E dependencies are development-only.
