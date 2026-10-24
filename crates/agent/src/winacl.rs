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
pub fn grantees(path: &Path) -> std::io::Result<Vec<String>> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let saved = std::env::temp_dir().join(format!(
        "wirehaus-acl-{}-{}.sddl",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let status = Command::new("icacls")
        .arg(path)
        .arg("/save")
        .arg(&saved)
        .output()?
        .status;
    if !status.success() {
        return Err(std::io::Error::other("icacls could not save the ACL"));
    }
    let bytes = std::fs::read(&saved)?;
    std::fs::remove_file(&saved)?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    Ok(sddl_grantees(&String::from_utf16_lossy(&units)))
}

#[cfg(test)]
fn sddl_grantees(text: &str) -> Vec<String> {
    let mut found: Vec<String> = text
        .split('(')
        .skip(1)
        .filter_map(|entry| entry.split(')').next())
        .filter_map(|entry| entry.rsplit(';').next())
        .map(str::to_owned)
        .collect();
    found.sort();
    found
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

    fn expected_grantees() -> Vec<String> {
        let mut expected = vec!["SY".to_owned(), current_user_sid().unwrap()];
        expected.sort();
        expected
    }

    #[test]
    fn lists_the_trustees_of_an_sddl_string() {
        assert_eq!(
            sddl_grantees("D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;S-1-5-21-1-2-3-1001)"),
            vec!["S-1-5-21-1-2-3-1001".to_owned(), "SY".to_owned()]
        );
    }

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
        assert_eq!(grantees(&dir).unwrap(), expected_grantees());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_restricted_file_stays_readable_by_its_owner() {
        let dir = std::env::temp_dir().join(format!("wirehaus-acl-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state");
        std::fs::write(&file, "data").unwrap();
        restrict_to_current_user(&file).unwrap();
        assert_eq!(grantees(&file).unwrap(), expected_grantees());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "data");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
