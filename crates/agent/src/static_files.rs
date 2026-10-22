use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use crate::integrity::{self, IntegrityError, MANIFEST_NAME};

const MAX_FILE_LEN: u64 = 4 << 20;

#[derive(Debug)]
pub struct StaticFile {
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

pub struct Viewer {
    files: HashMap<String, StaticFile>,
    bundle: String,
}

impl std::fmt::Debug for Viewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewer")
            .field("files", &self.files.len())
            .field("bundle", &self.bundle)
            .finish()
    }
}

impl Viewer {
    pub fn open(dir: &Path, expected: Option<&str>) -> Result<Self, IntegrityError> {
        let mut servable = read_servable(dir)?;
        let manifest = read_manifest(dir)?;
        integrity::verify(&servable, &manifest)?;
        let bundle = integrity::bundle_hash(&manifest);
        if let Some(wanted) = expected
            && wanted != bundle
        {
            return Err(IntegrityError::WrongBundle {
                expected: wanted.to_owned(),
                actual: bundle,
            });
        }
        servable.insert(MANIFEST_NAME.to_owned(), manifest.into_bytes());
        let files = servable
            .into_iter()
            .filter_map(|(name, body)| {
                let content_type = content_type(&name)?;
                Some((name, StaticFile { body, content_type }))
            })
            .collect();
        Ok(Self { files, bundle })
    }

    #[must_use]
    pub fn bundle(&self) -> &str {
        &self.bundle
    }

    #[must_use]
    pub fn get(&self, request_path: &str) -> Option<&StaticFile> {
        self.files.get(file_name(request_path)?)
    }
}

pub fn write_manifest(dir: &Path) -> Result<String, IntegrityError> {
    let servable = read_servable(dir)?;
    let manifest = integrity::manifest_text(&servable);
    fs::write(dir.join(MANIFEST_NAME), &manifest).map_err(|e| IntegrityError::Io(e.to_string()))?;
    Ok(integrity::bundle_hash(&manifest))
}

fn read_manifest(dir: &Path) -> Result<String, IntegrityError> {
    match fs::read_to_string(dir.join(MANIFEST_NAME)) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(IntegrityError::MissingManifest)
        }
        Err(error) => Err(IntegrityError::Io(error.to_string())),
    }
}

fn read_servable(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, IntegrityError> {
    let io = |error: std::io::Error| IntegrityError::Io(error.to_string());
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name == MANIFEST_NAME
            || file_name(&format!("/{name}")).is_none()
            || content_type(&name).is_none()
        {
            continue;
        }
        let metadata = fs::metadata(entry.path()).map_err(io)?;
        if !metadata.is_file() {
            continue;
        }
        if metadata.len() > MAX_FILE_LEN {
            return Err(IntegrityError::Io(format!(
                "{name} is larger than {MAX_FILE_LEN} bytes"
            )));
        }
        files.insert(name, fs::read(entry.path()).map_err(io)?);
    }
    Ok(files)
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
        "js" | "mjs" => Some("text/javascript; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "wasm" => Some("application/wasm"),
        "sha256" => Some("text/plain; charset=utf-8"),
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
        assert_eq!(content_type("a.mjs"), content_type("a.js"));
        assert!(content_type("a.css").is_some());
        assert_eq!(content_type("a.wasm"), Some("application/wasm"));
        assert!(content_type("manifest.sha256").is_some());
        assert!(content_type("a.map").is_none());
        assert!(content_type("a").is_none());
    }

    struct TempViewer(std::path::PathBuf);

    impl TempViewer {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("wirehaus-static-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("index.html"), b"<p>hi</p>").unwrap();
            fs::write(dir.join("main.js"), b"export {};").unwrap();
            Self(dir)
        }
    }

    impl Drop for TempViewer {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn serves_a_verified_viewer_from_memory() {
        let temp = TempViewer::new("serve");
        let bundle = write_manifest(&temp.0).unwrap();
        let viewer = Viewer::open(&temp.0, Some(&bundle)).unwrap();
        assert_eq!(viewer.bundle(), bundle);
        let index = viewer.get("/").unwrap();
        assert_eq!(index.body, b"<p>hi</p>");
        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert!(viewer.get("/missing.js").is_none());
        fs::write(temp.0.join("index.html"), b"<p>changed</p>").unwrap();
        assert_eq!(viewer.get("/").unwrap().body, b"<p>hi</p>");
    }

    #[test]
    fn the_manifest_is_served_so_it_can_be_checked_from_outside() {
        let temp = TempViewer::new("manifest");
        write_manifest(&temp.0).unwrap();
        let viewer = Viewer::open(&temp.0, None).unwrap();
        let manifest = viewer.get("/manifest.sha256").unwrap();
        assert_eq!(manifest.content_type, "text/plain; charset=utf-8");
        let text = String::from_utf8(manifest.body.clone()).unwrap();
        assert!(text.contains("  index.html\n") && text.contains("  main.js\n"));
        assert!(!text.contains("manifest.sha256"));
    }

    #[test]
    fn a_viewer_without_a_manifest_or_with_changes_is_refused() {
        let temp = TempViewer::new("refuse");
        assert_eq!(
            Viewer::open(&temp.0, None).unwrap_err(),
            IntegrityError::MissingManifest
        );
        write_manifest(&temp.0).unwrap();
        fs::write(temp.0.join("main.js"), b"steal()").unwrap();
        assert_eq!(
            Viewer::open(&temp.0, None).unwrap_err(),
            IntegrityError::Mismatch("main.js".to_owned())
        );
        write_manifest(&temp.0).unwrap();
        fs::write(temp.0.join("extra.js"), b"x").unwrap();
        assert_eq!(
            Viewer::open(&temp.0, None).unwrap_err(),
            IntegrityError::Unlisted("extra.js".to_owned())
        );
        fs::remove_file(temp.0.join("main.js")).unwrap();
        fs::remove_file(temp.0.join("extra.js")).unwrap();
        assert_eq!(
            Viewer::open(&temp.0, None).unwrap_err(),
            IntegrityError::Missing("main.js".to_owned())
        );
    }

    #[test]
    fn a_pinned_hash_must_match_and_files_that_are_not_served_are_ignored() {
        let temp = TempViewer::new("pin");
        fs::write(temp.0.join("notes.txt"), b"not served").unwrap();
        fs::write(temp.0.join("app.js.map"), b"{}").unwrap();
        let bundle = write_manifest(&temp.0).unwrap();
        assert!(Viewer::open(&temp.0, Some(&bundle)).is_ok());
        let wrong = "0".repeat(64);
        assert!(matches!(
            Viewer::open(&temp.0, Some(&wrong)),
            Err(IntegrityError::WrongBundle { .. })
        ));
        let manifest = fs::read_to_string(temp.0.join(MANIFEST_NAME)).unwrap();
        assert!(!manifest.contains("notes.txt") && !manifest.contains("app.js.map"));
    }

    #[test]
    fn the_same_files_always_give_the_same_hash() {
        let first = TempViewer::new("same-a");
        let second = TempViewer::new("same-b");
        assert_eq!(
            write_manifest(&first.0).unwrap(),
            write_manifest(&second.0).unwrap()
        );
        fs::write(second.0.join("main.js"), b"export {}; ").unwrap();
        assert_ne!(
            write_manifest(&first.0).unwrap(),
            write_manifest(&second.0).unwrap()
        );
    }

    #[test]
    fn no_request_path_can_name_anything_outside_the_viewer_directory() {
        let mut rng = crate::testing::Rng::new(0xB10C_C0DE_0000_0042);
        let alphabet: &[u8] = b"/.\\%abcXYZ019_-?#\0 :~";
        let mut served = 0;
        for _ in 0..100_000 {
            let length = rng.below(24);
            let path: String = (0..length)
                .map(|_| char::from(alphabet[rng.below(alphabet.len())]))
                .collect();
            if let Some(name) = file_name(&path) {
                served += 1;
                assert!(!name.contains('/') && !name.contains('\\'));
                assert!(!name.starts_with('.'));
                assert_ne!(name, "");
                assert_ne!(name, "..");
            }
        }
        assert!(served > 100);
    }
}
