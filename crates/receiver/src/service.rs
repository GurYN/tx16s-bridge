//! Windows service registration and entry point.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use tracing::error;
use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use crate::config::{self, Config};

const NAME: &str = "tx16s-receiver";
const DISPLAY_NAME: &str = "TX16S Receiver";
const DESCRIPTION: &str =
    "Recoit la radio TX16S envoyee par le Mac sur le reseau local et pilote un joystick vJoy.";
const FIREWALL_RULE: &str = "tx16s-receiver";
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;

static CONFIG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main);

/// Entry point when started by the Service Control Manager.
pub fn dispatch(config_path: Option<PathBuf>) -> Result<()> {
    CONFIG_PATH.set(config_path).ok();
    service_dispatcher::start(NAME, ffi_service_main)
        .context("ce mode est reserve au gestionnaire de services Windows")
}

fn service_main(_arguments: Vec<OsString>) {
    let _guard = crate::logging::file(&config::data_dir().join("logs")).ok();
    if let Err(error) = run_service() {
        error!("{error:#}");
    }
}

fn run_service() -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = stop.clone();
    let status = service_control_handler::register(NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            handler_stop.store(true, Ordering::Relaxed);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    let report = |state, exit_code| {
        status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
            } else {
                ServiceControlAccept::empty()
            },
            exit_code,
            checkpoint: 0,
            wait_hint: Duration::from_secs(2),
            process_id: None,
        })
    };
    report(ServiceState::Running, ServiceExitCode::Win32(0))?;

    let config_path = CONFIG_PATH.get().cloned().flatten();
    let result = Config::load(config_path.as_deref()).and_then(|config| {
        let mut output = crate::make_output(&config, false)?;
        crate::server::run(&config, output.as_mut(), &stop)
    });

    let exit_code = match &result {
        Ok(()) => ServiceExitCode::Win32(0),
        Err(_) => ServiceExitCode::ServiceSpecific(1),
    };
    report(ServiceState::Stopped, exit_code)?;
    result
}

fn admin_hint(error: windows_service::Error) -> anyhow::Error {
    match &error {
        windows_service::Error::Winapi(io) if io.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
            anyhow!("acces refuse : lance cette commande dans un terminal administrateur")
        }
        _ => error.into(),
    }
}

fn is_missing(error: &windows_service::Error) -> bool {
    matches!(error, windows_service::Error::Winapi(io)
        if io.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST))
}

fn manager(access: ServiceManagerAccess) -> Result<ServiceManager> {
    ServiceManager::local_computer(None::<&str>, access).map_err(admin_hint)
}

/// Copies the executable to Program Files so the service survives the download folder being cleaned.
fn install_executable() -> Result<PathBuf> {
    let current = std::env::current_exe()?;
    let directory = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join("tx16s-bridge");
    let target = directory.join("tx16s-receiver.exe");
    let same = current.canonicalize().ok() == target.canonicalize().ok();
    if !same {
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("creation de {} impossible", directory.display()))?;
        std::fs::copy(&current, &target)
            .with_context(|| format!("copie vers {} impossible", target.display()))?;
    }
    Ok(target)
}

fn write_default_config(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, config::DEFAULT_FILE)
        .with_context(|| format!("ecriture de {} impossible", path.display()))
}

fn netsh(arguments: &[&str]) -> bool {
    Command::new("netsh")
        .args(["advfirewall", "firewall"])
        .args(arguments)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn remove_firewall_rule() -> bool {
    netsh(&["delete", "rule", &format!("name={FIREWALL_RULE}")])
}

pub fn install(config_path: Option<PathBuf>) -> Result<()> {
    let manager = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    if manager
        .open_service(NAME, ServiceAccess::QUERY_STATUS)
        .is_ok()
    {
        bail!("le service est deja installe : lance d'abord 'tx16s-receiver uninstall'");
    }

    let executable = install_executable()?;
    let config_file = config_path.clone().or_else(config::default_path).unwrap();
    write_default_config(&config_file)?;
    Config::load(Some(&config_file))?;

    let mut launch_arguments = Vec::new();
    if let Some(path) = &config_path {
        launch_arguments.push(OsString::from("--config"));
        launch_arguments.push(path.canonicalize()?.into_os_string());
    }
    launch_arguments.push(OsString::from("service"));
    let info = ServiceInfo {
        name: OsString::from(NAME),
        display_name: OsString::from(DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: executable.clone(),
        launch_arguments,
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };
    let service = manager
        .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
        .map_err(admin_hint)?;
    service.set_description(DESCRIPTION)?;

    remove_firewall_rule();
    let firewall = netsh(&[
        "add",
        "rule",
        &format!("name={FIREWALL_RULE}"),
        "dir=in",
        "action=allow",
        "protocol=UDP",
        "profile=private",
        &format!("program={}", executable.display()),
    ]);

    service
        .start(&[] as &[&OsStr])
        .context("le service est installe mais n'a pas demarre")?;

    println!("Service installe et demarre : {}", executable.display());
    println!("Configuration : {}", config_file.display());
    println!("Logs : {}", config::data_dir().join("logs").display());
    if firewall {
        println!("Pare-feu : UDP autorise sur les reseaux prives.");
        println!("  Si ton reseau est classe 'Public' dans Windows, passe-le en 'Prive'.");
    } else {
        println!("Pare-feu : la regle n'a pas pu etre ajoutee, autorise UDP a la main.");
    }
    Ok(())
}

fn wait_for(service: &windows_service::service::Service, state: ServiceState) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while service.query_status()?.current_state != state {
        if Instant::now() > deadline {
            bail!("le service n'a pas atteint l'etat {state:?} en 10 s");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    let access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
    match manager.open_service(NAME, access) {
        Ok(service) => {
            if service.query_status()?.current_state != ServiceState::Stopped {
                service.stop().map_err(admin_hint)?;
                wait_for(&service, ServiceState::Stopped)?;
            }
            service.delete().map_err(admin_hint)?;
            println!("Service supprime.");
        }
        Err(error) if is_missing(&error) => println!("Le service n'etait pas installe."),
        Err(error) => return Err(admin_hint(error)),
    }
    if remove_firewall_rule() {
        println!("Regle de pare-feu supprimee.");
    }
    println!(
        "La configuration et les logs restent dans {}.",
        config::data_dir().display()
    );
    Ok(())
}

fn open(access: ServiceAccess) -> Result<windows_service::service::Service> {
    manager(ServiceManagerAccess::CONNECT)?
        .open_service(NAME, access)
        .map_err(|error| {
            if is_missing(&error) {
                anyhow!("le service n'est pas installe : lance 'tx16s-receiver install'")
            } else {
                admin_hint(error)
            }
        })
}

pub fn start() -> Result<()> {
    let service = open(ServiceAccess::START | ServiceAccess::QUERY_STATUS)?;
    service.start(&[] as &[&OsStr]).map_err(admin_hint)?;
    wait_for(&service, ServiceState::Running)?;
    println!("Service demarre.");
    Ok(())
}

pub fn stop() -> Result<()> {
    let service = open(ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?;
    if service.query_status()?.current_state != ServiceState::Stopped {
        service.stop().map_err(admin_hint)?;
        wait_for(&service, ServiceState::Stopped)?;
    }
    println!("Service arrete.");
    Ok(())
}

pub fn status() -> Result<()> {
    let service = open(ServiceAccess::QUERY_STATUS)?;
    let state = match service.query_status()?.current_state {
        ServiceState::Running => "en marche",
        ServiceState::Stopped => "arrete",
        ServiceState::StartPending => "en cours de demarrage",
        ServiceState::StopPending => "en cours d'arret",
        _ => "en pause",
    };
    println!("Service : {state}");
    println!(
        "Configuration : {}",
        config::default_path().unwrap().display()
    );
    println!("Logs : {}", config::data_dir().join("logs").display());
    Ok(())
}
