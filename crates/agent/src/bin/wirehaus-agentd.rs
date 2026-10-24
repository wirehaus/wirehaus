#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let code = (|| -> Option<i32> {
        let launcher = std::env::current_exe().ok()?;
        let agent = wirehaus_agent::launcher::agent_path(&launcher)?;
        let local = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from)?;
        let log =
            wirehaus_agent::launcher::open_log(&wirehaus_agent::launcher::log_path(&local)).ok()?;
        let errors = log.try_clone().ok()?;
        let status = Command::new(agent)
            .args(std::env::args_os().skip(1))
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(errors)
            .status()
            .ok()?;
        status.code()
    })();
    std::process::exit(code.unwrap_or(1));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("wirehaus-agentd only runs on Windows");
    std::process::exit(1);
}
