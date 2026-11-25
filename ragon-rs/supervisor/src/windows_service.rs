//! The `ragon-node` Windows service: install, uninstall, start, stop, query, and the service side
//! (`server run --service`, entered through the service control dispatcher).
//!
//! The service runs as the virtual account `NT SERVICE\ragon-node`: not an administrator, no
//! password to keep, its own SID for file permissions (the state directory is granted to it at
//! install). Installing needs an administrator once; the installing user is then granted start,
//! stop and query on the service itself, so day-to-day `server start|stop|status` needs no
//! elevation. A supervisor that dies is restarted by the service manager (failure actions), and
//! the engine dies with it through the Job Object (src/windows_job.rs).

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CALL_NOT_IMPLEMENTED, ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST,
    ERROR_SERVICE_NOT_ACTIVE, ERROR_SERVICE_SPECIFIC_ERROR, HANDLE, LocalFree, NO_ERROR,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetTokenInformation, PSECURITY_DESCRIPTOR, SC_HANDLE, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::System::Services::{
    ChangeServiceConfig2W, CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW,
    OpenServiceW, QueryServiceStatusEx, RegisterServiceCtrlHandlerExW, SC_ACTION, SC_ACTION_RESTART,
    SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE, SC_STATUS_PROCESS_INFO, SERVICE_ACCEPT_SHUTDOWN,
    SERVICE_ACCEPT_STOP, SERVICE_ALL_ACCESS, SERVICE_AUTO_START, SERVICE_CONFIG_DESCRIPTION,
    SERVICE_CONFIG_FAILURE_ACTIONS, SERVICE_CONFIG_SERVICE_SID_INFO, SERVICE_CONTROL_INTERROGATE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_DESCRIPTIONW, SERVICE_ERROR_NORMAL,
    SERVICE_FAILURE_ACTIONSW, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_SID_INFO,
    SERVICE_SID_TYPE_UNRESTRICTED, SERVICE_START, SERVICE_STATUS, SERVICE_STATUS_PROCESS, SERVICE_STOP,
    SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
    SetServiceObjectSecurity, SetServiceStatus, StartServiceCtrlDispatcherW, StartServiceW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const DELETE: u32 = 0x0001_0000;

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn last_error(what: &str) -> io::Error {
    let e = io::Error::last_os_error();
    io::Error::new(e.kind(), format!("{what}: {e}"))
}

/// A service-manager handle, closed on drop.
struct Sc(SC_HANDLE);

impl Drop for Sc {
    fn drop(&mut self) {
        // SAFETY: owned handle.
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn manager(access: u32) -> io::Result<Sc> {
    // SAFETY: null machine and database names mean the local active database.
    let h = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), access) };
    if h == 0 {
        return Err(last_error("OpenSCManager"));
    }
    Ok(Sc(h))
}

/// `Ok(None)` when the service does not exist.
fn open(name: &str, access: u32) -> io::Result<Option<Sc>> {
    let scm = manager(SC_MANAGER_CONNECT)?;
    let wname = wide(name);
    // SAFETY: valid manager handle and NUL-terminated name.
    let h = unsafe { OpenServiceW(scm.0, wname.as_ptr(), access) };
    if h == 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
            return Ok(None);
        }
        return Err(io::Error::new(e.kind(), format!("OpenService {name}: {e}")));
    }
    Ok(Some(Sc(h)))
}

#[derive(Clone, Debug)]
pub struct Query {
    /// `stopped`, `start_pending`, `stop_pending`, `running`, ...
    pub state: &'static str,
    pub pid: u32,
}

fn state_name(state: u32) -> &'static str {
    match state {
        1 => "stopped",
        2 => "start_pending",
        3 => "stop_pending",
        4 => "running",
        5 => "continue_pending",
        6 => "pause_pending",
        7 => "paused",
        _ => "unknown",
    }
}

fn query_handle(service: &Sc) -> io::Result<Query> {
    // SAFETY: the buffer is a SERVICE_STATUS_PROCESS of the size passed.
    unsafe {
        let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
        let mut needed = 0u32;
        let ok = QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        );
        if ok == 0 {
            return Err(last_error("QueryServiceStatusEx"));
        }
        Ok(Query { state: state_name(status.dwCurrentState), pid: status.dwProcessId })
    }
}

/// The service's state and process id; `Ok(None)` when it is not installed.
pub fn query(name: &str) -> io::Result<Option<Query>> {
    match open(name, SERVICE_QUERY_STATUS)? {
        Some(service) => query_handle(&service).map(Some),
        None => Ok(None),
    }
}

pub fn start(name: &str) -> io::Result<()> {
    let service = open(name, SERVICE_START | SERVICE_QUERY_STATUS)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("service {name} is not installed")))?;
    // SAFETY: valid service handle, no arguments.
    if unsafe { StartServiceW(service.0, 0, std::ptr::null()) } == 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(ERROR_SERVICE_ALREADY_RUNNING as i32) {
            return Err(io::Error::new(e.kind(), format!("StartService {name}: {e}")));
        }
    }
    Ok(())
}

fn stop_handle(service: &Sc, timeout: Duration) -> io::Result<()> {
    // SAFETY: valid handle; the status buffer is ours.
    let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut status) } == 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(ERROR_SERVICE_NOT_ACTIVE as i32) {
            return Ok(());
        }
        return Err(io::Error::new(e.kind(), format!("ControlService(stop): {e}")));
    }
    let until = Instant::now() + timeout;
    while Instant::now() < until {
        if query_handle(service)?.state == "stopped" {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "the service did not stop in time"))
}

pub fn stop(name: &str, timeout: Duration) -> io::Result<()> {
    let service = open(name, SERVICE_STOP | SERVICE_QUERY_STATUS)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("service {name} is not installed")))?;
    stop_handle(&service, timeout)
}

/// The SID string (`S-1-5-21-...`) of the user running this process.
fn current_user_sid() -> io::Result<String> {
    // SAFETY: token handle closed below; the buffer is sized by the first call; the string from
    // ConvertSidToStringSidW is freed with LocalFree.
    unsafe {
        let mut token: HANDLE = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(last_error("OpenProcessToken"));
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
        CloseHandle(token);
        if ok == 0 {
            return Err(last_error("GetTokenInformation"));
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(last_error("ConvertSidToStringSid"));
        }
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
        LocalFree(text.cast());
        Ok(sid)
    }
}

/// The service's access list: Windows' default for a new service, plus query for authenticated
/// users (status over a remote shell is not an interactive logon), plus start, stop and query
/// for the operator (the user who installed it).
pub fn service_sddl(operator_sid: &str) -> String {
    format!(
        "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWLOCRRC;;;IU)\
         (A;;CCLCSWLOCRRC;;;SU)(A;;CCLCSWLOCRRC;;;AU)(A;;CCLCSWRPWPLORC;;;{operator_sid})"
    )
}

fn set_security(service: &Sc, sddl: &str) -> io::Result<()> {
    let wsddl = wide(sddl);
    // SAFETY: the descriptor is allocated by the conversion and freed with LocalFree.
    unsafe {
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(wsddl.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut())
            == 0
        {
            return Err(last_error("ConvertStringSecurityDescriptorToSecurityDescriptor"));
        }
        let ok = SetServiceObjectSecurity(service.0, DACL_SECURITY_INFORMATION, sd);
        LocalFree(sd);
        if ok == 0 {
            return Err(last_error("SetServiceObjectSecurity"));
        }
    }
    Ok(())
}

/// Creates the service (auto start, own process, `account` without a password), its description,
/// an unrestricted service SID, failure actions that restart the supervisor after 5, 10 and 30 s,
/// the operator's rights, and the state directory with modify rights for the service's account.
pub fn install(name: &str, account: &str, command_line: &str, state_dir: &Path) -> io::Result<()> {
    let scm = manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    let (wname, wdisplay, wcmd, waccount) =
        (wide(name), wide("Ragon node model server"), wide(command_line), wide(account));
    // SAFETY: all strings are NUL-terminated and outlive the call.
    let h = unsafe {
        CreateServiceW(
            scm.0,
            wname.as_ptr(),
            wdisplay.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            wcmd.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            waccount.as_ptr(),
            std::ptr::null(),
        )
    };
    if h == 0 {
        return Err(last_error(&format!("CreateService {name}")));
    }
    let service = Sc(h);
    let configure = || -> io::Result<()> {
        let mut description = wide(
            "Starts, watches and restarts the Ragon model server (ragon server start|stop|status).",
        );
        let info = SERVICE_DESCRIPTIONW { lpDescription: description.as_mut_ptr() };
        // SAFETY: the structures and the strings they point to outlive each call.
        unsafe {
            if ChangeServiceConfig2W(service.0, SERVICE_CONFIG_DESCRIPTION, (&info as *const SERVICE_DESCRIPTIONW).cast())
                == 0
            {
                return Err(last_error("ChangeServiceConfig2(description)"));
            }
            let sid = SERVICE_SID_INFO { dwServiceSidType: SERVICE_SID_TYPE_UNRESTRICTED };
            if ChangeServiceConfig2W(service.0, SERVICE_CONFIG_SERVICE_SID_INFO, (&sid as *const SERVICE_SID_INFO).cast())
                == 0
            {
                return Err(last_error("ChangeServiceConfig2(sid)"));
            }
            let mut actions = [
                SC_ACTION { Type: SC_ACTION_RESTART, Delay: 5_000 },
                SC_ACTION { Type: SC_ACTION_RESTART, Delay: 10_000 },
                SC_ACTION { Type: SC_ACTION_RESTART, Delay: 30_000 },
            ];
            let failure = SERVICE_FAILURE_ACTIONSW {
                dwResetPeriod: 86_400,
                lpRebootMsg: std::ptr::null_mut(),
                lpCommand: std::ptr::null_mut(),
                cActions: actions.len() as u32,
                lpsaActions: actions.as_mut_ptr(),
            };
            if ChangeServiceConfig2W(
                service.0,
                SERVICE_CONFIG_FAILURE_ACTIONS,
                (&failure as *const SERVICE_FAILURE_ACTIONSW).cast(),
            ) == 0
            {
                return Err(last_error("ChangeServiceConfig2(failure actions)"));
            }
        }
        set_security(&service, &service_sddl(&current_user_sid()?))?;
        std::fs::create_dir_all(state_dir)?;
        let grant = format!("{account}:(OI)(CI)M");
        let out = std::process::Command::new("icacls").arg(state_dir).args(["/grant", &grant]).output()?;
        if !out.status.success() {
            return Err(io::Error::other(format!(
                "icacls {} /grant {grant}: {}",
                state_dir.display(),
                String::from_utf8_lossy(&out.stdout).trim()
            )));
        }
        Ok(())
    };
    if let Err(e) = configure() {
        // SAFETY: valid handle with DELETE access (SERVICE_ALL_ACCESS).
        unsafe { DeleteService(service.0) };
        return Err(e);
    }
    Ok(())
}

/// Stops the service if it runs and deletes it. Not installed is not an error.
pub fn uninstall(name: &str) -> io::Result<()> {
    let Some(service) = open(name, DELETE | SERVICE_STOP | SERVICE_QUERY_STATUS)? else {
        return Ok(());
    };
    if query_handle(&service)?.state != "stopped" {
        stop_handle(&service, Duration::from_secs(30))?;
    }
    // SAFETY: valid handle with DELETE access.
    if unsafe { DeleteService(service.0) } == 0 {
        return Err(last_error("DeleteService"));
    }
    Ok(())
}

type Body = Box<dyn FnOnce(Arc<AtomicBool>) -> i32 + Send>;

static BODY: Mutex<Option<Body>> = Mutex::new(None);
static NAME: OnceLock<Vec<u16>> = OnceLock::new();
static STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static STATUS_HANDLE: AtomicIsize = AtomicIsize::new(0);
static EXIT: AtomicI32 = AtomicI32::new(0);

fn report(state: u32, accepted: u32, exit_code: i32, wait_hint_ms: u32) {
    let handle = STATUS_HANDLE.load(Ordering::SeqCst);
    if handle == 0 {
        return;
    }
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepted,
        dwWin32ExitCode: if exit_code == 0 { NO_ERROR } else { ERROR_SERVICE_SPECIFIC_ERROR },
        dwServiceSpecificExitCode: exit_code as u32,
        dwCheckPoint: 0,
        dwWaitHint: wait_hint_ms,
    };
    // SAFETY: the handle came from RegisterServiceCtrlHandlerExW and is never closed.
    unsafe { SetServiceStatus(handle, &status) };
}

unsafe extern "system" fn control_handler(
    control: u32,
    _event_type: u32,
    _event_data: *mut core::ffi::c_void,
    _context: *mut core::ffi::c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            if let Some(stop) = STOP.get() {
                stop.store(true, Ordering::SeqCst);
            }
            report(SERVICE_STOP_PENDING, 0, 0, 30_000);
            NO_ERROR
        }
        SERVICE_CONTROL_INTERROGATE => NO_ERROR,
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut windows_sys::core::PWSTR) {
    let Some(name) = NAME.get() else { return };
    // SAFETY: the name is a static NUL-terminated string; the handler is a static function.
    let handle = unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control_handler), std::ptr::null()) };
    if handle == 0 {
        return;
    }
    STATUS_HANDLE.store(handle, Ordering::SeqCst);
    report(SERVICE_RUNNING, SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN, 0, 0);
    let body = BODY.lock().ok().and_then(|mut b| b.take());
    let stop = STOP.get().cloned().unwrap_or_default();
    let code = body.map(|body| body(stop)).unwrap_or(1);
    EXIT.store(code, Ordering::SeqCst);
    report(SERVICE_STOPPED, 0, code, 0);
}

/// Hands the process to the service control dispatcher, which calls `body` on the service thread
/// with a flag that the stop and shutdown controls set. Returns `body`'s exit code once the
/// service has stopped; fails (error 1063) when the process was not started by the service manager.
pub fn run_as_service(name: &str, body: Body) -> io::Result<i32> {
    *BODY.lock().map_err(|_| io::Error::other("service body lock poisoned"))? = Some(body);
    let _ = NAME.set(wide(name));
    let _ = STOP.set(Arc::new(AtomicBool::new(false)));
    let name_ptr = NAME.get().map(|n| n.as_ptr() as *mut u16).unwrap_or(std::ptr::null_mut());
    let table = [
        SERVICE_TABLE_ENTRYW { lpServiceName: name_ptr, lpServiceProc: Some(service_main) },
        SERVICE_TABLE_ENTRYW { lpServiceName: std::ptr::null_mut(), lpServiceProc: None },
    ];
    // SAFETY: the table is NUL-terminated and lives until the dispatcher returns.
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        return Err(last_error("StartServiceCtrlDispatcher"));
    }
    Ok(EXIT.load(Ordering::SeqCst))
}
