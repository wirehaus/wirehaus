use std::collections::BTreeMap;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const MANIFEST_NAME: &str = "manifest.sha256";
const HASH_HEX_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IntegrityError {
    #[error("the viewer has no {MANIFEST_NAME}; create it with \"wirehaus-agent viewer manifest\"")]
    MissingManifest,
    #[error("the viewer manifest is not valid: {0}")]
    InvalidManifest(String),
    #[error("{0} does not match its hash in the manifest")]
    Mismatch(String),
    #[error("{0} is listed in the manifest but is missing")]
    Missing(String),
    #[error("{0} is in the viewer directory but not in the manifest")]
    Unlisted(String),
    #[error("the viewer has hash {actual} but {expected} was expected")]
    WrongBundle { expected: String, actual: String },
    #[error("cannot read the viewer: {0}")]
    Io(String),
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(HASH_HEX_LEN), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[must_use]
pub fn manifest_text(files: &BTreeMap<String, Vec<u8>>) -> String {
    files.iter().fold(String::new(), |mut out, (name, body)| {
        let _ = writeln!(out, "{}  {name}", sha256_hex(body));
        out
    })
}

#[must_use]
pub fn bundle_hash(manifest: &str) -> String {
    sha256_hex(manifest.as_bytes())
}

pub fn parse(manifest: &str) -> Result<BTreeMap<String, String>, IntegrityError> {
    let invalid = |reason: &str| IntegrityError::InvalidManifest(reason.to_owned());
    if manifest.is_empty() || !manifest.ends_with('\n') {
        return Err(invalid("it must end with a newline and not be empty"));
    }
    let mut entries = BTreeMap::new();
    let mut previous: Option<&str> = None;
    for line in manifest.lines() {
        let (hash, name) = line
            .split_once("  ")
            .ok_or_else(|| invalid("a line must be a hash, two spaces and a name"))?;
        let well_formed = hash.len() == HASH_HEX_LEN
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !well_formed {
            return Err(invalid("a hash must be 64 lowercase hexadecimal digits"));
        }
        if name.is_empty() || name.contains(['/', '\\', '\n']) || name.starts_with('.') {
            return Err(invalid("a name must be a plain file name"));
        }
        if previous.is_some_and(|last| last >= name) {
            return Err(invalid("names must be sorted and unique"));
        }
        previous = Some(name);
        entries.insert(name.to_owned(), hash.to_owned());
    }
    Ok(entries)
}

pub fn verify(files: &BTreeMap<String, Vec<u8>>, manifest: &str) -> Result<(), IntegrityError> {
    let listed = parse(manifest)?;
    for (name, expected) in &listed {
        let body = files
            .get(name)
            .ok_or_else(|| IntegrityError::Missing(name.clone()))?;
        if sha256_hex(body) != *expected {
            return Err(IntegrityError::Mismatch(name.clone()));
        }
    }
    if let Some(extra) = files.keys().find(|name| !listed.contains_key(*name)) {
        return Err(IntegrityError::Unlisted(extra.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(entries: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(name, body)| ((*name).to_owned(), body.to_vec()))
            .collect()
    }

    #[test]
    fn computes_the_standard_sha256() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn the_manifest_uses_the_sha256sum_layout_in_name_order() {
        let text = manifest_text(&files(&[("b.js", b"abc"), ("a.css", b"")]));
        assert_eq!(
            text,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  a.css\n\
             ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  b.js\n"
        );
        assert_eq!(bundle_hash(&text), sha256_hex(text.as_bytes()));
    }

    #[test]
    fn a_matching_viewer_verifies() {
        let viewer = files(&[("index.html", b"<p>"), ("main.js", b"x")]);
        assert_eq!(verify(&viewer, &manifest_text(&viewer)), Ok(()));
    }

    #[test]
    fn any_change_is_detected() {
        let viewer = files(&[("index.html", b"<p>"), ("main.js", b"x")]);
        let manifest = manifest_text(&viewer);
        let mut changed = viewer.clone();
        changed.insert("main.js".to_owned(), b"y".to_vec());
        assert_eq!(
            verify(&changed, &manifest),
            Err(IntegrityError::Mismatch("main.js".to_owned()))
        );
        let mut missing = viewer.clone();
        missing.remove("main.js");
        assert_eq!(
            verify(&missing, &manifest),
            Err(IntegrityError::Missing("main.js".to_owned()))
        );
        let mut extra = viewer;
        extra.insert("evil.js".to_owned(), b"z".to_vec());
        assert_eq!(
            verify(&extra, &manifest),
            Err(IntegrityError::Unlisted("evil.js".to_owned()))
        );
    }

    #[test]
    fn rejects_malformed_manifests() {
        let good = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        for text in [
            String::new(),
            format!("{good}  a.js"),
            format!("{good} a.js\n"),
            format!("{}  a.js\n", good.to_uppercase()),
            format!("{}  a.js\n", &good[1..]),
            format!("{good}  \n"),
            format!("{good}  a/b.js\n"),
            format!("{good}  ..\\x.js\n"),
            format!("{good}  .hidden.js\n"),
            format!("{good}  b.js\n{good}  a.js\n"),
            format!("{good}  a.js\n{good}  a.js\n"),
            format!("{good}  a.js\n\n"),
            format!("{good}  a.js\n{}  b.js\n", "g".repeat(64)),
        ] {
            assert!(
                matches!(parse(&text), Err(IntegrityError::InvalidManifest(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_bundle_hash_changes_with_any_file() {
        let one = manifest_text(&files(&[("a.js", b"1")]));
        let two = manifest_text(&files(&[("a.js", b"2")]));
        assert_ne!(bundle_hash(&one), bundle_hash(&two));
        assert_eq!(bundle_hash(&one), bundle_hash(&one));
    }
}
