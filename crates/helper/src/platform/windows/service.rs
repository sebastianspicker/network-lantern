//! Native Windows Service Control Manager and UAC lifecycle.

use super::{serve, validate_regular_directory, validate_regular_file};
use crate::{HelperError, HelperServer, Result};
use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_INSUFFICIENT_BUFFER,
        ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_EXISTS,
        ERROR_SERVICE_NOT_ACTIVE, GetLastError, INVALID_HANDLE_VALUE, MAX_PATH, WAIT_OBJECT_0,
    },
    Security::WinTrust::{
        WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
        WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
        WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
    },
    Storage::FileSystem::{
        CreateFileW, DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_APPEND_DATA,
        FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_WRITE_DATA, OPEN_EXISTING, WRITE_DAC, WRITE_OWNER,
    },
    System::{
        Services::{
            CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW,
            OpenServiceW, QueryServiceConfigW, QueryServiceStatusEx, RegisterServiceCtrlHandlerExW,
            SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE, SC_STATUS_PROCESS_INFO,
            SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE,
            SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_DEMAND_START,
            SERVICE_ERROR_NORMAL, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
            SERVICE_START, SERVICE_START_PENDING, SERVICE_STATUS, SERVICE_STATUS_HANDLE,
            SERVICE_STATUS_PROCESS, SERVICE_STOP, SERVICE_STOP_PENDING, SERVICE_STOPPED,
            SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS, SetServiceStatus,
            StartServiceCtrlDispatcherW, StartServiceW,
        },
        Threading::{GetExitCodeProcess, WaitForSingleObject},
    },
    UI::{
        Shell::{
            CSIDL_PROGRAM_FILES, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
            SHGFP_TYPE_CURRENT, SHGetFolderPathW, ShellExecuteExW,
        },
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

const SERVICE_NAME: &str = "NetworkLanternHelper";
const DISPLAY_NAME: &str = "Network Lantern Privileged Helper";
const HELPER_FILE_NAME: &str = "network-lantern-helper.exe";
const SERVICE_ARGUMENT: &str = "--service";
const ELEVATION_WAIT_MS: u32 = 120_000;
const SERVICE_WAIT: Duration = Duration::from_secs(20);

static SERVER: OnceLock<Mutex<Option<HelperServer>>> = OnceLock::new();
static STOP: OnceLock<CancellationToken> = OnceLock::new();
static STATUS_HANDLE: OnceLock<usize> = OnceLock::new();

struct ScHandle(windows_sys::Win32::System::Services::SC_HANDLE);

impl ScHandle {
    fn new(
        raw: windows_sys::Win32::System::Services::SC_HANDLE,
        operation: &'static str,
    ) -> Result<Self> {
        if raw.is_null() {
            Err(last(operation))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for ScHandle {
    fn drop(&mut self) {
        unsafe { CloseServiceHandle(self.0) };
    }
}

pub(super) fn is_registered() -> Result<bool> {
    let manager = open_manager(SC_MANAGER_CONNECT)?;
    let name = wide(SERVICE_NAME);
    let service = unsafe { OpenServiceW(manager.0, name.as_ptr(), SERVICE_QUERY_STATUS) };
    if service.is_null() {
        if unsafe { GetLastError() } == ERROR_SERVICE_DOES_NOT_EXIST {
            return Ok(false);
        }
        return Err(last("open Windows helper service"));
    }
    drop(ScHandle(service));
    Ok(true)
}

pub(super) fn is_stopped() -> Result<bool> {
    let manager = open_manager(SC_MANAGER_CONNECT)?;
    let name = wide(SERVICE_NAME);
    let raw = unsafe { OpenServiceW(manager.0, name.as_ptr(), SERVICE_QUERY_STATUS) };
    if raw.is_null() {
        if unsafe { GetLastError() } == ERROR_SERVICE_DOES_NOT_EXIST {
            return Ok(true);
        }
        return Err(last("open Windows helper service state"));
    }
    query_state(&ScHandle(raw)).map(|state| state == SERVICE_STOPPED)
}

pub(super) fn elevate_install(owner_sid: &str) -> Result<()> {
    elevate(ElevationAction::Install(owner_sid))
}

pub(super) fn elevate_remove() -> Result<()> {
    elevate(ElevationAction::Remove)
}

enum ElevationAction<'a> {
    Install(&'a str),
    Remove,
}

fn elevate(action: ElevationAction<'_>) -> Result<()> {
    let app = std::env::current_exe()
        .map_err(|error| HelperError::transport("locate Network Lantern executable", error))?;
    let helper = deployed_helper_for(&app)?;
    ensure_deployment_not_user_writable(&helper)?;
    verify_authenticode(&helper)?;

    let parameters = match action {
        ElevationAction::Install(owner) => {
            ensure_file_not_user_writable(&helper)?;
            format!("--install-service {owner}")
        }
        ElevationAction::Remove => "--remove-service".to_owned(),
    };
    let verb = wide("runas");
    let file = wide_os(helper.as_os_str());
    let args = wide(&parameters);
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        hwnd: null_mut(),
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: args.as_ptr(),
        lpDirectory: null(),
        nShow: SW_SHOWNORMAL,
        hInstApp: null_mut(),
        lpIDList: null_mut(),
        lpClass: null(),
        hkeyClass: null_mut(),
        dwHotKey: 0,
        Anonymous: Default::default(),
        hProcess: null_mut(),
    };
    if unsafe { ShellExecuteExW(&mut execute) } == 0 {
        return if unsafe { GetLastError() } == ERROR_CANCELLED {
            Err(HelperError::Authorization(
                "Windows elevation was cancelled".into(),
            ))
        } else {
            Err(last("launch elevated Windows helper"))
        };
    }
    if execute.hProcess.is_null() {
        return Err(HelperError::Unavailable(
            "Windows elevation did not return a process handle".into(),
        ));
    }
    let wait = unsafe { WaitForSingleObject(execute.hProcess, ELEVATION_WAIT_MS) };
    if wait != WAIT_OBJECT_0 {
        unsafe { CloseHandle(execute.hProcess) };
        return Err(HelperError::Unavailable(
            "Elevated Windows helper did not finish within two minutes".into(),
        ));
    }
    let mut exit_code = 1;
    let read_exit = unsafe { GetExitCodeProcess(execute.hProcess, &mut exit_code) };
    unsafe { CloseHandle(execute.hProcess) };
    if read_exit == 0 {
        return Err(last("read elevated Windows helper result"));
    }
    if exit_code != 0 {
        return Err(HelperError::Unavailable(format!(
            "Elevated Windows helper exited with code {exit_code}"
        )));
    }
    Ok(())
}

pub(super) fn install(_owner_sid: &str) -> Result<()> {
    let helper = installed_helper_path()?;
    verify_authenticode(&helper)?;
    let command = service_command(&helper)?;
    let manager = open_manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    let name = wide(SERVICE_NAME);
    let display = wide(DISPLAY_NAME);
    let binary = wide(&command);
    let raw = unsafe {
        CreateServiceW(
            manager.0,
            name.as_ptr(),
            display.as_ptr(),
            SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_START | SERVICE_STOP,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_DEMAND_START,
            SERVICE_ERROR_NORMAL,
            binary.as_ptr(),
            null(),
            null_mut(),
            null(),
            null(),
            null(),
        )
    };
    let service = if raw.is_null() {
        if unsafe { GetLastError() } != ERROR_SERVICE_EXISTS {
            return Err(last("create Windows helper service"));
        }
        let existing = unsafe {
            OpenServiceW(
                manager.0,
                name.as_ptr(),
                SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_START,
            )
        };
        let existing = ScHandle::new(existing, "open existing Windows helper service")?;
        ensure_service_command(&existing, &command)?;
        existing
    } else {
        ScHandle(raw)
    };
    if unsafe { StartServiceW(service.0, 0, null()) } == 0
        && unsafe { GetLastError() } != ERROR_SERVICE_ALREADY_RUNNING
    {
        return Err(last("start Windows helper service"));
    }
    wait_service_state(&service, SERVICE_RUNNING)
}

pub(super) fn remove() -> Result<()> {
    let manager = open_manager(SC_MANAGER_CONNECT)?;
    let name = wide(SERVICE_NAME);
    let raw = unsafe {
        OpenServiceW(
            manager.0,
            name.as_ptr(),
            SERVICE_QUERY_STATUS | SERVICE_STOP | DELETE,
        )
    };
    if raw.is_null() {
        if unsafe { GetLastError() } == ERROR_SERVICE_DOES_NOT_EXIST {
            return Ok(());
        }
        return Err(last("open Windows helper service for removal"));
    }
    let service = ScHandle(raw);
    let state = query_state(&service)?;
    if state != SERVICE_STOPPED {
        let mut status = SERVICE_STATUS::default();
        if unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut status) } == 0
            && unsafe { GetLastError() } != ERROR_SERVICE_NOT_ACTIVE
        {
            return Err(last("stop Windows helper service"));
        }
        wait_service_state(&service, SERVICE_STOPPED)?;
    }
    if unsafe { DeleteService(service.0) } == 0 {
        return Err(last("delete Windows helper service"));
    }
    Ok(())
}

pub(super) fn run_dispatcher(server: HelperServer) -> Result<()> {
    SERVER.set(Mutex::new(Some(server))).map_err(|_| {
        HelperError::Protocol("Windows service dispatcher was initialized twice".into())
    })?;
    let mut name = wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        return Err(last("start Windows service control dispatcher"));
    }
    Ok(())
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut windows_sys::core::PWSTR) {
    let name = wide(SERVICE_NAME);
    let handle =
        unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control_handler), null()) };
    if handle.is_null() {
        return;
    }
    let _ = STATUS_HANDLE.set(handle as usize);
    report_status(SERVICE_START_PENDING, 0, 10_000);
    let stop = CancellationToken::new();
    let _ = STOP.set(stop.clone());
    let server = SERVER
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|mut slot| slot.take());
    let Some(server) = server else {
        report_status(SERVICE_STOPPED, 1, 0);
        return;
    };
    report_status(SERVICE_RUNNING, 0, 0);
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            HelperError::Unavailable(format!("create Windows helper runtime: {error}"))
        })
        .and_then(|runtime| runtime.block_on(serve(server, stop)));
    report_status(SERVICE_STOPPED, u32::from(result.is_err()), 0);
}

unsafe extern "system" fn control_handler(
    control: u32,
    _event_type: u32,
    _event_data: *mut core::ffi::c_void,
    _context: *mut core::ffi::c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            report_status(SERVICE_STOP_PENDING, 0, 10_000);
            if let Some(stop) = STOP.get() {
                stop.cancel();
            }
        }
        SERVICE_CONTROL_INTERROGATE => report_status(SERVICE_RUNNING, 0, 0),
        _ => {}
    }
    0
}

fn report_status(state: u32, exit_code: u32, wait_hint: u32) {
    let Some(handle) = STATUS_HANDLE.get().copied() else {
        return;
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: wait_hint,
    };
    unsafe { SetServiceStatus(handle as SERVICE_STATUS_HANDLE, &status) };
}

fn open_manager(access: u32) -> Result<ScHandle> {
    ScHandle::new(
        unsafe { OpenSCManagerW(null(), null(), access) },
        "open Windows service manager",
    )
}

fn query_state(service: &ScHandle) -> Result<u32> {
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0;
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(last("query Windows helper service state"));
    }
    Ok(status.dwCurrentState)
}

fn wait_service_state(service: &ScHandle, expected: u32) -> Result<()> {
    let deadline = Instant::now() + SERVICE_WAIT;
    loop {
        if query_state(service)? == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(HelperError::Unavailable(
                "Windows helper service state transition timed out".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn ensure_service_command(service: &ScHandle, expected: &str) -> Result<()> {
    let mut needed = 0;
    unsafe { QueryServiceConfigW(service.0, null_mut(), 0, &mut needed) };
    if needed == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(last("size Windows helper service configuration"));
    }
    let mut buffer = vec![0_u8; needed as usize];
    if unsafe { QueryServiceConfigW(service.0, buffer.as_mut_ptr().cast(), needed, &mut needed) }
        == 0
    {
        return Err(last("read Windows helper service configuration"));
    }
    let config = unsafe {
        &*(buffer
            .as_ptr()
            .cast::<windows_sys::Win32::System::Services::QUERY_SERVICE_CONFIGW>())
    };
    if config.dwServiceType != SERVICE_WIN32_OWN_PROCESS
        || config.dwStartType != SERVICE_DEMAND_START
        || config.lpBinaryPathName.is_null()
        || config.lpServiceStartName.is_null()
        || read_wide(config.lpBinaryPathName) != expected
        || !read_wide(config.lpServiceStartName).eq_ignore_ascii_case("LocalSystem")
    {
        return Err(HelperError::Authorization(
            "An existing Windows helper service has an unexpected executable configuration".into(),
        ));
    }
    Ok(())
}

fn installed_helper_path() -> Result<PathBuf> {
    let current = std::env::current_exe()
        .map_err(|error| HelperError::transport("locate elevated helper executable", error))?;
    validate_helper_path(&current)?;
    let current = current
        .canonicalize()
        .map_err(|error| HelperError::transport("canonicalize helper executable", error))?;
    ensure_in_program_files(&current)?;
    Ok(current)
}

fn deployed_helper_for(app: &Path) -> Result<PathBuf> {
    let parent = app.parent().ok_or_else(|| {
        HelperError::Validation("Network Lantern executable has no deployment directory".into())
    })?;
    let helper = parent.join(HELPER_FILE_NAME);
    validate_helper_path(&helper)?;
    let helper = helper.canonicalize().map_err(|error| {
        HelperError::transport("canonicalize deployed helper executable", error)
    })?;
    ensure_in_program_files(&helper)?;
    Ok(helper)
}

fn validate_helper_path(path: &Path) -> Result<()> {
    if path
        .file_name()
        .and_then(OsStr::to_str)
        .is_none_or(|name| !name.eq_ignore_ascii_case(HELPER_FILE_NAME))
    {
        return Err(HelperError::Authorization(
            "Windows helper executable name is not trusted".into(),
        ));
    }
    validate_regular_file(path)
}

fn service_command(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or_else(|| {
        HelperError::Validation("Windows helper executable path is not valid Unicode".into())
    })?;
    if path.contains('"') || path.starts_with(r"\\") {
        return Err(HelperError::Authorization(
            "Windows helper executable path is not a trusted local path".into(),
        ));
    }
    Ok(format!("\"{path}\" {SERVICE_ARGUMENT}"))
}

fn ensure_in_program_files(path: &Path) -> Result<()> {
    let root = program_files()?.canonicalize().map_err(|error| {
        HelperError::transport("canonicalize Windows Program Files directory", error)
    })?;
    if !path.starts_with(&root) || path.parent() == Some(root.as_path()) {
        return Err(HelperError::Authorization(
            "Windows helper must be inside an application directory under Program Files".into(),
        ));
    }
    Ok(())
}

fn ensure_deployment_not_user_writable(helper: &Path) -> Result<()> {
    let directory = helper.parent().ok_or_else(|| {
        HelperError::Authorization("Windows helper has no trusted deployment directory".into())
    })?;
    validate_regular_directory(directory)?;
    deny_each_access(
        directory,
        &[
            FILE_ADD_FILE,
            FILE_ADD_SUBDIRECTORY,
            FILE_DELETE_CHILD,
            DELETE,
            WRITE_DAC,
            WRITE_OWNER,
        ],
        FILE_FLAG_BACKUP_SEMANTICS,
        "Windows helper deployment directory is writable by the application user",
    )
}

fn ensure_file_not_user_writable(helper: &Path) -> Result<()> {
    deny_each_access(
        helper,
        &[
            FILE_WRITE_DATA,
            FILE_APPEND_DATA,
            DELETE,
            WRITE_DAC,
            WRITE_OWNER,
        ],
        0,
        "Windows helper executable is writable by the application user",
    )
}

fn deny_each_access(
    path: &Path,
    access_rights: &[u32],
    flags: u32,
    writable_message: &'static str,
) -> Result<()> {
    let wide_path = wide_os(path.as_os_str());
    for access in access_rights {
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                *access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                flags,
                null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(handle) };
            return Err(HelperError::Authorization(writable_message.into()));
        }
        if unsafe { GetLastError() } != ERROR_ACCESS_DENIED {
            return Err(last("verify Windows helper deployment access"));
        }
    }
    Ok(())
}

fn program_files() -> Result<PathBuf> {
    let mut buffer = [0_u16; MAX_PATH as usize];
    let status = unsafe {
        SHGetFolderPathW(
            null_mut(),
            CSIDL_PROGRAM_FILES as i32,
            null_mut(),
            SHGFP_TYPE_CURRENT as u32,
            buffer.as_mut_ptr(),
        )
    };
    if status < 0 {
        return Err(HelperError::Unavailable(format!(
            "locate Windows Program Files directory: HRESULT 0x{:08X}",
            status as u32
        )));
    }
    let length = buffer.iter().position(|unit| *unit == 0).ok_or_else(|| {
        HelperError::Protocol("Windows Program Files path was not terminated".into())
    })?;
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

fn verify_authenticode(path: &Path) -> Result<()> {
    let path = wide_os(path.as_os_str());
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path.as_ptr(),
        hFile: null_mut(),
        pgKnownSubject: null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: null_mut(),
        pSIPClientData: null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: null_mut(),
        pwszURLReference: null_mut(),
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        dwUIContext: 0,
        pSignatureSettings: null_mut(),
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe {
        WinVerifyTrust(
            null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    if status != 0 {
        return Err(HelperError::Authorization(format!(
            "Windows helper Authenticode verification failed (0x{:08X})",
            status as u32
        )));
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    wide_os(OsStr::new(value))
}

fn wide_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn read_wide(value: *const u16) -> String {
    let length = unsafe { (0..).position(|index| *value.add(index) == 0).unwrap_or(0) };
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value, length) })
}

fn last(operation: &'static str) -> HelperError {
    HelperError::transport(operation, std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_command_quotes_only_the_fixed_local_binary() {
        assert_eq!(
            service_command(Path::new(
                r"C:\Program Files\Network Lantern\network-lantern-helper.exe"
            ))
            .unwrap(),
            r#""C:\Program Files\Network Lantern\network-lantern-helper.exe" --service"#
        );
        assert!(service_command(Path::new(r"\\server\share\network-lantern-helper.exe")).is_err());
        assert!(service_command(Path::new("C:\\bad\"path\\network-lantern-helper.exe")).is_err());
    }

    #[test]
    fn deployment_path_is_always_a_sibling_with_a_fixed_name() {
        let path = Path::new(r"C:\Program Files\Network Lantern\network-lantern.exe");
        let candidate = path.parent().unwrap().join(HELPER_FILE_NAME);
        assert_eq!(
            candidate,
            PathBuf::from(r"C:\Program Files\Network Lantern\network-lantern-helper.exe")
        );
    }
}
