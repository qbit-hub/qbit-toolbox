# Hotkeys Automated Acceptance

## Design summary

Tier A is a Windows-only Rust integration suite that enters through the existing typed `RawEvent` boundary and exercises the production semantic/native compilers, `PublishedKeymap`, `HotPathState`, bounded `ArrayQueue`, parked `Worker`, fixed-capacity output encoder, and real temporary feature SQLite. Its recorder is bounded and deterministic. The suite never installs a hook, calls `SendInput`, changes foreground focus, or accesses the normal Qbit profile. Existing production event-origin classification, recursion tags, and fail-open behavior remain the authority; the test does not synthesize hardware provenance.

Tier B is a separate Windows integration-test binary behind a non-default Cargo feature and ignored by default. It uses the production `SystemInjector` only after a machine-bound external runner attestation, a dedicated disposable-session identity check, absence of production Qbit processes, and per-batch verification that the test-owned receiver HWND is foreground and belongs to this process. It measures OS output delivery only. This Codex run will compile but never execute it.

Tier C is a documented future USB HID fixture protocol. No physical rig is configured, so it will remain NOT RUN and cannot establish hardware-to-hook acceptance.

## Evidence tiers

| Tier                   | Method                                                                                                         | Proves                                                                                                                          | Does not prove                                                             | Normal CI                                                                  |
| ---------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| A — in-process         | Typed physical `RawEvent` fixtures, production map/worker/output code, bounded fake injector, temporary SQLite | Core remapping decisions, queue and held-key behavior, exact encoded output, worker accounting, service persistence/publication | Windows hook ingress, hardware provenance, OS delivery, real `SendInput`   | Included in Windows `cargo test --workspace`                               |
| B — isolated OS output | Separate ignored test binary, production `SystemInjector`, owned receiver window and attested disposable VM    | Guarded delivery of the benign F10 output sequence to the receiver window                                                       | Physical source-key input, low-level hook remapping, HID/device provenance | Excluded unless feature is enabled and ignored test is explicitly selected |
| C — physical HID       | External USB HID fixture protocol                                                                              | Full physical-device-to-hook-to-output chain when actually run with a verified fixture                                          | Nothing until a real rig produces a signed/traceable run record            | Not available                                                              |

## Tier A scenarios

The acceptance tests use fixed, non-sensitive fixtures only. `Ctrl+Shift+F9 → Ctrl+Shift+F10` covers left, right, and both physical modifier sides; modifier absence; down/repeat/up consumption; one queued action per press; exact output key transitions, Qbit tag and counters; and modifier release/restoration. Additional cases cover `Disable`, unmatched input, both injected origins, live disable/removal while a source key is held, physical-over-logical priority, queue overflow and fail-open retry behavior, execution-time modifier changes, full/short sends, worker cancellation, and transactional service publication/reload against real temporary SQLite.

The evidence is a core-pipeline simulation at the typed OS-decoded event boundary. A standard Cargo test result is the per-scenario machine-readable runner output; no additional report dependency or raw keystroke recorder is introduced.

## Tier B isolation contract

Tier B is not a local desktop shortcut test. It must run only in a disposable Windows VM/session with no interactive user activity and no running `qbit-desktop.exe`. A separate administrator-controlled provisioning step supplies a short-lived attestation marker at the fixed path `ProgramData\Qbit\HotkeysAcceptance\runner-attestation.json`. The marker binds the configured runner ID, SHA-256 of the Windows `MachineGuid`, current user SID, session ID, medium-integrity token, expiry, and an explicitly isolated profile root below `ProgramData\Qbit\HotkeysAcceptance\profiles\qbit-hotkeys-isolated-*`. The test checks the marker and containing directories are not writable by its current token, rejects reparse points, and compares the current machine/user/session/token values. The test requires both the attestation and an explicit opt-in variable; the variables alone never establish authorization.

Before each output batch, the guarded injector checks that the foreground HWND equals the receiver HWND and that `GetWindowThreadProcessId` identifies the current test process. It fails closed on missing/expired/mismatched attestation, wrong session or integrity, locked/disconnected desktop, a production Qbit process, uncertain focus, or any unobserved key transition. Cleanup releases only keys while the verified receiver still owns foreground; if focus is lost, cleanup reports uncertainty and sends no keys to the new foreground application. No unrelated process is terminated.

Opt-in command (Windows disposable VM only; do not run from routine CI or a developer desktop):

```powershell
$env:QBIT_HOTKEYS_ISOLATED_OUTPUT = '1'
$env:QBIT_HOTKEYS_ISOLATED_ATTESTATION = 'C:\ProgramData\Qbit\HotkeysAcceptance\runner-attestation.json'
$env:QBIT_HOTKEYS_ISOLATED_RUNNER_ID = 'qbit-hotkeys-disposable-<provisioned-runner-id>'
cargo +1.97.1 test -p qbit-hotkeys --features isolated-output-acceptance --test acceptance_isolated_output -- --ignored --exact guarded_f10_output_reaches_owned_receiver
```

The marker JSON fields are `schemaVersion: 1`, `runnerId`, `machineGuidSha256`, `userSid`, `sessionId`, `integrityRid: 8192`, `expiresUnixSeconds`, and `isolatedProfileRoot`. Provisioning must create the marker only on a disposable runner, bind it to that exact machine/user/session, set an expiry no more than four hours away, and designate a dedicated non-production profile. Before reading the marker, the preflight opens and holds `ProgramData`, `Qbit`, `HotkeysAcceptance`, and the marker itself without write/delete sharing; it checks each handle's object type, reparse status, owner, group, and DACL, then reads marker bytes from the same handle whose ACL was checked. Any missing/null DACL, missing owner/group, API error, sharing conflict, or writable dangerous right fails closed. The marker check rejects `FILE_WRITE_DATA`, `FILE_APPEND_DATA`, `FILE_WRITE_EA`, `FILE_WRITE_ATTRIBUTES`, `DELETE`, `WRITE_DAC`, and `WRITE_OWNER`. Each existing ancestor is checked for `FILE_WRITE_EA`, `FILE_WRITE_ATTRIBUTES`, `FILE_DELETE_CHILD`, `DELETE`, `WRITE_DAC`, and `WRITE_OWNER`. `FILE_ADD_FILE` or `FILE_ADD_SUBDIRECTORY` alone may create an unrelated sibling; it cannot replace the already-open protected child, while delete/rebind rights remain denied. Generic access ACEs are mapped to file rights before checks, and each dangerous right is checked independently against the current process token. The test also requires a non-elevated medium-integrity token, an unlocked `Default` input desktop, no production Qbit process, and the receiver HWND/PID to be the current process. If no trustworthy marker and protected provisioning are available, Tier B is BLOCKED and must not be invoked.

## Tier C physical HID protocol

An eventual rig must be a uniquely identified USB HID keyboard fixture with documented firmware/build hash, physical device identity, source key frames, output receiver, and emergency release/reset behavior. The fixture sends a fixed left-Ctrl down, left-Shift down, F9 down/up, Shift up, Ctrl up sequence. The receiver records the generated F10 down/up sequence with timestamps and process/window identity. The Windows hook diagnostic must independently record only aggregate counts and the observed physical-origin classification; it must not log user keystrokes or spoof injected flags. A run is PASS only when the configured VID/PID/serial matches, the expected device is the sole fixture, the test receiver has verified focus, every source key is released, exactly one mapping action is observed, and output arrives in order. Disconnect, focus change, timeout, extra input, or uncertain release is FAIL/INCONCLUSIVE and requires an emergency physical reset. Until that hardware exists, report `NO RIG CONFIGURED / NOT RUN`.

The future fixture report should contain `schemaVersion`, `tier`, `runId`, `outcome`, `fixtureId`, VID/PID/serial, firmware hash, Windows build, receiver PID/HWND, fixed source-frame counts, physical-origin and consumed counts, emitted F10 down/up counts, start/end timestamps, cleanup/release status, disconnect/focus/timeout flags, and an error category. It must not contain arbitrary user input, text, window titles, or raw keyboard history.

## Interpretation and remaining coverage

The maintained [desktop lifecycle coverage matrix and CI protocol](hotkeys-desktop-lifecycle.md) extends the existing WebdriverIO journeys with real UI/IPC/SQLite assertions and PID-verified graceful Quit/restart. GUI execution requires the explicitly selected disposable GitHub-hosted Windows desktop; a normal workstation must report `BLOCKED_E2E_ENVIRONMENT`.

- Tier A is the mandatory regression gate and is selected by ordinary Windows `cargo test --workspace`.
- Tier B is OS-output-only and remains ignored in ordinary CI, `bun run check`, and desktop E2E.
- To compile the guarded binary without running it, use `cargo +1.97.1 test -p qbit-hotkeys --features isolated-output-acceptance --test acceptance_isolated_output --no-run`. It is also safe to compile this target while a normal Qbit process is running; it does not build the product release executable or send input.
- Tier C is the only tier capable of proving the complete physical HID-to-hook chain.
- Existing WebdriverIO journeys exercise UI/IPC/database behavior in the isolated `desktop-e2e` app. They do not prove global keyboard output and must not be run locally while a normal production Qbit process is active.
- Real hook ingress, hardware provenance, UIPI/elevation behavior, sleep/resume recovery, Windows silently removing a timed-out hook, and idle CPU/RAM behavior remain separate review or manual acceptance items.

## Windows API references

- [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)
- [KBDLLHOOKSTRUCT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct)
- [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [GetMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessagew)
- [Keyboard input messages](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input)
- [SHGetKnownFolderPath](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath) (COM initialization and CoTaskMemFree ownership)
- [AccessCheck](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-accesscheck) (protected marker ACL)
- [MapGenericMask](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-mapgenericmask) (generic access ACE mapping)
- [WTSQuerySessionInformationW](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsquerysessioninformationw) and [WTS connection state](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ne-wtsapi32-wts_connectstate_class)
- [OpenInputDesktop](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openinputdesktop)
- [GetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getforegroundwindow), [GetWindowThreadProcessId](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid), and [SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow)
