use crate::config_store::resolve_path_with_fallback;
use crate::core::vault::{
    KdfParams, NONCE_LEN, SALT_LEN, VaultError, keys_path, open, seal, stored_names,
};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use thiserror::Error;

const FILE_MODE: u32 = 0o600;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("{0}")]
    Vault(#[from] VaultError),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("could not gather random bytes: {0}")]
    Random(String),
}

pub fn resolve_path() -> PathBuf {
    resolve_path_with_fallback("MUADDIB_KEYS", "XDG_STATE_HOME", keys_path)
}

pub fn names() -> Vec<String> {
    names_at(&resolve_path())
}

pub fn unlock(passphrase: &str) -> Result<BTreeMap<String, String>, StoreError> {
    unlock_at(&resolve_path(), passphrase)
}

pub fn save(entries: &BTreeMap<String, String>, passphrase: &str) -> Result<(), StoreError> {
    save_at(&resolve_path(), entries, passphrase)
}

pub fn exists() -> bool {
    resolve_path().is_file()
}

pub fn names_at(path: &Path) -> Vec<String> {
    fs::read(path)
        .map(|bytes| stored_names(&bytes))
        .unwrap_or_default()
}

pub fn unlock_at(path: &Path, passphrase: &str) -> Result<BTreeMap<String, String>, StoreError> {
    match fs::read(path) {
        Ok(bytes) => Ok(open(&bytes, passphrase)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error.into()),
    }
}

pub fn save_at(
    path: &Path,
    entries: &BTreeMap<String, String>,
    passphrase: &str,
) -> Result<(), StoreError> {
    let mut seed = [0u8; SALT_LEN + NONCE_LEN];
    getrandom::fill(&mut seed).map_err(|error| StoreError::Random(error.to_string()))?;
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    salt.copy_from_slice(&seed[..SALT_LEN]);
    nonce.copy_from_slice(&seed[SALT_LEN..]);
    let sealed = seal(entries, passphrase, salt, nonce, KdfParams::default())?;
    write_private(path, &sealed)?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = temp_sibling(path)?;
    let outcome = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(FILE_MODE)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(&temp);
    }
    outcome
}

fn temp_sibling(path: &Path) -> io::Result<PathBuf> {
    let mut suffix = [0u8; 8];
    getrandom::fill(&mut suffix)
        .map_err(|error| io::Error::other(format!("could not gather random bytes: {error}")))?;
    let tag: String = suffix.iter().map(|byte| format!("{byte:02x}")).collect();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("keys.enc");
    Ok(path.with_file_name(format!(".{name}.{tag}.tmp")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut suffix = [0u8; 8];
            getrandom::fill(&mut suffix).expect("random");
            let unique: String = suffix.iter().map(|byte| format!("{byte:02x}")).collect();
            let dir = std::env::temp_dir().join(format!("muaddib-vault-{tag}-{unique}"));
            fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }

        fn keys(&self) -> PathBuf {
            self.0.join("nested").join("keys.enc")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn entries(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, key)| ((*name).to_string(), (*key).to_string()))
            .collect()
    }

    #[test]
    fn saving_then_unlocking_round_trips_through_the_file() {
        let dir = TempDir::new("roundtrip");
        let want = entries(&[("anthropic", "sk-ant-1"), ("openai", "sk-2")]);
        save_at(&dir.keys(), &want, "hunter2").expect("save");
        assert_eq!(unlock_at(&dir.keys(), "hunter2").expect("unlock"), want);
    }

    #[test]
    fn the_written_file_is_readable_only_by_the_owner() {
        let dir = TempDir::new("mode");
        save_at(&dir.keys(), &entries(&[("openai", "sk-1")]), "pass").expect("save");
        let mode = fs::metadata(dir.keys())
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, FILE_MODE, "{mode:o}");
    }

    #[test]
    fn the_file_on_disk_never_contains_the_plaintext_key() {
        let dir = TempDir::new("opaque");
        save_at(
            &dir.keys(),
            &entries(&[("openai", "sk-plaintext-leak")]),
            "pass",
        )
        .expect("save");
        let bytes = fs::read(dir.keys()).expect("read");
        assert!(!String::from_utf8_lossy(&bytes).contains("sk-plaintext-leak"));
    }

    #[test]
    fn names_are_listed_without_the_passphrase() {
        let dir = TempDir::new("names");
        save_at(
            &dir.keys(),
            &entries(&[("gemini", "k1"), ("ollama", "k2")]),
            "pass",
        )
        .expect("save");
        assert_eq!(
            names_at(&dir.keys()),
            vec!["gemini".to_string(), "ollama".to_string()]
        );
    }

    #[test]
    fn a_missing_vault_unlocks_to_an_empty_map_and_lists_no_names() {
        let dir = TempDir::new("missing");
        assert!(unlock_at(&dir.keys(), "pass").expect("unlock").is_empty());
        assert!(names_at(&dir.keys()).is_empty());
    }

    #[test]
    fn the_wrong_passphrase_is_rejected() {
        let dir = TempDir::new("wrong");
        save_at(&dir.keys(), &entries(&[("openai", "sk-1")]), "right").expect("save");
        let error = unlock_at(&dir.keys(), "wrong").expect_err("must reject");
        assert!(
            matches!(error, StoreError::Vault(VaultError::Decrypt)),
            "{error}"
        );
    }

    #[test]
    fn saving_twice_replaces_the_vault_and_leaves_no_temp_files() {
        let dir = TempDir::new("replace");
        save_at(&dir.keys(), &entries(&[("openai", "sk-1")]), "pass").expect("first");
        save_at(&dir.keys(), &entries(&[("gemini", "sk-2")]), "pass").expect("second");
        assert_eq!(
            unlock_at(&dir.keys(), "pass").expect("unlock"),
            entries(&[("gemini", "sk-2")])
        );
        let leftovers: Vec<_> = fs::read_dir(dir.keys().parent().expect("parent"))
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn a_temp_sibling_stays_in_the_target_directory_and_is_hidden() {
        let temp = temp_sibling(Path::new("/var/lib/muaddib/keys.enc")).expect("temp");
        assert_eq!(temp.parent(), Some(Path::new("/var/lib/muaddib")));
        let name = temp
            .file_name()
            .and_then(|name| name.to_str())
            .expect("name");
        assert!(name.starts_with(".keys.enc."), "{name}");
        assert!(name.contains(".tmp"), "{name}");
    }

    #[test]
    fn two_temp_siblings_never_collide() {
        let path = Path::new("/tmp/keys.enc");
        assert_ne!(
            temp_sibling(path).expect("first"),
            temp_sibling(path).expect("second")
        );
    }
}
