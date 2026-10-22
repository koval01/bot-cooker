//! Running as a Windows service. The Service Control Manager starts the
//! binary with `--service`; we register with it, report Running, serve, and
//! shut down gracefully on Stop or system shutdown. Install it with
//! `deploy/windows/install-service.ps1`.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Context;
use tokio::sync::oneshot;
use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

use crate::config::Config;
use crate::server;

/// The default service name; the install script may choose another.
pub const SERVICE_NAME: &str = "bot-cooker";

/// What the service needs, handed over before the dispatcher takes the thread.
struct Launch {
    name: String,
    config: Option<PathBuf>,
}

static LAUNCH: OnceLock<Launch> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main);

/// Hand the current thread to the Service Control Manager. Returns when the
/// service has stopped. Fails if the process was not started by the SCM.
pub fn dispatch(name: String, config: Option<PathBuf>) -> anyhow::Result<()> {
    let name = LAUNCH.get_or_init(|| Launch { name, config }).name.clone();
    service_dispatcher::start(name, ffi_service_main)
        .context("connecting to the Service Control Manager (run --service only from a service)")
}

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        tracing::error!("service failed: {e:#}");
    }
}

fn run_service() -> anyhow::Result<()> {
    let launch = LAUNCH.get().context("service launched without parameters")?;
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let mut stop_tx = Some(stop_tx);
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Some(tx) = stop_tx.take() {
                let _ = tx.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status = service_control_handler::register(&launch.name, handler).context("registering the control handler")?;
    let report = |state: ServiceState, code: u32, wait: Duration| {
        let accept = if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        };
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(code),
            checkpoint: 0,
            wait_hint: wait,
            process_id: None,
        });
    };

    report(ServiceState::StartPending, 0, Duration::from_secs(10));
    let result = Config::load(launch.config.as_deref()).and_then(|cfg| {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().context("starting the runtime")?;
        report(ServiceState::Running, 0, Duration::default());
        runtime.block_on(server::run(cfg, async {
            let _ = stop_rx.await;
            tracing::info!("stop requested by the Service Control Manager");
        }))
    });
    // ERROR_SERVICE_SPECIFIC_ERROR-style failure code so `sc query` shows it failed.
    report(ServiceState::Stopped, if result.is_ok() { 0 } else { 1 }, Duration::default());
    result
}
