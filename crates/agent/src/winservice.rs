use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::i18n::tr;
use crate::launcher::{AGENT_IMAGE, agent_path, log_path};
use crate::static_files::Viewer;
use crate::store::{Store, system_clock};
use crate::taskxml::{
    TASK_NAME, TaskConfig, create_arguments, delete_arguments, end_arguments, kill_arguments,
    process_listed, query_arguments, run_arguments, task_xml, tasklist_arguments, utf16_with_bom,
};

const LAUNCHER_IMAGE: &str = "wirehaus-agentd.exe";

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("{}", self.describe())]
    Io(#[from] std::io::Error),
    #[error("{}", self.describe())]
    Scheduler(String),
    #[error("{}", self.describe())]
    NotInitialized(PathBuf),
    #[error("{}", self.describe())]
    MissingLauncher(PathBuf),
    #[error("{}", self.describe())]
    Viewer(#[from] crate::integrity::IntegrityError),
}

impl ServiceError {
    fn describe(&self) -> String {
        match self {
            ServiceError::Io(error) => tr!(
                format!("no se puede preparar el servicio: {error}"),
                format!("cannot set up the service: {error}")
            ),
            ServiceError::Scheduler(text) => tr!(
                format!("el programador de tareas ha fallado: {text}"),
                format!("the task scheduler failed: {text}")
            ),
            ServiceError::NotInitialized(path) => tr!(
                format!(
                    "el agente no está inicializado en {}; ejecuta primero wirehaus-agent init",
                    path.display()
                ),
                format!(
                    "the agent is not initialized in {}; run wirehaus-agent init first",
                    path.display()
                )
            ),
            ServiceError::MissingLauncher(path) => tr!(
                format!("falta {} junto al agente", path.display()),
                format!("{} is missing next to the agent", path.display())
            ),
            ServiceError::Viewer(error) => tr!(
                format!("el visor no es de fiar: {error}"),
                format!("the viewer cannot be trusted: {error}")
            ),
        }
    }
}

fn run(program: &str, arguments: &[String]) -> Result<(bool, String), ServiceError> {
    let output = Command::new(program).args(arguments).output()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((output.status.success(), text.trim().to_owned()))
}

fn schtasks(arguments: &[String]) -> Result<(bool, String), ServiceError> {
    run("schtasks", arguments)
}

fn agent_running() -> Result<bool, ServiceError> {
    let (_, text) = run("tasklist", &tasklist_arguments(AGENT_IMAGE))?;
    Ok(process_listed(&text, AGENT_IMAGE))
}

fn local_log() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| log_path(Path::new(&base)))
}

pub fn install(
    state_dir: &Path,
    viewer_dir: &Path,
    listen: SocketAddr,
    public_hosts: &[String],
    viewer_origins: &[String],
    out: &mut impl Write,
) -> Result<(), ServiceError> {
    let state_dir = std::path::absolute(state_dir)?;
    Store::open(&state_dir, system_clock())
        .map_err(|_| ServiceError::NotInitialized(state_dir.clone()))?;
    let viewer_dir = std::path::absolute(viewer_dir)?;
    Viewer::open(&viewer_dir, None)?;
    let executable = std::env::current_exe()?;
    let launcher = executable
        .parent()
        .map(|dir| dir.join(LAUNCHER_IMAGE))
        .filter(|path| path.exists())
        .ok_or_else(|| ServiceError::MissingLauncher(executable.clone()))?;
    if agent_path(&launcher).is_none_or(|agent| !agent.exists()) {
        return Err(ServiceError::MissingLauncher(executable));
    }
    let working_dir = launcher.parent().map(Path::to_path_buf).unwrap_or_default();
    let config = TaskConfig {
        launcher,
        state_dir,
        viewer_dir,
        listen,
        public_hosts: public_hosts.to_vec(),
        viewer_origins: viewer_origins.to_vec(),
        user_sid: crate::winacl::current_user_sid()?,
        working_dir,
    };
    let xml = std::env::temp_dir().join(format!("wirehaus-task-{}.xml", std::process::id()));
    std::fs::write(&xml, utf16_with_bom(&task_xml(&config)))?;
    let created = schtasks(&create_arguments(&xml));
    let _ = std::fs::remove_file(&xml);
    let (created, text) = created?;
    if !created {
        return Err(ServiceError::Scheduler(text));
    }
    let (started, text) = schtasks(&run_arguments())?;
    if !started {
        return Err(ServiceError::Scheduler(text));
    }
    writeln!(
        out,
        "{}",
        tr!(
            format!("Tarea \"{TASK_NAME}\" instalada."),
            format!("Task \"{TASK_NAME}\" installed.")
        )
    )?;
    if let Some(log) = local_log() {
        writeln!(out, "{}: {}", tr!("Registro", "Log"), log.display())?;
    }
    writeln!(
        out,
        "{}",
        tr!(
            "Arranca sola al iniciar sesión, sin ventana, y se vuelve a lanzar cada minuto si se cae. Solo funciona con la sesión del usuario abierta: es lo que le da acceso a la pantalla.",
            "It starts by itself when you sign in, without a window, and is relaunched every minute if it dies. It only works while the user session is open, which is what gives it access to the screen."
        )
    )?;
    Ok(())
}

pub fn uninstall(out: &mut impl Write) -> Result<(), ServiceError> {
    let (exists, _) = schtasks(&query_arguments())?;
    if exists {
        let _ = schtasks(&end_arguments())?;
        let (deleted, text) = schtasks(&delete_arguments())?;
        if !deleted {
            return Err(ServiceError::Scheduler(text));
        }
    }
    let _ = run("taskkill", &kill_arguments(AGENT_IMAGE, std::process::id()))?;
    let message = if exists {
        tr!(
            format!("Tarea \"{TASK_NAME}\" desinstalada y agente parado."),
            format!("Task \"{TASK_NAME}\" uninstalled and agent stopped.")
        )
    } else {
        tr!(
            format!("La tarea \"{TASK_NAME}\" no estaba instalada."),
            format!("Task \"{TASK_NAME}\" was not installed.")
        )
    };
    writeln!(out, "{message}")?;
    Ok(())
}

pub fn status(out: &mut impl Write) -> Result<(), ServiceError> {
    let (exists, _) = schtasks(&query_arguments())?;
    let message = if !exists {
        tr!(
            format!("La tarea \"{TASK_NAME}\" no está instalada."),
            format!("Task \"{TASK_NAME}\" is not installed.")
        )
    } else if agent_running()? {
        tr!(
            format!("La tarea \"{TASK_NAME}\" está instalada y el agente está en marcha."),
            format!("Task \"{TASK_NAME}\" is installed and the agent is running.")
        )
    } else {
        tr!(
            format!(
                "La tarea \"{TASK_NAME}\" está instalada pero el agente no corre; se lanzará en el próximo minuto si hay una sesión abierta. Mira el registro."
            ),
            format!(
                "Task \"{TASK_NAME}\" is installed but the agent is not running; it will be launched within a minute if a session is open. Check the log."
            )
        )
    };
    writeln!(out, "{message}")?;
    Ok(())
}
