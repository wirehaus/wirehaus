use std::fs;
use std::path::Path;

const MAX_FILE_LEN: u64 = 4 << 20;

pub struct StaticFile {
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

pub fn load(root: &Path, request_path: &str) -> Option<StaticFile> {
    let name = file_name(request_path)?;
    let content_type = content_type(name)?;
    let path = root.join(name);
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_LEN {
        return None;
    }
    Some(StaticFile {
        body: fs::read(path).ok()?,
        content_type,
    })
}

fn file_name(request_path: &str) -> Option<&str> {
    let path = request_path.split(['?', '#']).next()?;
    if path == "/" {
        return Some("index.html");
    }
    let name = path.strip_prefix('/')?;
    let valid = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    valid.then_some(name)
}

fn content_type(name: &str) -> Option<&'static str> {
    match name.rsplit_once('.')?.1 {
        "html" => Some("text/html; charset=utf-8"),
        "js" => Some("text/javascript; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_root_to_the_index() {
        assert_eq!(file_name("/"), Some("index.html"));
        assert_eq!(file_name("/?x=1"), Some("index.html"));
    }

    #[test]
    fn accepts_plain_file_names() {
        assert_eq!(file_name("/viewer.js"), Some("viewer.js"));
        assert_eq!(file_name("/style-1_b.css"), Some("style-1_b.css"));
    }

    #[test]
    fn rejects_anything_that_could_leave_the_directory() {
        for path in [
            "/../etc/passwd",
            "/..",
            "/a/b.js",
            "/%2e%2e/x.js",
            "/.hidden.js",
            "/a\\b.js",
            "//etc/passwd",
            "",
            "viewer.js",
        ] {
            assert_eq!(file_name(path), None, "{path}");
        }
    }

    #[test]
    fn only_known_extensions_are_served() {
        assert!(content_type("a.html").is_some());
        assert!(content_type("a.js").is_some());
        assert!(content_type("a.css").is_some());
        assert!(content_type("a.map").is_none());
        assert!(content_type("a").is_none());
    }

    #[test]
    fn loads_a_file_from_the_root() {
        let dir = std::env::temp_dir().join(format!("wirehaus-static-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.html"), b"<p>hi</p>").unwrap();
        let file = load(&dir, "/").unwrap();
        assert_eq!(file.body, b"<p>hi</p>");
        assert_eq!(file.content_type, "text/html; charset=utf-8");
        assert!(load(&dir, "/missing.js").is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
