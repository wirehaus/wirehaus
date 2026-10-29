mod cli;

use std::io::stdout;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use clap::Parser;
use wirehaus_agent::commands;
use wirehaus_agent::{Server, ServerConfig, Store, default_dir, system_clock};
use wirehaus_protocol::PROTOCOL_VERSION;

use wirehaus_agent::state::{ClipboardMode, SecretStorage};

use crate::cli::{
    AudioAction, Cli, ClipboardAction, Command, DeviceAction, Direction, FilesAction,
    SecretsAction, SecretsTarget, TerminalAction, UpdateAction, ViewerAction,
};

fn main() -> ExitCode {
    wirehaus_agent::i18n::init(wirehaus_agent::i18n::Language::from_environment());
    let cli = Cli::parse();
    let dir = cli.state_dir.clone().unwrap_or_else(default_dir);
    match run(cli.command, &dir) {
        Ok(()) if wirehaus_agent::update::restart_requested() => {
            ExitCode::from(wirehaus_agent::update::RESTART_EXIT_CODE)
        }
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("wirehaus-agent: {error}");
            ExitCode::FAILURE
        }
    }
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn initialize(
    dir: &std::path::Path,
    allow_all: bool,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let home = allow_all.then(home_dir).flatten();
    commands::init_with(dir, system_clock(), home.as_deref(), out)?;
    Ok(())
}

fn allow_everything(
    store: &Store,
    code: &str,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let home = home_dir().ok_or("cannot find your home folder")?;
    commands::allow_all(store, &home, code, out)?;
    Ok(())
}

fn run(command: Command, dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut out = stdout().lock();
    let open = || Store::open(dir, system_clock());
    match command {
        Command::Info => {
            println!(
                "wirehaus-agent {} (protocol {})",
                env!("CARGO_PKG_VERSION"),
                PROTOCOL_VERSION
            );
        }
        Command::Init { allow_all } => initialize(dir, allow_all, &mut out)?,
        Command::AllowAll { code } => allow_everything(&open()?, &code, &mut out)?,
        Command::Pairing => {
            commands::pairing_with_link(&open()?, commands::viewer_url(dir).as_deref(), &mut out)?;
        }
        Command::Totp => commands::totp(&open()?, &mut out)?,
        #[cfg(any(target_os = "macos", target_os = "linux", windows))]
        Command::Service { action } => match action {
            crate::cli::ServiceAction::Install {
                viewer_dir,
                listen,
                public_hosts,
                viewer_origins,
            } => {
                wirehaus_agent::Exposure::new(&public_hosts, &viewer_origins)?;
                wirehaus_agent::service::install(
                    dir,
                    &viewer_dir,
                    listen,
                    &public_hosts,
                    &viewer_origins,
                    &mut out,
                )?;
                commands::remember_viewer_url(dir, &public_hosts, &viewer_origins)?;
            }
            crate::cli::ServiceAction::Uninstall => wirehaus_agent::service::uninstall(&mut out)?,
            crate::cli::ServiceAction::Status => wirehaus_agent::service::status(&mut out)?,
        },
        #[cfg(target_os = "macos")]
        Command::Permissions => {
            let screen = wirehaus_capture::screen_capture_allowed()
                || wirehaus_capture::request_screen_capture();
            let input = wirehaus_input::input_allowed() || wirehaus_input::request_input();
            commands::permissions(screen, input, &mut out)?;
        }
        Command::Devices { action: None } => commands::devices(&open()?, &mut out)?,
        Command::Devices {
            action: Some(DeviceAction::Revoke { device, code }),
        } => commands::revoke(&open()?, &device, &code, &mut out)?,
        Command::Devices {
            action: Some(DeviceAction::Reactivate { device, code }),
        } => commands::reactivate(&open()?, &device, &code, &mut out)?,
        Command::Terminal {
            action: TerminalAction::Allow { code },
        } => commands::terminal(&open()?, true, Some(&code), &mut out)?,
        Command::Terminal {
            action: TerminalAction::Deny,
        } => commands::terminal(&open()?, false, None, &mut out)?,
        Command::Audio {
            action: AudioAction::Allow { code },
        } => commands::audio(&open()?, true, Some(&code), &mut out)?,
        Command::Audio {
            action: AudioAction::Deny,
        } => commands::audio(&open()?, false, None, &mut out)?,
        Command::Viewer { action } => run_viewer(action)?,
        Command::Clipboard { action } => run_clipboard(&open()?, action, &mut out)?,
        Command::Files { action } => run_files(&open()?, action, &mut out)?,
        Command::Secrets { action } => run_secrets(&open()?, action, &mut out)?,
        Command::Lock { code } => commands::lock(&open()?, true, &code, &mut out)?,
        Command::Unlock { code } => commands::lock(&open()?, false, &code, &mut out)?,
        Command::Update { action } => run_update(dir, action)?,
        Command::Serve {
            no_auto_update,
            listen,
            display,
            viewer_dir,
            quality,
            idle_minutes,
            tunnel_sleep_minutes,
            viewer_hash,
            public_hosts,
            viewer_origins,
        } => serve(
            dir,
            ServeOptions {
                auto_update: !no_auto_update,
                listen,
                display,
                viewer_dir,
                quality,
                idle_minutes,
                tunnel_sleep_minutes,
                viewer_hash,
                public_hosts,
                viewer_origins,
            },
            &mut out,
        )?,
    }
    Ok(())
}

fn run_viewer(action: ViewerAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ViewerAction::Manifest { viewer_dir } => {
            println!(
                "{}",
                wirehaus_agent::static_files::write_manifest(&viewer_dir)?
            );
        }
        ViewerAction::Hash { viewer_dir } => {
            let viewer = wirehaus_agent::static_files::Viewer::open(&viewer_dir, None)?;
            println!("{}", viewer.bundle());
        }
    }
    Ok(())
}

fn run_clipboard(
    store: &Store,
    action: ClipboardAction,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ClipboardAction::Allow { code, direction } => {
            let mode = match direction {
                Direction::Both => ClipboardMode::Both,
                Direction::ToRemote => ClipboardMode::ToRemote,
                Direction::FromRemote => ClipboardMode::FromRemote,
            };
            commands::clipboard(store, mode, Some(&code), out)?;
        }
        ClipboardAction::Deny => commands::clipboard(store, ClipboardMode::Off, None, out)?,
    }
    Ok(())
}

fn run_files(
    store: &Store,
    action: FilesAction,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        FilesAction::Add { folder, code } => commands::files_add(store, &folder, &code, out)?,
        FilesAction::Remove { folder } => commands::files_remove(store, &folder, out)?,
        FilesAction::List => commands::files_list(store, out)?,
    }
    Ok(())
}

fn run_secrets(
    store: &Store,
    action: SecretsAction,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        SecretsAction::Status => commands::secrets_status(store, out)?,
        SecretsAction::Use { target, code } => {
            let target = match target {
                SecretsTarget::System => SecretStorage::System,
                SecretsTarget::File => SecretStorage::File,
            };
            commands::secrets_use(store, target, code.as_deref(), out)?;
        }
    }
    Ok(())
}

struct ServeOptions {
    auto_update: bool,
    listen: std::net::SocketAddr,
    display: Option<String>,
    viewer_dir: std::path::PathBuf,
    quality: u8,
    idle_minutes: u32,
    tunnel_sleep_minutes: u32,
    viewer_hash: Option<String>,
    public_hosts: Vec<String>,
    viewer_origins: Vec<String>,
}

fn serve(
    dir: &std::path::Path,
    options: ServeOptions,
    out: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let layout = if options.auto_update {
        wirehaus_agent::update::Layout::detect(dir).ok()
    } else {
        None
    };
    if let Some(layout) = &layout {
        wirehaus_agent::update::recover(layout);
        if let wirehaus_agent::update::Probation::RolledBack(version) =
            wirehaus_agent::update::begin_probation(layout)?
        {
            return Err(format!("version {version} did not start and was rolled back").into());
        }
    }
    #[cfg(target_os = "macos")]
    {
        let screen = wirehaus_capture::screen_capture_allowed()
            || wirehaus_capture::request_screen_capture();
        let input = wirehaus_input::input_allowed() || wirehaus_input::request_input();
        commands::permissions(screen, input, out)?;
    }
    #[cfg(windows)]
    wirehaus_agent::preflight(options.display.as_deref())?;
    #[cfg(not(target_os = "macos"))]
    let _ = out;
    let exposure = wirehaus_agent::Exposure::new(&options.public_hosts, &options.viewer_origins)?;
    let options_hosts = options.public_hosts.clone();
    let tunnel_sleep_minutes = options.tunnel_sleep_minutes;
    let server = Server::bind(ServerConfig {
        listen: options.listen,
        display: options.display,
        viewer_dir: options.viewer_dir,
        jpeg_quality: options.quality,
        state_dir: dir.to_path_buf(),
        clock: None,
        vault: None,
        idle_timeout: Duration::from_secs(u64::from(options.idle_minutes) * 60),
        viewer_hash: options.viewer_hash,
        exposure,
    })?;
    println!("listening on http://{}", server.local_addr()?);
    let keeper = tunnel_keeper(dir, &options_hosts, tunnel_sleep_minutes);
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        if let Some(layout) = &layout {
            let server = &server;
            let stop = &stop;
            scope.spawn(move || {
                let mut waited = 0;
                while waited < 90 && !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(1));
                    waited += 1;
                }
                if !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    wirehaus_agent::update::mark_healthy(&layout.state_dir);
                }
                wirehaus_agent::update::run_background(
                    layout,
                    &|| server.is_busy(),
                    stop,
                    Duration::from_secs(6 * 3600),
                    Duration::from_secs(60),
                );
            });
        }
        if let Some(keeper) = keeper {
            let server = &server;
            let stop = &stop;
            scope.spawn(move || {
                wirehaus_agent::connector::run_background(
                    keeper,
                    &|| server.idle_for(),
                    &|| server.is_busy(),
                    stop,
                );
            });
        }
        let result = server.run(&stop);
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        result
    })?;
    Ok(())
}

fn tunnel_keeper(
    dir: &std::path::Path,
    public_hosts: &[String],
    sleep_minutes: u32,
) -> Option<
    wirehaus_agent::connector::Keeper<
        wirehaus_agent::connector::CurlRemote,
        wirehaus_agent::connector::ServiceTunnel,
    >,
> {
    use wirehaus_agent::connector::{CurlRemote, Keeper, ServiceTunnel, registry_url, valid_name};
    if sleep_minutes == 0 || !dir.join(wirehaus_agent::connector::TOKEN_FILE).is_file() {
        return None;
    }
    let host = public_hosts.first()?;
    let name = host.split('.').next().filter(|n| valid_name(n))?;
    let base = registry_url(dir, host)?;
    Some(Keeper::new(
        CurlRemote::new(&base),
        ServiceTunnel,
        dir,
        name,
        Duration::from_secs(u64::from(sleep_minutes) * 60),
        std::time::Instant::now(),
    ))
}

fn run_update(
    dir: &std::path::Path,
    action: UpdateAction,
) -> Result<(), Box<dyn std::error::Error>> {
    use wirehaus_agent::update;
    let version = update::Version::current();
    match action {
        UpdateAction::Status => {
            let managed = update::Layout::detect(dir).is_ok();
            println!("version: {version}");
            println!(
                "managed installation: {}",
                if managed { "yes" } else { "no" }
            );
            println!(
                "automatic updates: {}",
                if update::enabled(dir) { "on" } else { "off" }
            );
            if let Some(skipped) = update::skipped(dir) {
                println!("skipped version: {skipped}");
            }
            if let Some(pending) = update::read_pending(dir) {
                println!(
                    "probation: {} to {} (start {} of {})",
                    pending.from,
                    pending.to,
                    pending.attempts,
                    update::MAX_ATTEMPTS
                );
            }
        }
        UpdateAction::Check => {
            let fetch = update::fetcher();
            match update::check(
                &fetch,
                &update::base_url(),
                version,
                update::skipped(dir),
                &update::trusted_keys(),
            )? {
                Some(release) => println!("{} is available (current {version})", release.version),
                None => println!("{version} is the latest version"),
            }
        }
        UpdateAction::Apply => {
            let layout = update::Layout::detect(dir)?;
            match update::apply_now(&layout, &update::fetcher(), &update::base_url())? {
                Some(next) => println!(
                    "{next} is installed; the service restarts when you stop the agent (systemctl --user restart wirehaus-agent)"
                ),
                None => println!("{version} is the latest version"),
            }
        }
        UpdateAction::Enable => {
            update::set_enabled(dir, true)?;
            println!("automatic updates are on");
        }
        UpdateAction::Disable => {
            update::set_enabled(dir, false)?;
            println!("automatic updates are off");
        }
        UpdateAction::Rollback => {
            let layout = update::Layout::detect(dir)?;
            match update::rollback(&layout)? {
                Some(failed) => println!("rolled back from {failed}"),
                None => println!("nothing to roll back to"),
            }
        }
    }
    Ok(())
}
