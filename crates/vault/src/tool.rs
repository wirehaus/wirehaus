use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::VaultError;
use crate::{SERVICE, Vault};

const TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(10);
const DRAIN: Duration = Duration::from_secs(1);
const MAX_OUTPUT: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct ToolVault {
    program: String,
    timeout: Duration,
}

struct Finished {
    success: bool,
    stdout: Vec<u8>,
    stderr: String,
}

impl ToolVault {
    #[must_use]
    pub fn secret_tool() -> Self {
        Self::with_program("secret-tool")
    }

    #[must_use]
    pub fn with_program(program: &str) -> Self {
        Self {
            program: program.to_owned(),
            timeout: TIMEOUT,
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn run(&self, arguments: &[&str], input: Option<&str>) -> Result<Finished, VaultError> {
        let mut child = Command::new(&self.program)
            .args(arguments)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                VaultError::Unavailable(format!(
                    "cannot run {} (install libsecret-tools): {error}",
                    self.program
                ))
            })?;
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            let payload = text.to_owned();
            thread::spawn(move || {
                let _ = stdin.write_all(payload.as_bytes());
            });
        }
        let stdout = capture(child.stdout.take());
        let stderr = capture(child.stderr.take());
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(VaultError::Unavailable(
                        "the vault did not answer in time".to_owned(),
                    ));
                }
                Err(error) => return Err(VaultError::Failed(error.to_string())),
            }
        };
        let stdout = stdout.recv_timeout(DRAIN).unwrap_or_default();
        let stderr = String::from_utf8_lossy(&stderr.recv_timeout(DRAIN).unwrap_or_default())
            .trim()
            .to_owned();
        Ok(Finished {
            success: status.success(),
            stdout,
            stderr,
        })
    }
}

fn capture(pipe: Option<impl Read + Send + 'static>) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(MAX_OUTPUT).read_to_end(&mut bytes);
        }
        let _ = sender.send(bytes);
    });
    receiver
}

impl Vault for ToolVault {
    fn get(&self, name: &str) -> Result<Option<String>, VaultError> {
        let done = self.run(&["lookup", "service", SERVICE, "account", name], None)?;
        if done.success {
            let text = String::from_utf8(done.stdout)
                .map_err(|_| VaultError::Failed("the stored value is not text".to_owned()))?;
            return Ok(Some(text));
        }
        if done.stderr.is_empty() {
            Ok(None)
        } else {
            Err(VaultError::Failed(done.stderr))
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), VaultError> {
        let done = self.run(
            &[
                "store",
                "--label=Wirehaus agent",
                "service",
                SERVICE,
                "account",
                name,
            ],
            Some(value),
        )?;
        if done.success {
            Ok(())
        } else {
            Err(VaultError::Failed(done.stderr))
        }
    }

    fn delete(&self, name: &str) -> Result<(), VaultError> {
        let done = self.run(&["clear", "service", SERVICE, "account", name], None)?;
        if done.success || done.stderr.is_empty() {
            Ok(())
        } else {
            Err(VaultError::Failed(done.stderr))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    static EXCLUSIVE: Mutex<()> = Mutex::new(());

    fn exclusive() -> MutexGuard<'static, ()> {
        EXCLUSIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "wirehaus-vault-{name}-{}-{}",
            std::process::id(),
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn install(directory: &Path, body: &str) -> String {
        let script = directory.join("secret-tool");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script.to_string_lossy().into_owned()
    }

    fn fake(directory: &Path) -> ToolVault {
        let data = directory.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let program = install(
            directory,
            &format!(
                r#"data='{}'
command="$1"
shift
account=
while [ $# -gt 0 ]; do
  if [ "$1" = account ]; then account="$2"; fi
  shift
done
file="$data/$(printf %s "$account" | tr '/' '_')"
case "$command" in
  lookup) if [ -f "$file" ]; then cat "$file"; else exit 1; fi ;;
  store) cat > "$file" ;;
  clear) rm -f "$file" ;;
  *) echo "unknown command" >&2; exit 2 ;;
esac"#,
                data.display()
            ),
        );
        ToolVault::with_program(&program)
    }

    #[test]
    fn a_value_goes_in_comes_out_and_goes_away() {
        let _exclusive = exclusive();
        let directory = scratch("roundtrip");
        let vault = fake(&directory);
        assert_eq!(vault.get("a/key").unwrap(), None);
        vault.set("a/key", "SECRETVALUE").unwrap();
        assert_eq!(vault.get("a/key").unwrap().as_deref(), Some("SECRETVALUE"));
        vault.set("a/key", "OTHER").unwrap();
        assert_eq!(vault.get("a/key").unwrap().as_deref(), Some("OTHER"));
        vault.delete("a/key").unwrap();
        assert_eq!(vault.get("a/key").unwrap(), None);
        vault.delete("a/key").unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_secret_never_appears_in_the_arguments() {
        let _exclusive = exclusive();
        let directory = scratch("arguments");
        let log = directory.join("arguments");
        let program = install(
            &directory,
            &format!("echo \"$@\" >> '{}'\ncat > /dev/null", log.display()),
        );
        let vault = ToolVault::with_program(&program);
        vault.set("account", "TOPSECRETVALUE").unwrap();
        let seen = std::fs::read_to_string(&log).unwrap();
        assert!(seen.contains("store"), "{seen}");
        assert!(!seen.contains("TOPSECRETVALUE"), "{seen}");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_locked_or_broken_vault_is_an_error_and_not_an_absent_value() {
        let _exclusive = exclusive();
        let directory = scratch("locked");
        let program = install(
            &directory,
            "echo 'Cannot create an item in a locked collection' >&2\nexit 1",
        );
        let vault = ToolVault::with_program(&program);
        assert!(matches!(
            vault.get("account"),
            Err(VaultError::Failed(message)) if message.contains("locked")
        ));
        assert!(vault.set("account", "value").is_err());
        assert!(vault.delete("account").is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_missing_tool_says_what_to_install() {
        let vault = ToolVault::with_program("/nonexistent/secret-tool");
        match vault.get("account") {
            Err(VaultError::Unavailable(message)) => {
                assert!(message.contains("libsecret-tools"), "{message}");
            }
            other => panic!("unexpected answer: {other:?}"),
        }
    }

    #[test]
    fn a_tool_that_never_answers_is_stopped() {
        let _exclusive = exclusive();
        let directory = scratch("slow");
        let program = install(&directory, "sleep 600");
        let vault = ToolVault::with_program(&program).with_timeout(Duration::from_millis(300));
        let started = Instant::now();
        assert!(matches!(
            vault.get("account"),
            Err(VaultError::Unavailable(_))
        ));
        assert!(started.elapsed() < Duration::from_secs(5));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
