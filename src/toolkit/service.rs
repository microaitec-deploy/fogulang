//! Standalone executables as system services (`fogu build --compile --service`).
//!
//! The service settings travel in the bundle (`"service": { name, displayName,
//! description, startType, args, stopTimeout, log }`), from `fogu.json`
//! `"service"` and the `--service*` flags of `fogu build`. An executable whose
//! bundle has them takes these commands as its first argument:
//!
//! ```text
//! app install [--start auto|manual|delayed] [--user <account> [--password <p>]] [--log <file>] [-- <program args>]
//! app uninstall | app start | app stop | app status
//! app -- <args>          the program itself, with <args> (even "install")
//! ```
//!
//! Anything else runs the program, as before (a console run, for debugging).
//!
//! Windows: `install` registers the executable with the service manager (SCM),
//! with the command line `"<app.exe>" service-run [--log <file>] [args]`; the SCM
//! starts it, `service-run` connects to the SCM (`StartServiceCtrlDispatcher`)
//! and runs the program on the service thread. stdout / stderr go to a log file
//! (`<name>.log` next to the executable by default), the current folder is the
//! executable's folder. A stop or shutdown from the SCM raises `service-stop` and
//! `SIGTERM` (`Fogu.signal`) — the ones the program listens to; it has
//! `stopTimeout` seconds (default 20) to end, then it is ended. A program that
//! listens to neither is ended right away. The exit code goes to the SCM with
//! SERVICE_STOPPED (non-zero: a service-specific error, so recovery actions apply).
//!
//! Linux: `install` (as root) writes `/etc/systemd/system/<name>.service` and
//! enables it; `start`/`stop`/`status`/`uninstall` use `systemctl`. systemd sends
//! SIGTERM on stop. `fogu build --service` also writes that unit next to the binary.

use serde_json::{json, Value as Json};

/// How the service starts with the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartType {
    Auto,
    Manual,
    /// automatic, a little after the boot (Windows "Automatic (Delayed Start)")
    Delayed,
}

impl StartType {
    pub fn parse(s: &str) -> Result<StartType, String> {
        match s.to_ascii_lowercase().as_str() {
            "auto" | "automatic" => Ok(StartType::Auto),
            "manual" | "demand" => Ok(StartType::Manual),
            "delayed" | "delayed-auto" => Ok(StartType::Delayed),
            other => Err(format!("unknown service start type '{}' (auto, manual or delayed)", other)),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            StartType::Auto => "auto",
            StartType::Manual => "manual",
            StartType::Delayed => "delayed",
        }
    }
}

/// The service settings of a bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceSpec {
    /// the service (key) name: `sc query <name>`, `systemctl status <name>`
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub start_type: StartType,
    /// arguments given to the program when the service manager starts it
    pub args: Vec<String>,
    /// seconds the program has to end after a stop request
    pub stop_timeout: u64,
    /// the log file (relative: to the executable's folder); default `<name>.log`
    pub log: Option<String>,
}

pub const DEFAULT_STOP_TIMEOUT: u64 = 20;

/// Checks a service name: letters, digits, `-`, `_`, `.` (no spaces or slashes).
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 80 {
        return Err("the service name must have 1 to 80 characters".into());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(format!("invalid service name '{}': use letters, digits, '-', '_' and '.'", name));
    }
    Ok(())
}

impl ServiceSpec {
    /// The default settings for an executable called `name`.
    pub fn new(name: &str) -> ServiceSpec {
        ServiceSpec {
            name: name.to_string(),
            display_name: name.to_string(),
            description: String::new(),
            start_type: StartType::Auto,
            args: Vec::new(),
            stop_timeout: DEFAULT_STOP_TIMEOUT,
            log: None,
        }
    }

    /// Reads `"service"` (of fogu.json or of a bundle): `true` = the defaults, an
    /// object = settings over the defaults, `false` / null / absent = no service.
    pub fn from_json(j: &Json, default_name: &str) -> Result<Option<ServiceSpec>, String> {
        let o = match j {
            Json::Null | Json::Bool(false) => return Ok(None),
            Json::Bool(true) => return Ok(Some(ServiceSpec::new(default_name))),
            Json::Object(o) => o,
            _ => return Err("\"service\" must be true or an object { name, displayName, description, startType, args, stopTimeout, log }".into()),
        };
        let mut s = ServiceSpec::new(default_name);
        let text = |k: &str| -> Result<Option<String>, String> {
            match o.get(k) {
                None | Some(Json::Null) => Ok(None),
                Some(Json::String(v)) => Ok(Some(v.clone())),
                Some(_) => Err(format!("service.{} must be a string", k)),
            }
        };
        for k in o.keys() {
            if !["name", "displayName", "description", "startType", "args", "stopTimeout", "log"].contains(&k.as_str()) {
                return Err(format!("unknown service setting '{}' (name, displayName, description, startType, args, stopTimeout, log)", k));
            }
        }
        if let Some(n) = text("name")? {
            s.display_name = n.clone();
            s.name = n;
        }
        if let Some(d) = text("displayName")? {
            s.display_name = d;
        }
        if let Some(d) = text("description")? {
            s.description = d;
        }
        if let Some(t) = text("startType")? {
            s.start_type = StartType::parse(&t)?;
        }
        if let Some(l) = text("log")? {
            s.log = Some(l);
        }
        match o.get("args") {
            None | Some(Json::Null) => {}
            Some(Json::Array(a)) => {
                s.args = a
                    .iter()
                    .map(|v| v.as_str().map(String::from).ok_or("service.args must be a list of strings".to_string()))
                    .collect::<Result<_, _>>()?
            }
            Some(_) => return Err("service.args must be a list of strings".into()),
        }
        match o.get("stopTimeout") {
            None | Some(Json::Null) => {}
            Some(v) => match v.as_u64() {
                Some(n) if (1..=3600).contains(&n) => s.stop_timeout = n,
                _ => return Err("service.stopTimeout must be a number of seconds (1 to 3600)".into()),
            },
        }
        check_name(&s.name)?;
        Ok(Some(s))
    }

    pub fn to_json(&self) -> Json {
        let mut j = json!({
            "name": self.name,
            "displayName": self.display_name,
            "description": self.description,
            "startType": self.start_type.name(),
            "args": self.args,
            "stopTimeout": self.stop_timeout,
        });
        if let Some(l) = &self.log {
            j["log"] = Json::String(l.clone());
        }
        j
    }

    /// A systemd unit running `exec` (the absolute path of the executable).
    pub fn systemd_unit(&self, exec: &str, workdir: &str) -> String {
        let mut cmd = quote_systemd(exec);
        for a in &self.args {
            cmd.push(' ');
            cmd.push_str(&quote_systemd(a));
        }
        let desc = if self.description.is_empty() { &self.display_name } else { &self.description };
        format!(
            "# Generated by fogu build --service (Fogu {ver})\n\
             [Unit]\n\
             Description={desc}\n\
             After=network-online.target\n\
             Wants=network-online.target\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={cmd}\n\
             WorkingDirectory={workdir}\n\
             Restart=on-failure\n\
             RestartSec=5\n\
             KillSignal=SIGTERM\n\
             TimeoutStopSec={timeout}\n\
             \n\
             [Install]\n\
             WantedBy={wanted}\n",
            ver = super::VERSION,
            desc = desc.replace('\n', " "),
            cmd = cmd,
            workdir = quote_systemd(workdir),
            timeout = self.stop_timeout,
            wanted = if self.start_type == StartType::Manual { "multi-user.target\n# manual start: `systemctl disable` it, or remove [Install]" } else { "multi-user.target" },
        )
    }
}

fn quote_systemd(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| !c.is_whitespace() && !"\"'\\;$%".contains(c)) {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%").replace('$', "$$"))
    }
}

// ---------------------------------------------------------------------
// The commands of a service executable
// ---------------------------------------------------------------------

/// What a service executable was asked to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// run the program (console), with these arguments
    Run(Vec<String>),
    Install(InstallOpts),
    Uninstall,
    Start,
    Stop,
    Status,
    /// started by the Windows service manager
    ServiceRun { log: Option<String>, args: Vec<String> },
    Help,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct InstallOpts {
    pub start_type: Option<StartType>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub log: Option<String>,
    /// program arguments (after `--`); None = the ones of the bundle
    pub args: Option<Vec<String>>,
}

/// Reads the command line of a service executable (without argv[0]).
pub fn parse_command(args: &[String]) -> Result<Command, String> {
    let Some(first) = args.first() else { return Ok(Command::Run(Vec::new())) };
    let rest = &args[1..];
    let no_extra = |c: Command| {
        if rest.is_empty() {
            Ok(c)
        } else {
            Err(format!("'{}' takes no arguments", first))
        }
    };
    match first.as_str() {
        "--" => Ok(Command::Run(rest.to_vec())),
        "install" => parse_install(rest).map(Command::Install),
        "uninstall" => no_extra(Command::Uninstall),
        "start" => no_extra(Command::Start),
        "stop" => no_extra(Command::Stop),
        "status" => no_extra(Command::Status),
        "service-help" => Ok(Command::Help),
        "service-run" => {
            let mut log = None;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--log" => {
                        log = Some(rest.get(i + 1).ok_or("--log expects a file")?.clone());
                        i += 2;
                    }
                    "--" => {
                        i += 1;
                        break;
                    }
                    _ => break,
                }
            }
            Ok(Command::ServiceRun { log, args: rest[i..].to_vec() })
        }
        _ => Ok(Command::Run(args.to_vec())),
    }
}

fn parse_install(args: &[String]) -> Result<InstallOpts, String> {
    let mut o = InstallOpts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{} expects {}", a, what));
        match a.as_str() {
            "--start" | "--start-type" => o.start_type = Some(StartType::parse(&value("auto, manual or delayed")?)?),
            "--user" => o.user = Some(value("an account (e.g. \"NT AUTHORITY\\LocalService\" or .\\name)")?),
            "--password" => o.password = Some(value("the account's password")?),
            "--log" => o.log = Some(value("a file")?),
            "--" => {
                o.args = Some(it.cloned().collect());
                break;
            }
            other => return Err(format!("unknown install option '{}' (--start, --user, --password, --log, -- <program args>)", other)),
        }
    }
    if o.password.is_some() && o.user.is_none() {
        return Err("--password needs --user".into());
    }
    Ok(o)
}

fn usage(spec: &ServiceSpec) -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| spec.name.clone());
    format!(
        "{exe}: the program '{name}' as a service\n\n\
         usage:\n  \
         {exe} [args]            run the program here (console)\n  \
         {exe} -- <args>         run it with arguments that look like commands\n  \
         {exe} install [--start auto|manual|delayed] [--user <account> [--password <p>]] [--log <file>] [-- <program args>]\n  \
         {exe} uninstall | start | stop | status\n",
        exe = exe,
        name = spec.name
    )
}

/// Entry point of an executable whose bundle has service settings.
pub fn main(spec: &ServiceSpec, entry: std::path::PathBuf, args: Vec<String>) -> i32 {
    let cmd = match parse_command(&args) {
        Ok(c) => c,
        Err(e) => {
            super::ui::error(&e);
            eprint!("{}", usage(spec));
            return 2;
        }
    };
    let result = match cmd {
        Command::Run(args) => {
            // started by the SCM without `service-run` (registered by hand): a
            // process without a console tries the dispatcher first
            #[cfg(windows)]
            if !win::has_console() {
                if let Some(code) = win::try_dispatch(spec, &entry, None, args.clone()) {
                    return code;
                }
            }
            return run_program(entry, args);
        }
        Command::Help => {
            print!("{}", usage(spec));
            return 0;
        }
        Command::ServiceRun { log, args } => {
            #[cfg(windows)]
            {
                return match win::try_dispatch(spec, &entry, log, args) {
                    Some(code) => code,
                    None => {
                        super::ui::error("'service-run' is used by the Windows service manager; run the executable without it to run the program in this console, or use 'start'");
                        1
                    }
                };
            }
            #[cfg(not(windows))]
            {
                let _ = log;
                return run_program(entry, args);
            }
        }
        Command::Install(o) => os::install(spec, &o),
        Command::Uninstall => os::uninstall(spec),
        Command::Start => os::start(spec),
        Command::Stop => os::stop(spec),
        Command::Status => os::status(spec),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            super::ui::error(&e);
            1
        }
    }
}

fn run_program(entry: std::path::PathBuf, args: Vec<String>) -> i32 {
    let code = super::run_embedded(entry, args);
    crate::native::cleanup();
    code
}

/// The log file of the service: `log` (relative: to the executable's folder)
/// or `<name>.log` next to the executable.
pub fn log_path(spec: &ServiceSpec, log: Option<&str>, exe_dir: &std::path::Path) -> std::path::PathBuf {
    match log.or(spec.log.as_deref()) {
        Some(l) => exe_dir.join(l),
        None => exe_dir.join(format!("{}.log", spec.name)),
    }
}

/// `2026-10-29 14:03:07Z` (UTC), for the log lines of the service runner.
pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}Z", y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

// ---------------------------------------------------------------------
// Windows: the service manager
// ---------------------------------------------------------------------

#[cfg(windows)]
use win as os;

#[cfg(windows)]
mod win {
    use super::{InstallOpts, ServiceSpec, StartType};
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use windows_service::service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode, ServiceInfo, ServiceStartType,
        ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle};
    use windows_service::service_dispatcher;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    /// What the service thread runs (set before the dispatcher starts).
    struct Job {
        spec: ServiceSpec,
        entry: PathBuf,
        log: Option<String>,
        args: Vec<String>,
    }

    static JOB: Mutex<Option<Job>> = Mutex::new(None);
    static STATUS: OnceLock<ServiceStatusHandle> = OnceLock::new();
    static EXIT: AtomicI32 = AtomicI32::new(0);
    static STOPPING: AtomicBool = AtomicBool::new(false);
    static REPORTED: AtomicBool = AtomicBool::new(false);

    windows_service::define_windows_service!(ffi_service_main, service_main);

    pub fn has_console() -> bool {
        unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() != std::ptr::null_mut() }
    }

    /// Runs the program as a service if the SCM started this process; None when
    /// it did not (ERROR_FAILED_SERVICE_CONTROLLER_CONNECT: a console run).
    pub fn try_dispatch(spec: &ServiceSpec, entry: &PathBuf, log: Option<String>, args: Vec<String>) -> Option<i32> {
        *JOB.lock().unwrap_or_else(|e| e.into_inner()) = Some(Job { spec: spec.clone(), entry: entry.clone(), log, args });
        match service_dispatcher::start(&spec.name, ffi_service_main) {
            Ok(()) => Some(EXIT.load(Ordering::SeqCst)),
            Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1063) => None,
            Err(e) => {
                super::super::ui::error(&format!("service dispatcher: {}", e));
                Some(1)
            }
        }
    }

    fn svc_status(state: ServiceState, code: i32, wait: Duration) -> ServiceStatus {
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running { ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN } else { ServiceControlAccept::empty() },
            exit_code: if code == 0 { ServiceExitCode::Win32(0) } else { ServiceExitCode::ServiceSpecific(code as u32) },
            checkpoint: 0,
            wait_hint: wait,
            process_id: None,
        }
    }

    /// Reports SERVICE_STOPPED (once).
    fn report_stopped(code: i32) {
        if REPORTED.swap(true, Ordering::SeqCst) {
            return;
        }
        log_line(&format!("stopped (exit code {})", code));
        if let Some(h) = STATUS.get() {
            let _ = h.set_service_status(svc_status(ServiceState::Stopped, code, Duration::ZERO));
        }
    }

    fn log_line(msg: &str) {
        eprintln!("[{}] fogu service: {}", super::timestamp(), msg);
    }

    /// stdout and stderr of the process go to `path` (a service has none).
    fn redirect_output(path: &std::path::Path) -> Result<(), String> {
        use std::os::windows::io::IntoRawHandle;
        use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let f2 = f.try_clone().map_err(|e| e.to_string())?;
        // the handles stay open for the life of the process
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, f.into_raw_handle() as _);
            SetStdHandle(STD_ERROR_HANDLE, f2.into_raw_handle() as _);
        }
        Ok(())
    }

    fn service_main(_args: Vec<OsString>) {
        let Some(job) = JOB.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        let exe = std::env::current_exe().unwrap_or_default();
        let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let log = super::log_path(&job.spec, job.log.as_deref(), &dir);
        let _ = redirect_output(&log);
        // relative paths (data files, .env) are the executable's folder, not System32
        let _ = std::env::set_current_dir(&dir);
        super::super::load_dotenv();

        let grace = Duration::from_secs(job.spec.stop_timeout);
        let handler = move |control| match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                if !STOPPING.swap(true, Ordering::SeqCst) {
                    if let Some(h) = STATUS.get() {
                        let _ = h.set_service_status(svc_status(ServiceState::StopPending, 0, grace + Duration::from_secs(5)));
                    }
                    log_line(&format!("stop requested ({:?})", control));
                    if crate::native::signal::raise_stop() {
                        // the program handles it: give it the grace period
                        std::thread::spawn(move || {
                            std::thread::sleep(grace);
                            log_line(&format!("the program did not end in {}s: ending it", grace.as_secs()));
                            crate::native::exit(1);
                        });
                    } else {
                        std::thread::spawn(|| crate::native::exit(0));
                    }
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let handle = match service_control_handler::register(&job.spec.name, handler) {
            Ok(h) => h,
            Err(e) => {
                log_line(&format!("cannot register the control handler: {}", e));
                EXIT.store(1, Ordering::SeqCst);
                return;
            }
        };
        let _ = STATUS.set(handle);
        // `exit()` in the program (or the end after a stop) reports SERVICE_STOPPED
        crate::native::on_exit(report_stopped);
        let _ = handle.set_service_status(svc_status(ServiceState::Running, 0, Duration::ZERO));
        log_line(&format!("started '{}' (pid {}, log {})", job.spec.name, std::process::id(), log.display()));
        let code = super::super::run_embedded(job.entry, job.args);
        crate::native::cleanup();
        EXIT.store(code, Ordering::SeqCst);
        report_stopped(code);
    }

    fn manager(write: bool) -> Result<ServiceManager, String> {
        let access = if write { ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE } else { ServiceManagerAccess::CONNECT };
        ServiceManager::local_computer(None::<&str>, access).map_err(|e| explain("cannot open the service manager", e))
    }

    fn explain(what: &str, e: windows_service::Error) -> String {
        match &e {
            windows_service::Error::Winapi(io) if io.raw_os_error() == Some(5) => {
                format!("{}: access denied — run this from an Administrator terminal", what)
            }
            windows_service::Error::Winapi(io) if io.raw_os_error() == Some(1060) => format!("{}: the service is not installed", what),
            windows_service::Error::Winapi(io) if io.raw_os_error() == Some(1073) => format!("{}: the service is already installed (uninstall it first)", what),
            windows_service::Error::Winapi(io) if io.raw_os_error() == Some(1072) => {
                format!("{}: the service is marked for deletion (close the Services window / wait, then retry)", what)
            }
            _ => format!("{}: {}", what, e),
        }
    }

    pub fn install(spec: &ServiceSpec, o: &InstallOpts) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let start = o.start_type.unwrap_or(spec.start_type);
        let mut launch: Vec<OsString> = vec!["service-run".into()];
        if let Some(l) = &o.log {
            // relative to where `install` runs, not to the executable
            let p = std::env::current_dir().map(|d| d.join(l)).unwrap_or_else(|_| l.into());
            launch.push("--log".into());
            launch.push(p.into_os_string());
        }
        launch.push("--".into());
        launch.extend(o.args.clone().unwrap_or_else(|| spec.args.clone()).into_iter().map(OsString::from));
        let info = ServiceInfo {
            name: spec.name.clone().into(),
            display_name: spec.display_name.clone().into(),
            service_type: ServiceType::OWN_PROCESS,
            start_type: if start == StartType::Manual { ServiceStartType::OnDemand } else { ServiceStartType::AutoStart },
            error_control: ServiceErrorControl::Normal,
            executable_path: exe.clone(),
            launch_arguments: launch,
            dependencies: vec![],
            account_name: o.user.clone().map(Into::into),
            account_password: o.password.clone().map(Into::into),
        };
        let m = manager(true)?;
        let svc = m
            .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::QUERY_STATUS)
            .map_err(|e| explain("cannot install the service", e))?;
        if !spec.description.is_empty() {
            svc.set_description(&spec.description).map_err(|e| explain("cannot set the description", e))?;
        }
        if start == StartType::Delayed {
            svc.set_delayed_auto_start(true).map_err(|e| explain("cannot set the delayed start", e))?;
        }
        println!(
            "installed service '{}' ({}, start: {}) → {}\n  start it: {} start   (or: sc start {})",
            spec.name,
            spec.display_name,
            start.name(),
            exe.display(),
            exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            spec.name
        );
        Ok(())
    }

    fn open(spec: &ServiceSpec, access: ServiceAccess) -> Result<windows_service::service::Service, String> {
        manager(false)?.open_service(&spec.name, access).map_err(|e| explain(&format!("service '{}'", spec.name), e))
    }

    fn wait_state(svc: &windows_service::service::Service, want: ServiceState, secs: u64) -> Result<ServiceStatus, String> {
        let until = std::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let s = svc.query_status().map_err(|e| explain("cannot query the service", e))?;
            if s.current_state == want || std::time::Instant::now() >= until {
                return Ok(s);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub fn uninstall(spec: &ServiceSpec) -> Result<(), String> {
        let svc = open(spec, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE)?;
        if svc.query_status().is_ok_and(|s| s.current_state != ServiceState::Stopped) {
            let _ = svc.stop();
            wait_state(&svc, ServiceState::Stopped, spec.stop_timeout + 10)?;
        }
        svc.delete().map_err(|e| explain("cannot uninstall the service", e))?;
        println!("uninstalled service '{}'", spec.name);
        Ok(())
    }

    pub fn start(spec: &ServiceSpec) -> Result<(), String> {
        let svc = open(spec, ServiceAccess::START | ServiceAccess::QUERY_STATUS)?;
        svc.start::<&str>(&[]).map_err(|e| explain("cannot start the service", e))?;
        let s = wait_state(&svc, ServiceState::Running, 30)?;
        if s.current_state != ServiceState::Running {
            return Err(format!("service '{}' is {:?} (see its log)", spec.name, s.current_state));
        }
        println!("service '{}' is running (pid {})", spec.name, s.process_id.unwrap_or(0));
        Ok(())
    }

    pub fn stop(spec: &ServiceSpec) -> Result<(), String> {
        let svc = open(spec, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?;
        svc.stop().map_err(|e| explain("cannot stop the service", e))?;
        let s = wait_state(&svc, ServiceState::Stopped, spec.stop_timeout + 10)?;
        if s.current_state != ServiceState::Stopped {
            return Err(format!("service '{}' is still {:?}", spec.name, s.current_state));
        }
        println!("service '{}' stopped ({})", spec.name, exit_text(&s.exit_code));
        Ok(())
    }

    fn exit_text(c: &ServiceExitCode) -> String {
        match c {
            ServiceExitCode::Win32(0) => "exit code 0".into(),
            ServiceExitCode::Win32(n) => format!("Windows error {}", n),
            ServiceExitCode::ServiceSpecific(n) => format!("exit code {}", n),
        }
    }

    pub fn status(spec: &ServiceSpec) -> Result<(), String> {
        let svc = open(spec, ServiceAccess::QUERY_STATUS)?;
        let s = svc.query_status().map_err(|e| explain("cannot query the service", e))?;
        let state = format!("{:?}", s.current_state).to_lowercase();
        match s.process_id {
            Some(pid) => println!("service '{}': {} (pid {})", spec.name, state, pid),
            None if s.current_state == ServiceState::Stopped => println!("service '{}': {} ({})", spec.name, state, exit_text(&s.exit_code)),
            None => println!("service '{}': {}", spec.name, state),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------
// Linux: systemd
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
use linux as os;

#[cfg(target_os = "linux")]
mod linux {
    use super::{InstallOpts, ServiceSpec, StartType};
    use std::process::Command;

    fn unit_path(spec: &ServiceSpec) -> std::path::PathBuf {
        std::path::Path::new("/etc/systemd/system").join(format!("{}.service", spec.name))
    }

    fn systemctl(args: &[&str]) -> Result<(), String> {
        let st = Command::new("systemctl").args(args).status().map_err(|e| format!("systemctl: {}", e))?;
        if st.success() { Ok(()) } else { Err(format!("systemctl {} failed ({})", args.join(" "), st)) }
    }

    pub fn install(spec: &ServiceSpec, o: &InstallOpts) -> Result<(), String> {
        if o.password.is_some() {
            return Err("--password is for Windows accounts; systemd runs the service as --user without one".into());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_else(|| "/".into());
        let mut s = spec.clone();
        if let Some(t) = o.start_type {
            s.start_type = t;
        }
        if let Some(a) = &o.args {
            s.args = a.clone();
        }
        let mut unit = s.systemd_unit(&exe.display().to_string(), &dir);
        if let Some(u) = &o.user {
            unit = unit.replace("[Service]\n", &format!("[Service]\nUser={}\n", u));
        }
        if let Some(l) = &o.log {
            let p = std::env::current_dir().map(|d| d.join(l)).unwrap_or_else(|_| l.into());
            unit = unit.replace("[Service]\n", &format!("[Service]\nStandardOutput=append:{0}\nStandardError=append:{0}\n", p.display()));
        }
        let path = unit_path(spec);
        std::fs::write(&path, unit).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => format!("{}: permission denied — run it as root (sudo)", path.display()),
            _ => format!("{}: {}", path.display(), e),
        })?;
        systemctl(&["daemon-reload"])?;
        if s.start_type != StartType::Manual {
            systemctl(&["enable", &spec.name])?;
        }
        println!("installed {} → {}\n  start it: sudo {} start   (or: systemctl start {})", path.display(), exe.display(), exe.display(), spec.name);
        Ok(())
    }

    pub fn uninstall(spec: &ServiceSpec) -> Result<(), String> {
        let path = unit_path(spec);
        if !path.exists() {
            return Err(format!("service '{}' is not installed ({} does not exist)", spec.name, path.display()));
        }
        let _ = systemctl(&["disable", "--now", &spec.name]);
        std::fs::remove_file(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        systemctl(&["daemon-reload"])?;
        println!("uninstalled service '{}'", spec.name);
        Ok(())
    }

    pub fn start(spec: &ServiceSpec) -> Result<(), String> {
        systemctl(&["start", &spec.name])
    }

    pub fn stop(spec: &ServiceSpec) -> Result<(), String> {
        systemctl(&["stop", &spec.name])
    }

    pub fn status(spec: &ServiceSpec) -> Result<(), String> {
        // `status` exits 3 when the service is stopped: still a valid answer
        let _ = Command::new("systemctl").args(["status", "--no-pager", &spec.name]).status().map_err(|e| format!("systemctl: {}", e))?;
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
use other as os;

#[cfg(not(any(windows, target_os = "linux")))]
mod other {
    use super::{InstallOpts, ServiceSpec};
    const MSG: &str = "services are supported on Windows (service manager) and Linux (systemd)";
    pub fn install(_: &ServiceSpec, _: &InstallOpts) -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn uninstall(_: &ServiceSpec) -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn start(_: &ServiceSpec) -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn stop(_: &ServiceSpec) -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn status(_: &ServiceSpec) -> Result<(), String> {
        Err(MSG.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn settings_from_json() {
        assert_eq!(ServiceSpec::from_json(&Json::Null, "app").unwrap(), None);
        assert_eq!(ServiceSpec::from_json(&json!(false), "app").unwrap(), None);
        assert_eq!(ServiceSpec::from_json(&json!(true), "app").unwrap(), Some(ServiceSpec::new("app")));
        let sp = ServiceSpec::from_json(
            &json!({ "name": "myapp", "description": "d", "startType": "delayed", "args": ["--port", "80"], "stopTimeout": 5 }),
            "app",
        )
        .unwrap()
        .unwrap();
        assert_eq!(sp.name, "myapp");
        assert_eq!(sp.display_name, "myapp");
        assert_eq!(sp.start_type, StartType::Delayed);
        assert_eq!(sp.args, s(&["--port", "80"]));
        assert_eq!(sp.stop_timeout, 5);
        // round trip through the bundle
        assert_eq!(ServiceSpec::from_json(&sp.to_json(), "x").unwrap().unwrap(), sp);
        assert!(ServiceSpec::from_json(&json!({ "name": "bad name" }), "app").is_err());
        assert!(ServiceSpec::from_json(&json!({ "startType": "sometimes" }), "app").is_err());
        assert!(ServiceSpec::from_json(&json!({ "nmae": "typo" }), "app").is_err());
        assert!(ServiceSpec::from_json(&json!({ "stopTimeout": 0 }), "app").is_err());
        assert!(ServiceSpec::from_json(&json!("yes"), "app").is_err());
    }

    #[test]
    fn commands() {
        assert_eq!(parse_command(&[]).unwrap(), Command::Run(vec![]));
        assert_eq!(parse_command(&s(&["--port", "8080"])).unwrap(), Command::Run(s(&["--port", "8080"])));
        assert_eq!(parse_command(&s(&["--", "install", "x"])).unwrap(), Command::Run(s(&["install", "x"])));
        assert_eq!(parse_command(&s(&["status"])).unwrap(), Command::Status);
        assert_eq!(parse_command(&s(&["stop"])).unwrap(), Command::Stop);
        assert_eq!(parse_command(&s(&["start"])).unwrap(), Command::Start);
        assert_eq!(parse_command(&s(&["uninstall"])).unwrap(), Command::Uninstall);
        assert!(parse_command(&s(&["stop", "now"])).is_err());
        assert_eq!(parse_command(&s(&["install"])).unwrap(), Command::Install(InstallOpts::default()));
        assert_eq!(
            parse_command(&s(&["install", "--start", "manual", "--user", "NT AUTHORITY\\LocalService", "--log", "x.log", "--", "--port", "1"])).unwrap(),
            Command::Install(InstallOpts {
                start_type: Some(StartType::Manual),
                user: Some("NT AUTHORITY\\LocalService".into()),
                password: None,
                log: Some("x.log".into()),
                args: Some(s(&["--port", "1"])),
            })
        );
        assert!(parse_command(&s(&["install", "--password", "p"])).is_err());
        assert!(parse_command(&s(&["install", "--start"])).is_err());
        assert!(parse_command(&s(&["install", "--bogus"])).is_err());
        assert_eq!(
            parse_command(&s(&["service-run", "--log", "C:\\l.log", "--", "--port", "1"])).unwrap(),
            Command::ServiceRun { log: Some("C:\\l.log".into()), args: s(&["--port", "1"]) }
        );
        assert_eq!(parse_command(&s(&["service-run"])).unwrap(), Command::ServiceRun { log: None, args: vec![] });
    }

    #[test]
    fn systemd_unit() {
        let mut sp = ServiceSpec::new("web");
        sp.description = "The web site".into();
        sp.args = s(&["--port", "80", "two words"]);
        let u = sp.systemd_unit("/opt/web/web", "/opt/web");
        assert!(u.contains("Description=The web site\n"), "{}", u);
        assert!(u.contains("ExecStart=/opt/web/web --port 80 \"two words\"\n"), "{}", u);
        assert!(u.contains("TimeoutStopSec=20\n"), "{}", u);
        assert!(u.contains("WantedBy=multi-user.target"), "{}", u);
    }

    #[test]
    fn log_paths_and_time() {
        let sp = ServiceSpec::new("web");
        let dir = std::path::Path::new("base");
        assert_eq!(log_path(&sp, None, dir), dir.join("web.log"));
        assert_eq!(log_path(&sp, Some("logs/x.log"), dir), dir.join("logs/x.log"));
        let t = timestamp();
        assert_eq!(t.len(), 20, "{}", t);
        assert!(t.ends_with('Z') && t.starts_with("20"), "{}", t);
    }
}
