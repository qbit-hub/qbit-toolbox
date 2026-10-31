#![cfg(windows)]

//! Explicitly selected OS-output-only acceptance. This binary is behind a
//! non-default feature and the test is ignored unless separately requested.

use crossbeam_queue::ArrayQueue;
use qbit_hotkeys::{
    domain::{
        Action, Chord, KeyIdentity, KeyToken, Mapping, MappingBehavior, MappingMetadata, Scope,
        Trigger,
    },
    platform::windows::{
        IsolatedSystemInjector,
        compiled::{self, PublishedKeymap},
        engine::{Shared, Worker},
        hot_path::{
            Decision, HotPathState, InputOrigin, KeyEventKind, QBIT_INJECTION_TAG, RawEvent,
        },
        output::{EncodedInput, Injector},
    },
    runtime::{Metrics, MetricsSnapshot},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    mem::size_of,
    path::{Component, Path, PathBuf},
    ptr::null_mut,
    slice,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_NO_MORE_FILES, GetLastError, HANDLE, HLOCAL, WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        Security::Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
        },
        Security::{
            ACE_HEADER, ACL_SIZE_INFORMATION, AccessCheck, AclSizeInformation,
            DACL_SECURITY_INFORMATION, DuplicateToken, GENERIC_MAPPING, GROUP_SECURITY_INFORMATION,
            GetAce, GetAclInformation, GetSecurityDescriptorDacl, GetSecurityDescriptorGroup,
            GetSecurityDescriptorOwner, GetSidSubAuthority, GetSidSubAuthorityCount,
            GetTokenInformation, IsValidSecurityDescriptor, MapGenericMask,
            OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, PSECURITY_DESCRIPTOR, PSID,
            SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_MANDATORY_LABEL,
            TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenIntegrityLevel, TokenUser,
        },
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_ALL_ACCESS, FILE_APPEND_DATA,
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DELETE_CHILD,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_EXECUTE,
            FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
            FILE_SHARE_READ, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA,
            GetFileInformationByHandle, GetFileSizeEx, OPEN_EXISTING, READ_CONTROL, ReadFile,
            WRITE_DAC, WRITE_OWNER,
        },
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            LibraryLoader::GetModuleHandleW,
            Registry::{
                HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, REG_VALUE_TYPE, RegCloseKey,
                RegOpenKeyExW, RegQueryValueExW,
            },
            RemoteDesktop::ProcessIdToSessionId,
            StationsAndDesktops::{
                CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS,
                GetUserObjectInformationW, OpenInputDesktop, UOI_NAME,
            },
            Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken},
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
            GetWindowThreadProcessId, IsWindow, MSG, MsgWaitForMultipleObjects, PM_REMOVE,
            PeekMessageW, QS_ALLINPUT, RegisterClassW, SW_SHOWNORMAL, SetForegroundWindow,
            ShowWindow, TranslateMessage, UnregisterClassW, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
            WM_SYSKEYUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
    core::{GUID, PCWSTR, PWSTR, w},
};

const ATTESTATION_ENV: &str = "QBIT_HOTKEYS_ISOLATED_ATTESTATION";
const OPT_IN_ENV: &str = "QBIT_HOTKEYS_ISOLATED_OUTPUT";
const RUNNER_ID_ENV: &str = "QBIT_HOTKEYS_ISOLATED_RUNNER_ID";
const F10_VK: u16 = 0x79;
const MAX_ATTESTATION_BYTES: u64 = 8 * 1024;
const ATTESTATION_MAX_AGE: Duration = Duration::from_secs(4 * 60 * 60);
const RECEIVER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunnerAttestation {
    schema_version: u32,
    runner_id: String,
    machine_guid_sha256: String,
    user_sid: String,
    session_id: u32,
    integrity_rid: u32,
    expires_unix_seconds: u64,
    isolated_profile_root: PathBuf,
}

struct Identity {
    machine_guid_sha256: String,
    user_sid: String,
    session_id: u32,
    integrity_rid: u32,
    elevated: bool,
}

#[test]
#[ignore = "requires a disposable, administrator-provisioned Windows VM and protected runner attestation"]
fn guarded_f10_output_reaches_owned_receiver() {
    let _com = ComApartment::initialize().expect("known-folder preflight requires COM STA");
    let attestation = load_and_validate_attestation().expect("isolated-runner preflight must pass");
    assert_no_production_qbit_process().expect("no production Qbit process may be running");
    assert_default_input_desktop().expect("interactive unlocked test desktop is required");

    let receiver = Arc::new(
        ReceiverWindow::create(attestation.session_id).expect("owned receiver window starts"),
    );
    receiver
        .verify_foreground()
        .expect("owned receiver must be foreground before input is authorized");

    let injector = Arc::new(GuardedSystemInjector::new(receiver.clone()));
    let map = Arc::new(PublishedKeymap::default());
    map.publish(Arc::new(
        compiled::compile(&[output_only_fixture()]).unwrap(),
    ));
    let metrics = Arc::new(Metrics::default());
    let shared = Arc::new(Shared::new(map, metrics.clone()));
    let mut worker = WorkerOwner::start(shared.clone(), injector.clone());

    let mut state = HotPathState::default();
    let decision = state.process(
        RawEvent {
            kind: KeyEventKind::Down,
            vk_code: 0x78,
            scan_code: 0x43,
            extended: false,
            origin: InputOrigin::Physical,
        },
        &shared.map.load(),
        &shared.queue,
        &shared.metrics,
    );
    assert_eq!(
        decision,
        Decision::Consume,
        "fixture input is an in-process core event, not hardware"
    );
    worker.thread().unpark();

    let observation = receiver.observe_f10();
    shared.cancelled.store(true, Ordering::Release);
    worker.thread().unpark();
    let stop_result = worker.stop();
    let release_result = injector.release_if_needed();
    let guarded_error = injector
        .error
        .lock()
        .expect("injector error lock is healthy")
        .clone();
    let send_count = injector.send_count.load(Ordering::SeqCst);
    let final_metrics = metrics.snapshot();

    assert!(
        stop_result.is_ok(),
        "worker cleanup acknowledgement: {stop_result:?}"
    );
    assert!(
        release_result.is_ok(),
        "synthetic key cleanup: {release_result:?}"
    );
    assert!(
        guarded_error.is_none(),
        "foreground/attestation guard failed: {guarded_error:?}"
    );
    assert_eq!(
        send_count, 1,
        "a single output batch is expected; no retry is permitted"
    );
    let observation = observation.expect("receiver observes bounded F10 key-down/up");
    assert!([WM_KEYDOWN, WM_SYSKEYDOWN].contains(&observation[0]));
    assert!([WM_KEYUP, WM_SYSKEYUP].contains(&observation[1]));
    assert_eq!(
        final_metrics,
        MetricsSnapshot {
            mapping_count: 0,
            events_seen: 1,
            events_matched: 1,
            queue_overflows: 0,
            actions_emitted: 1,
            injection_failures: 0,
            hook_active: false,
        }
    );

    let _ = attestation.runner_id;
}

fn output_only_fixture() -> Mapping {
    Mapping::with_id(
        uuid::Uuid::from_u128(0x484f_545f_4f53_0000_0000_0000_0000_0001),
        true,
        Trigger::Chord {
            chord: Chord::new([], KeyIdentity::Logical(KeyToken::new("F9").unwrap())).unwrap(),
        },
        Action::EmitShortcut {
            chord: Chord::new([], KeyIdentity::Logical(KeyToken::new("F10").unwrap())).unwrap(),
        },
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    )
}

fn load_and_validate_attestation() -> Result<RunnerAttestation, String> {
    if std::env::var(OPT_IN_ENV).as_deref() != Ok("1") {
        return Err(format!("{OPT_IN_ENV}=1 is required"));
    }
    let program_data = known_folder_path(&windows::Win32::UI::Shell::FOLDERID_ProgramData)?;
    let expected_path = program_data
        .join("Qbit")
        .join("HotkeysAcceptance")
        .join("runner-attestation.json");
    let supplied_path = std::env::var_os(ATTESTATION_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{ATTESTATION_ENV} is required"))?;
    if normalized_path(&supplied_path) != normalized_path(&expected_path) {
        return Err(format!(
            "attestation path must be the provisioned ProgramData marker: {}",
            expected_path.display()
        ));
    }
    let runner_id =
        std::env::var(RUNNER_ID_ENV).map_err(|_| format!("{RUNNER_ID_ENV} is required"))?;
    if !runner_id.starts_with("qbit-hotkeys-disposable-") {
        return Err("runner ID must identify a provisioned disposable VM".to_owned());
    }

    let identity = current_identity()?;
    if identity.elevated || identity.integrity_rid != 0x2000 {
        return Err("runner must use a non-elevated medium-integrity token".to_owned());
    }
    if identity.session_id == 0 {
        return Err("session 0 is not an interactive test session".to_owned());
    }

    let qbit_directory = program_data.join("Qbit");
    let acceptance_directory = qbit_directory.join("HotkeysAcceptance");
    let protected_paths = [
        (
            program_data.as_path(),
            ProtectedPathKind::DirectoryWithExistingProtectedChild,
        ),
        (
            qbit_directory.as_path(),
            ProtectedPathKind::DirectoryWithExistingProtectedChild,
        ),
        (
            acceptance_directory.as_path(),
            ProtectedPathKind::DirectoryWithExistingProtectedChild,
        ),
        (expected_path.as_path(), ProtectedPathKind::MarkerFile),
    ];
    // Keep each opened directory and the marker alive while checking all ACLs
    // and reading the marker. FILE_SHARE_READ denies new write/delete opens;
    // opening each next component only after holding its parent also prevents
    // pathname replacement between ACL verification and marker read.
    let mut protected_handles = Vec::with_capacity(protected_paths.len());
    for (protected_path, kind) in protected_paths {
        let handle = open_protected_path(protected_path, kind)?;
        let descriptor = security_descriptor_for_handle(handle.0, protected_path)?;
        let access_result = ensure_caller_cannot_mutate_descriptor(
            descriptor,
            kind,
            &protected_path.display().to_string(),
        );
        // SAFETY: GetSecurityInfo allocated this descriptor for LocalFree.
        unsafe { windows::Win32::Foundation::LocalFree(Some(HLOCAL(descriptor.0))) };
        access_result?;
        protected_handles.push(handle);
    }
    let marker_handle = protected_handles
        .last()
        .ok_or("attestation marker handle missing")?
        .0;
    let bytes = read_bounded_marker(marker_handle)?;
    let attestation: RunnerAttestation = serde_json::from_slice(&bytes)
        .map_err(|error| format!("attestation JSON is invalid: {error}"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock predates Unix epoch: {error}"))?
        .as_secs();
    if attestation.schema_version != 1
        || attestation.runner_id != runner_id
        || attestation.machine_guid_sha256 != identity.machine_guid_sha256
        || attestation.user_sid != identity.user_sid
        || attestation.session_id != identity.session_id
        || attestation.integrity_rid != identity.integrity_rid
        || attestation.expires_unix_seconds <= now
        || attestation.expires_unix_seconds.saturating_sub(now) > ATTESTATION_MAX_AGE.as_secs()
    {
        return Err(
            "attestation is expired or does not match this machine/user/session/token".to_owned(),
        );
    }

    let profiles_root = program_data
        .join("Qbit")
        .join("HotkeysAcceptance")
        .join("profiles");
    validate_isolated_profile_root(&attestation.isolated_profile_root, &profiles_root)?;
    assert_no_production_qbit_process()?;
    assert_default_input_desktop()?;
    Ok(attestation)
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn validate_isolated_profile_root(path: &Path, profiles_root: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| component == Component::ParentDir)
        || !normalized_path(path).starts_with(&format!("{}\\", normalized_path(profiles_root)))
    {
        return Err("attested profile root must be a dedicated path under ProgramData\\Qbit\\HotkeysAcceptance\\profiles".to_owned());
    }
    let leaf = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "attested profile root has no valid fixture name".to_owned())?;
    if !leaf.starts_with("qbit-hotkeys-isolated-") {
        return Err(
            "attested profile leaf must use the isolated Hotkeys fixture prefix".to_owned(),
        );
    }
    let app_data = known_folder_path(&windows::Win32::UI::Shell::FOLDERID_RoamingAppData)?
        .join("click.qbit.toolbox");
    if normalized_path(path) == normalized_path(&app_data)
        || normalized_path(path).starts_with(&normalized_path(&app_data))
        || normalized_path(&app_data).starts_with(&normalized_path(path))
    {
        return Err("isolated runner profile overlaps the production Qbit profile".to_owned());
    }
    Ok(())
}

fn known_folder_path(folder: &GUID) -> Result<PathBuf, String> {
    // SAFETY: known-folder identifiers are static Windows constants; the API
    // allocates the returned string with COM task memory.
    let path = unsafe {
        windows::Win32::UI::Shell::SHGetKnownFolderPath(
            folder,
            windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG(0),
            None,
        )
    }
    .map_err(|error| format!("resolve Windows known folder: {error}"))?;
    let text = pwstr_to_string(path);
    // SAFETY: SHGetKnownFolderPath allocated this string with CoTaskMemAlloc.
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(path.0.cast())) };
    if text.is_empty() {
        return Err("Windows returned an empty known-folder path".to_owned());
    }
    Ok(PathBuf::from(text))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProtectedPathKind {
    DirectoryWithExistingProtectedChild,
    MarkerFile,
}

fn open_protected_path(path: &Path, kind: ProtectedPathKind) -> Result<OwnedHandle, String> {
    let wide = wide_path(path);
    let mut desired_access = READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0;
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    if kind == ProtectedPathKind::DirectoryWithExistingProtectedChild {
        flags |= FILE_FLAG_BACKUP_SEMANTICS;
    } else {
        desired_access |= FILE_READ_DATA.0;
    }
    // Share reads only. Existing writers/deleters cause a safe failure, and
    // new write/delete handles cannot race ACL validation or marker reading.
    // SAFETY: `wide` is nul-terminated and remains alive for the API call.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            desired_access,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )
    }
    .map_err(|error| {
        format!(
            "cannot open protected attestation path {}: {error}",
            path.display()
        )
    })?;
    let handle = OwnedHandle(handle);

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is live and Windows writes the fixed-size result struct.
    unsafe { GetFileInformationByHandle(handle.0, &mut information) }
        .map_err(|error| format!("cannot inspect protected path {}: {error}", path.display()))?;
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(format!(
            "reparse points are forbidden in the attestation path: {}",
            path.display()
        ));
    }
    let is_directory = information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    if is_directory != (kind == ProtectedPathKind::DirectoryWithExistingProtectedChild) {
        return Err(format!(
            "protected attestation path has the wrong object type: {}",
            path.display()
        ));
    }
    Ok(handle)
}

fn security_descriptor_for_handle(
    handle: HANDLE,
    path: &Path,
) -> Result<PSECURITY_DESCRIPTOR, String> {
    let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
    // SAFETY: the handle is live and the output descriptor pointer is valid.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            Some(&mut descriptor),
        )
    };
    if status.0 != 0 || descriptor.0.is_null() {
        return Err(format!(
            "cannot inspect attestation ACL for {}: {}",
            path.display(),
            status.0
        ));
    }
    Ok(descriptor)
}

fn read_bounded_marker(handle: HANDLE) -> Result<Vec<u8>, String> {
    let mut size = 0i64;
    // SAFETY: `handle` is a live, read-only marker handle.
    unsafe { GetFileSizeEx(handle, &mut size) }
        .map_err(|error| format!("cannot inspect attestation marker size: {error}"))?;
    if size <= 0 || size as u64 > MAX_ATTESTATION_BYTES {
        return Err("attestation must be a non-empty regular file no larger than 8 KiB".to_owned());
    }
    let mut bytes = vec![0u8; size as usize];
    let mut read = 0u32;
    // SAFETY: `bytes` is a writable buffer of the marker's bounded size.
    unsafe { ReadFile(handle, Some(&mut bytes), Some(&mut read), None) }
        .map_err(|error| format!("read attestation marker: {error}"))?;
    if read as usize != bytes.len() {
        return Err("attestation marker changed or was only partially read".to_owned());
    }
    Ok(bytes)
}

fn ensure_caller_cannot_mutate_descriptor(
    descriptor: PSECURITY_DESCRIPTOR,
    kind: ProtectedPathKind,
    description: &str,
) -> Result<(), String> {
    validate_security_descriptor(descriptor)?;
    let mut token = HANDLE::default();
    // SAFETY: current process is a pseudo-handle; token output pointer is valid.
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut token,
        )
    }
    .map_err(|error| format!("open current token for ACL check: {error}"))?;
    let _token = OwnedHandle(token);
    let mut impersonation_token = HANDLE::default();
    // AccessCheck requires an impersonation token, so duplicate the read-only
    // current token at SecurityImpersonation level for this local ACL query.
    unsafe { DuplicateToken(token, SecurityImpersonation, &mut impersonation_token) }
        .map_err(|error| format!("duplicate current token for ACL check: {error}"))?;
    let _impersonation_token = OwnedHandle(impersonation_token);

    let mapping = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ.0,
        GenericWrite: FILE_GENERIC_WRITE.0,
        GenericExecute: FILE_GENERIC_EXECUTE.0,
        GenericAll: FILE_ALL_ACCESS.0,
    };
    // AccessCheck maps the requested mask, but raw generic bits in ACEs from a
    // synthetic or non-canonical descriptor are not reliably interpreted as
    // object-specific file rights. Map the access ACE masks in this private
    // descriptor copy before evaluating its effective rights.
    map_generic_ace_rights(descriptor, &mapping)?;
    let dangerous_rights: &[(&str, u32)] = match kind {
        ProtectedPathKind::MarkerFile => &[
            ("FILE_WRITE_DATA", FILE_WRITE_DATA.0),
            ("FILE_APPEND_DATA", FILE_APPEND_DATA.0),
            ("FILE_WRITE_EA", FILE_WRITE_EA.0),
            ("FILE_WRITE_ATTRIBUTES", FILE_WRITE_ATTRIBUTES.0),
            ("DELETE", DELETE.0),
            ("WRITE_DAC", WRITE_DAC.0),
            ("WRITE_OWNER", WRITE_OWNER.0),
        ],
        // Add-file/add-subdirectory rights may create unrelated siblings, but
        // the caller has already opened the existing protected child while
        // holding this directory. Rebinding it still requires DELETE_CHILD,
        // DELETE on the child, or rights that change the controlling ACL.
        ProtectedPathKind::DirectoryWithExistingProtectedChild => &[
            ("FILE_WRITE_EA", FILE_WRITE_EA.0),
            ("FILE_WRITE_ATTRIBUTES", FILE_WRITE_ATTRIBUTES.0),
            ("FILE_DELETE_CHILD", FILE_DELETE_CHILD.0),
            ("DELETE", DELETE.0),
            ("WRITE_DAC", WRITE_DAC.0),
            ("WRITE_OWNER", WRITE_OWNER.0),
        ],
    };
    for (right_name, right) in dangerous_rights {
        if access_check_single_right(descriptor, impersonation_token, *right, &mapping)? {
            return Err(format!(
                "current test user can exercise {right_name} on protected attestation path: {description}"
            ));
        }
    }
    Ok(())
}

fn validate_security_descriptor(descriptor: PSECURITY_DESCRIPTOR) -> Result<(), String> {
    if descriptor.0.is_null() {
        return Err("security descriptor is null".to_owned());
    }
    // SAFETY: this pointer came from GetSecurityInfo or the controlled SDDL
    // converter used by regression tests; IsValidSecurityDescriptor checks it.
    if !unsafe { IsValidSecurityDescriptor(descriptor) }.as_bool() {
        return Err("security descriptor is invalid".to_owned());
    }
    let mut owner = PSID(null_mut());
    let mut owner_defaulted = windows::core::BOOL(0);
    // SAFETY: output pointers are valid and descriptor validity was checked.
    unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted) }
        .map_err(|error| format!("cannot inspect security descriptor owner: {error}"))?;
    let mut group = PSID(null_mut());
    let mut group_defaulted = windows::core::BOOL(0);
    // SAFETY: output pointers are valid and descriptor validity was checked.
    unsafe { GetSecurityDescriptorGroup(descriptor, &mut group, &mut group_defaulted) }
        .map_err(|error| format!("cannot inspect security descriptor group: {error}"))?;
    if owner.0.is_null() || group.0.is_null() {
        return Err("security descriptor has an ambiguous owner or group".to_owned());
    }
    let mut dacl_present = windows::core::BOOL(0);
    let mut dacl = std::ptr::null_mut();
    let mut dacl_defaulted = windows::core::BOOL(0);
    // SAFETY: output pointers are valid and descriptor validity was checked.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    }
    .map_err(|error| format!("cannot inspect security descriptor DACL: {error}"))?;
    if !dacl_present.as_bool() || dacl.is_null() {
        return Err("security descriptor has a missing or null DACL".to_owned());
    }
    Ok(())
}

fn map_generic_ace_rights(
    descriptor: PSECURITY_DESCRIPTOR,
    mapping: &GENERIC_MAPPING,
) -> Result<(), String> {
    let mut dacl_present = windows::core::BOOL(0);
    let mut dacl = std::ptr::null_mut();
    let mut dacl_defaulted = windows::core::BOOL(0);
    // SAFETY: descriptor was validated and output pointers are valid.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    }
    .map_err(|error| format!("cannot inspect security descriptor DACL: {error}"))?;
    if !dacl_present.as_bool() || dacl.is_null() {
        return Err("security descriptor has a missing or null DACL".to_owned());
    }

    let mut information = ACL_SIZE_INFORMATION::default();
    // SAFETY: the validated DACL is live and the output struct has the right size.
    unsafe {
        GetAclInformation(
            dacl,
            (&mut information as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    }
    .map_err(|error| format!("cannot inspect attestation ACL entries: {error}"))?;

    for index in 0..information.AceCount {
        let mut ace = null_mut();
        // SAFETY: index is bounded by the ACL's reported AceCount.
        unsafe { GetAce(dacl, index, &mut ace) }
            .map_err(|error| format!("cannot inspect attestation ACL entry: {error}"))?;
        if ace.is_null() {
            return Err("Windows returned a null attestation ACL entry".to_owned());
        }
        // SAFETY: GetAce returns a pointer to an ACE in the validated DACL.
        let header = unsafe { &mut *ace.cast::<ACE_HEADER>() };
        let has_access_mask = matches!(header.AceType, 0 | 1 | 4 | 5 | 6 | 9 | 10 | 11 | 12);
        if !has_access_mask {
            continue;
        }
        if usize::from(header.AceSize) < std::mem::size_of::<ACE_HEADER>() + size_of::<u32>() {
            return Err("attestation ACL contains a truncated access ACE".to_owned());
        }
        // SAFETY: access-allowed, access-denied, object, and callback ACEs place
        // their ACCESS_MASK immediately after ACE_HEADER; AceSize was checked.
        let mask = unsafe { ace.cast::<u8>().add(size_of::<ACE_HEADER>()).cast::<u32>() };
        // SAFETY: mask points into the validated writable descriptor allocation.
        unsafe { MapGenericMask(mask, mapping) };
    }
    Ok(())
}

fn access_check_single_right(
    descriptor: PSECURITY_DESCRIPTOR,
    impersonation_token: HANDLE,
    desired_access: u32,
    mapping: &GENERIC_MAPPING,
) -> Result<bool, String> {
    let mut privileges = [0usize; 64];
    let mut privilege_bytes = std::mem::size_of_val(&privileges) as u32;
    let mut granted = 0u32;
    let mut allowed = windows::core::BOOL(0);
    // Each call requests exactly one independent permission. FALSE therefore
    // cannot hide a different dangerous right granted by the same descriptor.
    // SAFETY: descriptor and token are live; output buffers remain alive.
    let mut desired_access = desired_access;
    // AccessCheck requires DesiredAccess to have no generic bits.
    // SAFETY: `desired_access` and `mapping` are valid for the call.
    unsafe { MapGenericMask(&mut desired_access, mapping) };
    // SAFETY: descriptor and token are live; output buffers remain alive.
    unsafe {
        AccessCheck(
            descriptor,
            impersonation_token,
            desired_access,
            mapping,
            Some(privileges.as_mut_ptr().cast::<PRIVILEGE_SET>()),
            &mut privilege_bytes,
            &mut granted,
            &mut allowed,
        )
    }
    .map_err(|error| format!("cannot prove attestation ACL is protected: {error}"))?;
    let right_granted = granted & desired_access != 0;
    if allowed.as_bool() && !right_granted {
        return Err("AccessCheck returned inconsistent granted-right evidence".to_owned());
    }
    Ok(right_granted)
}

#[cfg(test)]
mod acl_regression_tests {
    use super::*;

    struct TestDescriptor(PSECURITY_DESCRIPTOR);

    impl TestDescriptor {
        fn from_sddl(sddl: &str) -> Self {
            let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
            let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
            // SAFETY: the SDDL string is nul-terminated and descriptor output is valid.
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(wide.as_ptr()),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )
            }
            .expect("controlled in-memory SDDL descriptor converts");
            assert!(!descriptor.0.is_null());
            Self(descriptor)
        }
    }

    impl Drop for TestDescriptor {
        fn drop(&mut self) {
            // SAFETY: the SDDL converter allocates descriptors for LocalFree.
            unsafe { windows::Win32::Foundation::LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }

    fn user_sid() -> String {
        let mut token = HANDLE::default();
        // SAFETY: current process is a pseudo-handle; token output pointer is valid.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .expect("test process token opens for read-only identity query");
        let _token = OwnedHandle(token);
        let user_bytes = token_information(token, TokenUser).expect("token user is available");
        // SAFETY: token information buffer contains a valid TOKEN_USER.
        let user = unsafe { &*user_bytes.as_ptr().cast::<TOKEN_USER>() };
        let mut sid_text = PWSTR::null();
        // SAFETY: SID is owned by the live token buffer; Windows allocates output.
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_text) }
            .expect("current test SID converts to SDDL text");
        let sid = pwstr_to_string(sid_text);
        // SAFETY: ConvertSidToStringSidW allocates with LocalAlloc.
        unsafe { windows::Win32::Foundation::LocalFree(Some(HLOCAL(sid_text.0.cast()))) };
        assert!(!sid.is_empty());
        sid
    }

    fn descriptor_for_user(mask: &str) -> TestDescriptor {
        let sid = user_sid();
        TestDescriptor::from_sddl(&format!("O:S-1-5-18G:S-1-5-18D:(A;;{mask};;;{sid})"))
    }

    #[test]
    fn acl_rejects_single_file_write_data_grant() {
        let descriptor = descriptor_for_user("0x00000002");
        let error = ensure_caller_cannot_mutate_descriptor(
            descriptor.0,
            ProtectedPathKind::MarkerFile,
            "in-memory marker",
        )
        .expect_err("FILE_WRITE_DATA must be rejected even without WRITE_DAC");
        assert!(error.contains("FILE_WRITE_DATA"), "{error}");
    }

    #[test]
    fn acl_rejects_each_independent_file_mutation_right() {
        for (right, mask) in [
            ("FILE_WRITE_DATA", "0x00000002"),
            ("FILE_APPEND_DATA", "0x00000004"),
            ("FILE_WRITE_EA", "0x00000010"),
            ("FILE_WRITE_ATTRIBUTES", "0x00000100"),
            ("DELETE", "0x00010000"),
            ("WRITE_DAC", "0x00040000"),
            ("WRITE_OWNER", "0x00080000"),
        ] {
            let descriptor = descriptor_for_user(mask);
            let error = ensure_caller_cannot_mutate_descriptor(
                descriptor.0,
                ProtectedPathKind::MarkerFile,
                "in-memory marker",
            )
            .unwrap_err();
            assert!(error.contains(right), "expected {right}, got {error}");
        }
    }

    #[test]
    fn acl_accepts_read_only_marker_descriptor() {
        let descriptor = descriptor_for_user("0x00000001");
        ensure_caller_cannot_mutate_descriptor(
            descriptor.0,
            ProtectedPathKind::MarkerFile,
            "in-memory marker",
        )
        .expect("read-only marker permissions are accepted");
    }

    #[test]
    fn acl_accepts_add_subdirectory_for_existing_protected_child_parent() {
        // FILE_ADD_SUBDIRECTORY permits a new sibling, but cannot replace the
        // already-open protected child without FILE_DELETE_CHILD or child DELETE.
        // The production path walk verifies that child exists and holds each
        // parent open before applying this narrowly-scoped directory policy.
        let descriptor = descriptor_for_user("0x00000004");
        ensure_caller_cannot_mutate_descriptor(
            descriptor.0,
            ProtectedPathKind::DirectoryWithExistingProtectedChild,
            "in-memory existing-child parent",
        )
        .expect("creating an unrelated sibling does not rebind the existing child");
    }

    #[test]
    fn acl_rejects_add_subdirectory_combined_with_child_replacement() {
        let descriptor = descriptor_for_user("0x00000044");
        let error = ensure_caller_cannot_mutate_descriptor(
            descriptor.0,
            ProtectedPathKind::DirectoryWithExistingProtectedChild,
            "in-memory existing-child parent",
        )
        .expect_err("FILE_DELETE_CHILD enables replacement of the protected child");
        assert!(error.contains("FILE_DELETE_CHILD"), "{error}");
    }

    #[test]
    fn acl_rejects_each_independent_directory_rebinding_right() {
        for (right, mask) in [
            ("FILE_WRITE_EA", "0x00000010"),
            ("FILE_WRITE_ATTRIBUTES", "0x00000100"),
            ("FILE_DELETE_CHILD", "0x00000040"),
            ("DELETE", "0x00010000"),
            ("WRITE_DAC", "0x00040000"),
            ("WRITE_OWNER", "0x00080000"),
        ] {
            let descriptor = descriptor_for_user(mask);
            let error = ensure_caller_cannot_mutate_descriptor(
                descriptor.0,
                ProtectedPathKind::DirectoryWithExistingProtectedChild,
                "in-memory existing-child parent",
            )
            .unwrap_err();
            assert!(error.contains(right), "expected {right}, got {error}");
        }
    }

    #[test]
    fn acl_rejects_null_dacl_and_missing_owner() {
        let null_dacl = TestDescriptor::from_sddl("O:S-1-5-18G:S-1-5-18D:NO_ACCESS_CONTROL");
        let error = ensure_caller_cannot_mutate_descriptor(
            null_dacl.0,
            ProtectedPathKind::MarkerFile,
            "in-memory null-DACL marker",
        )
        .expect_err("null DACL is ambiguous and must fail closed");
        assert!(error.contains("null DACL"), "{error}");

        let missing_owner = TestDescriptor::from_sddl("G:S-1-5-18D:");
        let error = ensure_caller_cannot_mutate_descriptor(
            missing_owner.0,
            ProtectedPathKind::MarkerFile,
            "in-memory ownerless marker",
        )
        .expect_err("ownerless descriptor is ambiguous and must fail closed");
        assert!(error.contains("owner"), "{error}");
    }

    #[test]
    fn acl_rejects_implicit_owner_write_dac() {
        let sid = user_sid();
        let descriptor =
            TestDescriptor::from_sddl(&format!("O:{sid}G:S-1-5-18D:(A;;0x00000001;;;{sid})"));
        let error = ensure_caller_cannot_mutate_descriptor(
            descriptor.0,
            ProtectedPathKind::MarkerFile,
            "in-memory owner marker",
        )
        .expect_err("owner has implicit WRITE_DAC despite a read-only explicit ACE");
        assert!(error.contains("WRITE_DAC"), "{error}");
    }

    #[test]
    fn acl_rejects_generic_write_after_mapping() {
        let descriptor = descriptor_for_user("0x40000000");
        let error = ensure_caller_cannot_mutate_descriptor(
            descriptor.0,
            ProtectedPathKind::MarkerFile,
            "in-memory GENERIC_WRITE marker",
        )
        .expect_err("GENERIC_WRITE maps to dangerous file rights");
        assert!(error.contains("FILE_WRITE_DATA"), "{error}");
    }

    #[test]
    fn acl_access_check_api_error_fails_closed() {
        let descriptor = descriptor_for_user("0x00000001");
        let mapping = GENERIC_MAPPING {
            GenericRead: FILE_GENERIC_READ.0,
            GenericWrite: FILE_GENERIC_WRITE.0,
            GenericExecute: FILE_GENERIC_EXECUTE.0,
            GenericAll: FILE_ALL_ACCESS.0,
        };
        let error =
            access_check_single_right(descriptor.0, HANDLE::default(), FILE_WRITE_DATA.0, &mapping)
                .expect_err("invalid token handle must surface as an API error");
        assert!(error.contains("cannot prove"), "{error}");
    }
}

fn current_identity() -> Result<Identity, String> {
    let mut token = HANDLE::default();
    // SAFETY: current process is a pseudo-handle; token output pointer is valid.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|error| format!("open current process token: {error}"))?;
    let _token = OwnedHandle(token);

    let user_bytes = token_information(token, TokenUser)?;
    // SAFETY: the token information buffer was sized and filled for TOKEN_USER.
    let user = unsafe { &*user_bytes.as_ptr().cast::<TOKEN_USER>() };
    let mut sid_text = PWSTR::null();
    // SAFETY: `user.User.Sid` is owned by the live token information buffer.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_text) }
        .map_err(|error| format!("convert current user SID: {error}"))?;
    let user_sid = pwstr_to_string(sid_text);
    // SAFETY: ConvertSidToStringSidW allocated this string with LocalAlloc.
    unsafe { windows::Win32::Foundation::LocalFree(Some(HLOCAL(sid_text.0.cast::<c_void>()))) };
    let integrity_bytes = token_information(token, TokenIntegrityLevel)?;
    // SAFETY: the token information buffer was sized and filled for TOKEN_MANDATORY_LABEL.
    let integrity = unsafe { &*integrity_bytes.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    // SAFETY: the SID pointer is owned by the live token information buffer.
    let count_ptr = unsafe { GetSidSubAuthorityCount(integrity.Label.Sid) };
    if count_ptr.is_null() {
        return Err("current token integrity SID is invalid".to_owned());
    }
    // SAFETY: `count_ptr` was checked against null and belongs to the live SID.
    let subauthority_count = unsafe { *count_ptr };
    if subauthority_count == 0 {
        return Err("current token integrity SID is invalid".to_owned());
    }
    // SAFETY: the final sub-authority index is validated from the SID header.
    let rid_ptr =
        unsafe { GetSidSubAuthority(integrity.Label.Sid, u32::from(subauthority_count) - 1) };
    if rid_ptr.is_null() {
        return Err("current token integrity RID is unavailable".to_owned());
    }
    // SAFETY: GetSidSubAuthority returned the final, validated sub-authority pointer.
    let integrity_rid = unsafe { *rid_ptr };
    let elevation_bytes = token_information(token, TokenElevation)?;
    // SAFETY: the token information buffer was sized and filled for TOKEN_ELEVATION.
    let elevation = unsafe { &*elevation_bytes.as_ptr().cast::<TOKEN_ELEVATION>() };

    let process_id = unsafe { GetCurrentProcessId() };
    let mut session_id = 0;
    // SAFETY: session output pointer is valid.
    unsafe { ProcessIdToSessionId(process_id, &mut session_id) }
        .map_err(|error| format!("resolve current session: {error}"))?;
    assert_session_active(session_id)?;

    Ok(Identity {
        machine_guid_sha256: machine_guid_sha256()?,
        user_sid,
        session_id,
        integrity_rid,
        elevated: elevation.TokenIsElevated != 0,
    })
}

fn token_information(
    token: HANDLE,
    information_class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<usize>, String> {
    let mut required = 0u32;
    // SAFETY: this first query asks Windows for the required buffer size only.
    let _ = unsafe { GetTokenInformation(token, information_class, None, 0, &mut required) };
    if required == 0 || required as usize > 64 * 1024 {
        return Err("Windows returned an invalid token-information size".to_owned());
    }
    let words = (required as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0usize; words];
    // SAFETY: aligned buffer has the requested byte capacity and remains live.
    unsafe {
        GetTokenInformation(
            token,
            information_class,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            (buffer.len() * size_of::<usize>()) as u32,
            &mut required,
        )
    }
    .map_err(|error| format!("read Windows token information: {error}"))?;
    Ok(buffer)
}

fn pwstr_to_string(value: PWSTR) -> String {
    if value.0.is_null() {
        return String::new();
    }
    let mut length = 0usize;
    // SAFETY: the API returns a valid nul-terminated string allocation.
    while unsafe { *value.0.add(length) } != 0 {
        length += 1;
        if length > 256 {
            return String::new();
        }
    }
    // SAFETY: `length` counts code units before the nul terminator.
    String::from_utf16_lossy(unsafe { slice::from_raw_parts(value.0, length) })
}

fn machine_guid_sha256() -> Result<String, String> {
    let mut key = HKEY::default();
    // SAFETY: registry key output pointer is valid and the requested access is read-only.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Cryptography"),
            Some(0),
            KEY_READ,
            &mut key,
        )
    };
    if status.0 != 0 {
        return Err(format!("open machine identity registry key: {}", status.0));
    }
    let _key = OwnedRegKey(key);
    let mut value_type = REG_VALUE_TYPE(0);
    let mut byte_count = 0u32;
    // SAFETY: size-only query; value buffers are optional.
    let status = unsafe {
        RegQueryValueExW(
            key,
            w!("MachineGuid"),
            None,
            Some(&mut value_type),
            None,
            Some(&mut byte_count),
        )
    };
    if status.0 != 0 || value_type != REG_SZ || !(4..=1024).contains(&byte_count) {
        return Err("machine GUID registry value is unavailable or malformed".to_owned());
    }
    let mut value = vec![0u16; (byte_count as usize).div_ceil(size_of::<u16>())];
    // SAFETY: `value` is sized from the read-only size query and correctly aligned.
    let status = unsafe {
        RegQueryValueExW(
            key,
            w!("MachineGuid"),
            None,
            Some(&mut value_type),
            Some(value.as_mut_ptr().cast::<u8>()),
            Some(&mut byte_count),
        )
    };
    if status.0 != 0 || value_type != REG_SZ {
        return Err("read machine GUID registry value failed".to_owned());
    }
    if let Some(end) = value.iter().position(|unit| *unit == 0) {
        value.truncate(end);
    }
    let guid = String::from_utf16(&value)
        .map_err(|error| format!("machine GUID is not UTF-16: {error}"))?;
    if guid.is_empty() {
        return Err("machine GUID is empty".to_owned());
    }
    Ok(format!("{:x}", Sha256::digest(guid.as_bytes())))
}

fn assert_no_production_qbit_process() -> Result<(), String> {
    // SAFETY: a process snapshot is read-only and does not open or modify targets.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|error| format!("enumerate processes: {error}"))?;
    let _snapshot = OwnedHandle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: entry is initialized with its ABI size and writable output storage.
    unsafe { Process32FirstW(snapshot, &mut entry) }
        .map_err(|error| format!("read process snapshot: {error}"))?;
    loop {
        let name = String::from_utf16_lossy(
            &entry.szExeFile[..entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.szExeFile.len())],
        );
        if name.eq_ignore_ascii_case("qbit-desktop.exe") {
            return Err(
                "production qbit-desktop.exe is running; isolated output is refused".to_owned(),
            );
        }
        // SAFETY: entry remains valid writable storage for the next snapshot row.
        if let Err(error) = unsafe { Process32NextW(snapshot, &mut entry) } {
            let last_error = unsafe { GetLastError() };
            if last_error == ERROR_NO_MORE_FILES {
                break;
            }
            return Err(format!("continue process snapshot: {error}"));
        }
    }
    Ok(())
}

fn assert_default_input_desktop() -> Result<(), String> {
    // SAFETY: opens the current input desktop read-only; it does not switch desktops.
    let desktop = unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) }
        .map_err(|error| {
            format!("active input desktop is unavailable (locked/disconnected?): {error}")
        })?;
    let mut name = [0u16; 64];
    let mut needed = 0u32;
    // SAFETY: desktop is live and `name` is bounded writable storage.
    let result = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast::<c_void>()),
            std::mem::size_of_val(&name) as u32,
            Some(&mut needed),
        )
    };
    // SAFETY: `desktop` was returned by OpenInputDesktop and is owned here.
    let close_result = unsafe { CloseDesktop(desktop) };
    result.map_err(|error| format!("read active input desktop name: {error}"))?;
    close_result.map_err(|error| format!("close input desktop handle: {error}"))?;
    let end = name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(name.len());
    if String::from_utf16_lossy(&name[..end]) != "Default" {
        return Err("input desktop is not the unlocked Default desktop".to_owned());
    }
    Ok(())
}

fn assert_session_active(session_id: u32) -> Result<(), String> {
    let mut connection_state = PWSTR::null();
    let mut byte_count = 0u32;
    // SAFETY: Windows returns a bounded WTS session-state buffer owned by WTSFreeMemory.
    unsafe {
        windows::Win32::System::RemoteDesktop::WTSQuerySessionInformationW(
            Some(windows::Win32::System::RemoteDesktop::WTS_CURRENT_SERVER_HANDLE),
            session_id,
            windows::Win32::System::RemoteDesktop::WTSConnectState,
            &mut connection_state,
            &mut byte_count,
        )
    }
    .map_err(|error| format!("query current WTS session state: {error}"))?;
    if connection_state.0.is_null()
        || byte_count
            < size_of::<windows::Win32::System::RemoteDesktop::WTS_CONNECTSTATE_CLASS>() as u32
    {
        if !connection_state.0.is_null() {
            // SAFETY: WTSQuerySessionInformationW returned this allocation.
            unsafe {
                windows::Win32::System::RemoteDesktop::WTSFreeMemory(connection_state.0.cast())
            };
        }
        return Err("WTS returned an invalid session-state buffer".to_owned());
    }
    // SAFETY: buffer size was checked and WTS returns this enum at its start.
    let state = unsafe {
        *connection_state
            .0
            .cast::<windows::Win32::System::RemoteDesktop::WTS_CONNECTSTATE_CLASS>()
    };
    // SAFETY: WTSQuerySessionInformationW returned this allocation.
    unsafe { windows::Win32::System::RemoteDesktop::WTSFreeMemory(connection_state.0.cast()) };
    if state != windows::Win32::System::RemoteDesktop::WTSActive {
        return Err("test session is disconnected or inactive".to_owned());
    }
    Ok(())
}

fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, String> {
        // SAFETY: this test thread initializes an STA for known-folder lookup.
        unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            )
        }
        .ok()
        .map_err(|error| format!("initialize COM STA for isolated runner preflight: {error}"))?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: balances this test thread's successful CoInitializeEx call.
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
}

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: each wrapper owns a real handle returned by a matching open API.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct WorkerOwner {
    shared: Arc<Shared>,
    worker: Option<Worker>,
}

impl WorkerOwner {
    fn start(shared: Arc<Shared>, injector: Arc<dyn Injector>) -> Self {
        let worker = Worker::start(shared.clone(), injector).expect("output worker starts");
        Self {
            shared,
            worker: Some(worker),
        }
    }

    fn thread(&self) -> &std::thread::Thread {
        &self.worker.as_ref().expect("worker is still owned").thread
    }

    fn stop(&mut self) -> Result<(), &'static str> {
        self.shared.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.as_mut() {
            worker.thread.unpark();
            worker.stop()?;
        }
        self.worker = None;
        Ok(())
    }
}

impl Drop for WorkerOwner {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.as_mut() {
            worker.thread.unpark();
            let _ = worker.stop();
        }
        self.worker = None;
    }
}

struct OwnedRegKey(HKEY);
impl Drop for OwnedRegKey {
    fn drop(&mut self) {
        // SAFETY: each wrapper owns a registry key returned by RegOpenKeyExW.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

#[derive(Clone, Copy)]
struct KeyObservation {
    message: u32,
    vk: usize,
}

static OBSERVED: OnceLock<ArrayQueue<KeyObservation>> = OnceLock::new();
static OBSERVATION_OVERFLOW: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn receiver_wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    if matches!(message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP) {
        if let Some(queue) = OBSERVED.get()
            && queue
                .push(KeyObservation {
                    message,
                    vk: wparam.0,
                })
                .is_err()
        {
            OBSERVATION_OVERFLOW.store(true, Ordering::Release);
        }
        return windows::Win32::Foundation::LRESULT(0);
    }
    // SAFETY: unhandled window messages are forwarded with their original parameters.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

struct ReceiverWindow {
    hwnd: isize,
    class_name: Vec<u16>,
    session_id: u32,
}

impl ReceiverWindow {
    fn create(session_id: u32) -> Result<Self, String> {
        OBSERVED.get_or_init(|| ArrayQueue::new(8));
        OBSERVATION_OVERFLOW.store(false, Ordering::Release);
        while OBSERVED.get().is_some_and(|queue| queue.pop().is_some()) {}

        let class_name: Vec<u16> =
            format!("QbitHotkeysReceiver-{}", unsafe { GetCurrentProcessId() })
                .encode_utf16()
                .chain(Some(0))
                .collect();
        // SAFETY: null module name requests this executable's module handle.
        let module = unsafe { GetModuleHandleW(PCWSTR::null()) }
            .map_err(|error| format!("resolve test executable module: {error}"))?;
        let instance = windows::Win32::Foundation::HINSTANCE(module.0);
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(receiver_wnd_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: class descriptor and name remain live during registration.
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return Err(format!("register receiver class failed: {:?}", unsafe {
                GetLastError()
            }));
        }
        // SAFETY: class is registered and every HWND parameter is explicit/null.
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                PCWSTR(class_name.as_ptr()),
                w!("Qbit Hotkeys Isolated Output Receiver"),
                WS_OVERLAPPEDWINDOW,
                40,
                40,
                360,
                180,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .map_err(|error| format!("create isolated receiver window: {error}"))?;
        // SAFETY: hwnd is the test process's own freshly created receiver.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        }
        // Windows may refuse foreground activation; refusal is a hard failure.
        // SAFETY: hwnd is owned by this process; no keyboard input is sent here.
        if !unsafe { SetForegroundWindow(hwnd) }.as_bool() {
            // SAFETY: the just-created window is owned by this process.
            unsafe { DestroyWindow(hwnd) }.ok();
            // SAFETY: no receiver window remains after the failed activation.
            unsafe { UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(instance)) }.ok();
            return Err("Windows refused foreground activation for the owned receiver".to_owned());
        }
        let receiver = Self {
            hwnd: hwnd.0 as isize,
            class_name,
            session_id,
        };
        receiver.verify_foreground()?;
        Ok(receiver)
    }

    fn verify_foreground(&self) -> Result<(), String> {
        let process_id = unsafe { GetCurrentProcessId() };
        let mut current_session = 0u32;
        // SAFETY: output pointer is valid and the PID belongs to this test process.
        unsafe { ProcessIdToSessionId(process_id, &mut current_session) }
            .map_err(|error| format!("recheck output session: {error}"))?;
        if current_session != self.session_id {
            return Err("output session changed after attestation".to_owned());
        }
        assert_session_active(self.session_id)?;
        // SAFETY: read-only foreground query.
        if unsafe { GetForegroundWindow() } != self.hwnd() {
            return Err("receiver HWND is no longer foreground; no batch was sent".to_owned());
        }
        let mut process_id = 0u32;
        // SAFETY: hwnd is validated by equality with our live receiver; PID output is valid.
        let thread_id = unsafe { GetWindowThreadProcessId(self.hwnd(), Some(&mut process_id)) };
        if thread_id == 0 || process_id != unsafe { GetCurrentProcessId() } {
            return Err("foreground receiver ownership is ambiguous".to_owned());
        }
        assert_default_input_desktop()?;
        assert_no_production_qbit_process()?;
        Ok(())
    }

    fn observe_f10(&self) -> Result<[u32; 2], String> {
        self.verify_foreground()?;
        let deadline = Instant::now() + RECEIVER_TIMEOUT;
        while OBSERVED.get().is_some_and(|queue| queue.len() < 2) {
            self.verify_foreground()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("timed out waiting for F10 receiver messages".to_owned());
            }
            let timeout_ms = remaining.as_millis().clamp(1, u32::MAX as u128) as u32;
            // SAFETY: bounded wait on the current thread's message queue; no handles are waited.
            let wait = unsafe { MsgWaitForMultipleObjects(None, false, timeout_ms, QS_ALLINPUT) };
            if wait == WAIT_TIMEOUT {
                return Err("timed out waiting for receiver window messages".to_owned());
            }
            if wait != WAIT_OBJECT_0 {
                return Err(format!("message wait failed with code {}", wait.0));
            }
            loop {
                let mut message = MSG::default();
                // SAFETY: message output is valid; PM_REMOVE removes only this test thread's queue.
                if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                    break;
                }
                // SAFETY: messages were removed from this test process's own queue.
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            if OBSERVATION_OVERFLOW.load(Ordering::Acquire) {
                return Err("receiver observation queue overflowed".to_owned());
            }
        }
        self.verify_foreground()?;
        let queue = OBSERVED
            .get()
            .ok_or("receiver observations were not initialized")?;
        let first = queue.pop().ok_or("F10 key-down message is missing")?;
        let second = queue.pop().ok_or("F10 key-up message is missing")?;
        if first.vk != usize::from(F10_VK) || second.vk != usize::from(F10_VK) {
            return Err("receiver observed a key other than the fixed F10 fixture".to_owned());
        }
        if !queue.is_empty() || OBSERVATION_OVERFLOW.load(Ordering::Acquire) {
            return Err("receiver observed extra or unbounded key messages".to_owned());
        }
        Ok([first.message, second.message])
    }

    fn hwnd(&self) -> windows::Win32::Foundation::HWND {
        windows::Win32::Foundation::HWND(self.hwnd as *mut c_void)
    }
}

impl Drop for ReceiverWindow {
    fn drop(&mut self) {
        // SAFETY: destroy and unregister only this test process's owned HWND/class.
        if unsafe { IsWindow(Some(self.hwnd())) }.as_bool() {
            unsafe { DestroyWindow(self.hwnd()) }.ok();
        }
        // SAFETY: this class name is unique to this process and was registered here.
        if let Ok(module) = unsafe { GetModuleHandleW(PCWSTR::null()) } {
            unsafe {
                UnregisterClassW(
                    PCWSTR(self.class_name.as_ptr()),
                    Some(windows::Win32::Foundation::HINSTANCE(module.0)),
                )
            }
            .ok();
        }
    }
}

struct GuardedSystemInjector {
    receiver: Arc<ReceiverWindow>,
    system: IsolatedSystemInjector,
    send_count: std::sync::atomic::AtomicUsize,
    f10_down: Mutex<bool>,
    release_attempted: AtomicBool,
    error: Mutex<Option<String>>,
}

impl GuardedSystemInjector {
    fn new(receiver: Arc<ReceiverWindow>) -> Self {
        Self {
            receiver,
            system: IsolatedSystemInjector,
            send_count: std::sync::atomic::AtomicUsize::new(0),
            f10_down: Mutex::new(false),
            release_attempted: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }

    fn release_if_needed(&self) -> Result<(), String> {
        if self.release_attempted.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let mut is_down = self
            .f10_down
            .lock()
            .map_err(|_| "F10 state lock is poisoned")?;
        if !*is_down {
            return Ok(());
        }
        self.receiver.verify_foreground()?;
        let release = EncodedInput {
            vk: F10_VK,
            scan: 0,
            extended: false,
            scan_mode: false,
            up: true,
            tag: QBIT_INJECTION_TAG,
        };
        let sent = self.system.send(&[release]);
        if sent != 1 {
            return Err(
                "F10 cleanup key-up was not fully accepted; cleanup is uncertain".to_owned(),
            );
        }
        *is_down = false;
        Ok(())
    }
}

impl Injector for GuardedSystemInjector {
    fn send(&self, events: &[EncodedInput]) -> usize {
        if let Err(error) = self.receiver.verify_foreground() {
            *self.error.lock().expect("guard error lock is healthy") = Some(error);
            return 0;
        }
        if events.len() != 2
            || events[0].vk != F10_VK
            || events[0].up
            || events[1].vk != F10_VK
            || !events[1].up
            || events.iter().any(|event| event.tag != QBIT_INJECTION_TAG)
        {
            *self.error.lock().expect("guard error lock is healthy") =
                Some("unexpected output batch; only tagged F10 down/up is permitted".to_owned());
            return 0;
        }
        let sent = self.system.send(events);
        self.send_count.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut is_down) = self.f10_down.lock() {
            for event in events.iter().take(sent) {
                *is_down = !event.up;
            }
        } else {
            *self.error.lock().expect("guard error lock is healthy") =
                Some("F10 state lock is poisoned".to_owned());
        }
        sent
    }
}

impl Drop for GuardedSystemInjector {
    fn drop(&mut self) {
        let _ = self.release_if_needed();
    }
}
