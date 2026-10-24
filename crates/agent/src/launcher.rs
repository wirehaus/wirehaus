use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

pub const AGENT_IMAGE: &str = "wirehaus-agent.exe";
pub const MAX_LOG_BYTES: u64 = 1 << 20;

#[must_use]
pub fn agent_path(launcher: &Path) -> Option<PathBuf> {
    launcher.parent().map(|dir| dir.join(AGENT_IMAGE))
}

#[must_use]
pub fn log_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Wirehaus").join("agent.log")
}

pub fn open_log(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let oversized = std::fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES);
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if oversized {
        options.truncate(true);
    } else {
        options.append(true);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wirehaus-launcher-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_agent_is_expected_next_to_the_launcher() {
        assert_eq!(
            agent_path(Path::new("/opt/wirehaus/wirehaus-agentd.exe")),
            Some(PathBuf::from("/opt/wirehaus/wirehaus-agent.exe"))
        );
    }

    #[test]
    fn the_log_lives_under_the_local_application_data_folder() {
        assert_eq!(
            log_path(Path::new("/data")),
            PathBuf::from("/data/Wirehaus/agent.log")
        );
    }

    #[test]
    fn a_small_log_is_appended_to() {
        let dir = temp("append");
        let path = dir.join("agent.log");
        open_log(&path).unwrap().write_all(b"one\n").unwrap();
        open_log(&path).unwrap().write_all(b"two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_oversized_log_starts_over() {
        let dir = temp("rotate");
        let path = dir.join("agent.log");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            vec![b'x'; usize::try_from(MAX_LOG_BYTES).unwrap() + 1],
        )
        .unwrap();
        open_log(&path).unwrap().write_all(b"fresh\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
