use std::path::Path;
use std::process::Command;

const SYSTEM_SID: &str = "*S-1-5-18";
const FOLDER_CONTROL: &str = "(OI)(CI)F";
const FILE_CONTROL: &str = "F";

pub fn restrict_to_current_user(path: &Path) -> std::io::Result<()> {
    let sid = current_user_sid()?;
    let rights = if path.is_dir() {
        FOLDER_CONTROL
    } else {
        FILE_CONTROL
    };
    let status = Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(format!("*{sid}:{rights}"))
        .arg("/grant:r")
        .arg(format!("{SYSTEM_SID}:{rights}"))
        .output()?
        .status;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "icacls exited with {status}"
        )))
    }
}

#[cfg(test)]
pub fn entries(path: &Path) -> std::io::Result<usize> {
    let output = Command::new("icacls").arg(path).output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.matches(":(").count())
}

fn current_user_sid() -> std::io::Result<String> {
    let output = Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_sid(&text).ok_or_else(|| std::io::Error::other("cannot read the current user"))
}

fn parse_sid(text: &str) -> Option<String> {
    let last = text.trim().rsplit(',').next()?;
    let sid = last.trim().trim_matches('"');
    (sid.starts_with("S-1-") && sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then(|| sid.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_sid_from_the_csv_line() {
        assert_eq!(
            parse_sid("\"windows\\cristobal\",\"S-1-5-21-1-2-3-1001\"\r\n"),
            Some("S-1-5-21-1-2-3-1001".to_owned())
        );
    }

    #[test]
    fn rejects_anything_that_is_not_a_sid() {
        assert_eq!(parse_sid("\"a\",\"not a sid\""), None);
        assert_eq!(parse_sid(""), None);
        assert_eq!(parse_sid("\"a\",\"S-1-5-21-1;calc\""), None);
    }

    #[test]
    fn a_restricted_directory_keeps_only_the_user_and_the_system() {
        let dir = std::env::temp_dir().join(format!("wirehaus-acl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        restrict_to_current_user(&dir).unwrap();
        assert_eq!(entries(&dir).unwrap(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_restricted_file_stays_readable_by_its_owner() {
        let dir = std::env::temp_dir().join(format!("wirehaus-acl-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state");
        std::fs::write(&file, "data").unwrap();
        restrict_to_current_user(&file).unwrap();
        assert_eq!(entries(&file).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "data");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
