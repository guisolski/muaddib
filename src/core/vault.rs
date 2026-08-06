use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use zeroize::Zeroizing;

pub const MAGIC: &[u8; 8] = b"MUADDIB1";
pub const VERSION: u8 = 1;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;

const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
const HEADER_FIXED: usize = MAGIC.len() + 1 + 12 + SALT_LEN + NONCE_LEN + 2;

const MIN_M_COST: u32 = 8;
const MAX_M_COST: u32 = 1_048_576;
const MAX_T_COST: u32 = 16;
const MAX_P_COST: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost: 19_456,
            t_cost: 2,
            p_cost: 1,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    #[error("not a muaddib key vault")]
    BadMagic,
    #[error("unsupported vault version {0}")]
    BadVersion(u8),
    #[error("vault file is truncated")]
    Truncated,
    #[error("vault key-derivation parameters are out of range")]
    BadParams,
    #[error("wrong passphrase or corrupted vault")]
    Decrypt,
    #[error("vault contents are not valid")]
    BadContents,
    #[error("key derivation failed")]
    Kdf,
    #[error("engine name must not be empty or contain a newline")]
    BadName,
}

pub fn keys_path(home: &Path, xdg_state_home: Option<&Path>) -> PathBuf {
    xdg_state_home
        .map_or_else(|| home.join(".local").join("state"), Path::to_path_buf)
        .join("muaddib")
        .join("keys.enc")
}

#[derive(Clone, PartialEq, Eq)]
pub struct Passphrase(Zeroizing<String>);

impl Passphrase {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Passphrase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Passphrase(***)")
    }
}

pub fn mask(value: &str) -> String {
    const MAX_DOTS: usize = 8;
    const TAIL: usize = 4;
    let characters: Vec<char> = value.chars().collect();
    if characters.len() <= TAIL {
        return "•".repeat(characters.len());
    }
    let tail: String = characters[characters.len() - TAIL..].iter().collect();
    format!(
        "{}{tail}",
        "•".repeat((characters.len() - TAIL).min(MAX_DOTS))
    )
}

pub fn seal(
    entries: &BTreeMap<String, String>,
    passphrase: &str,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    params: KdfParams,
) -> Result<Vec<u8>, VaultError> {
    check_params(params)?;
    let header = build_header(entries, salt, nonce, params)?;
    let plaintext = Zeroizing::new(toml::to_string(entries).map_err(|_| VaultError::BadContents)?);
    let key = derive_key(passphrase, &salt, params)?;
    let cipher = XChaCha20Poly1305::new(key.as_slice().into());
    let sealed = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext.as_bytes(),
                aad: &header,
            },
        )
        .map_err(|_| VaultError::Decrypt)?;
    let mut out = header;
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub fn open(sealed: &[u8], passphrase: &str) -> Result<BTreeMap<String, String>, VaultError> {
    let header_len = header_len(sealed)?;
    let (header, body) = sealed.split_at(header_len);
    if body.len() < TAG_LEN {
        return Err(VaultError::Truncated);
    }
    let params = read_params(header)?;
    check_params(params)?;
    let salt = read_salt(header);
    let nonce = read_nonce(header);
    let key = derive_key(passphrase, &salt, params)?;
    let cipher = XChaCha20Poly1305::new(key.as_slice().into());
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: body,
                    aad: header,
                },
            )
            .map_err(|_| VaultError::Decrypt)?,
    );
    let text = std::str::from_utf8(&plaintext).map_err(|_| VaultError::BadContents)?;
    toml::from_str(text).map_err(|_| VaultError::BadContents)
}

pub fn stored_names(sealed: &[u8]) -> Vec<String> {
    let Ok(header_len) = header_len(sealed) else {
        return Vec::new();
    };
    let start = HEADER_FIXED;
    let Some(raw) = sealed.get(start..header_len) else {
        return Vec::new();
    };
    let Ok(text) = std::str::from_utf8(raw) else {
        return Vec::new();
    };
    text.split('\n')
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn build_header(
    entries: &BTreeMap<String, String>,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    params: KdfParams,
) -> Result<Vec<u8>, VaultError> {
    if entries
        .keys()
        .any(|name| name.is_empty() || name.contains('\n'))
    {
        return Err(VaultError::BadName);
    }
    let names = entries.keys().cloned().collect::<Vec<_>>().join("\n");
    let names_len = u16::try_from(names.len()).map_err(|_| VaultError::BadName)?;
    let mut header = Vec::with_capacity(HEADER_FIXED + names.len());
    header.extend_from_slice(MAGIC);
    header.push(VERSION);
    header.extend_from_slice(&params.m_cost.to_le_bytes());
    header.extend_from_slice(&params.t_cost.to_le_bytes());
    header.extend_from_slice(&params.p_cost.to_le_bytes());
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);
    header.extend_from_slice(&names_len.to_le_bytes());
    header.extend_from_slice(names.as_bytes());
    Ok(header)
}

fn header_len(sealed: &[u8]) -> Result<usize, VaultError> {
    if sealed.len() < HEADER_FIXED {
        return Err(VaultError::Truncated);
    }
    if &sealed[..MAGIC.len()] != MAGIC {
        return Err(VaultError::BadMagic);
    }
    let version = sealed[MAGIC.len()];
    if version != VERSION {
        return Err(VaultError::BadVersion(version));
    }
    let names_len = usize::from(u16::from_le_bytes([
        sealed[HEADER_FIXED - 2],
        sealed[HEADER_FIXED - 1],
    ]));
    let total = HEADER_FIXED + names_len;
    if sealed.len() < total {
        return Err(VaultError::Truncated);
    }
    Ok(total)
}

fn read_params(header: &[u8]) -> Result<KdfParams, VaultError> {
    let base = MAGIC.len() + 1;
    let word = |offset: usize| -> Result<u32, VaultError> {
        header
            .get(offset..offset + 4)
            .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
            .map(u32::from_le_bytes)
            .ok_or(VaultError::Truncated)
    };
    Ok(KdfParams {
        m_cost: word(base)?,
        t_cost: word(base + 4)?,
        p_cost: word(base + 8)?,
    })
}

fn read_salt(header: &[u8]) -> [u8; SALT_LEN] {
    let start = MAGIC.len() + 1 + 12;
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&header[start..start + SALT_LEN]);
    salt
}

fn read_nonce(header: &[u8]) -> [u8; NONCE_LEN] {
    let start = MAGIC.len() + 1 + 12 + SALT_LEN;
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&header[start..start + NONCE_LEN]);
    nonce
}

fn check_params(params: KdfParams) -> Result<(), VaultError> {
    let sane = (MIN_M_COST..=MAX_M_COST).contains(&params.m_cost)
        && (1..=MAX_T_COST).contains(&params.t_cost)
        && (1..=MAX_P_COST).contains(&params.p_cost)
        && params.m_cost >= 8 * params.p_cost;
    sane.then_some(()).ok_or(VaultError::BadParams)
}

fn derive_key(
    passphrase: &str,
    salt: &[u8],
    params: KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
    let argon_params = Params::new(params.m_cost, params.t_cost, params.p_cost, Some(KEY_LEN))
        .map_err(|_| VaultError::Kdf)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key[..])
        .map_err(|_| VaultError::Kdf)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PARAMS: KdfParams = KdfParams {
        m_cost: 64,
        t_cost: 1,
        p_cost: 1,
    };
    const SALT: [u8; SALT_LEN] = [7u8; SALT_LEN];
    const NONCE: [u8; NONCE_LEN] = [9u8; NONCE_LEN];

    fn entries(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, key)| ((*name).to_string(), (*key).to_string()))
            .collect()
    }

    fn sealed_fixture() -> Vec<u8> {
        seal(
            &entries(&[("anthropic", "sk-ant-secret"), ("openai", "sk-openai")]),
            "correct horse",
            SALT,
            NONCE,
            TEST_PARAMS,
        )
        .expect("seal")
    }

    #[test]
    fn sealing_then_opening_returns_the_same_entries() {
        struct Case {
            name: &'static str,
            entries: Vec<(&'static str, &'static str)>,
        }
        let cases = [
            Case {
                name: "empty vault",
                entries: vec![],
            },
            Case {
                name: "single key",
                entries: vec![("openai", "sk-proj-abc")],
            },
            Case {
                name: "several keys",
                entries: vec![
                    ("anthropic", "sk-ant-1"),
                    ("gemini", "AIza-2"),
                    ("openai", "sk-3"),
                ],
            },
            Case {
                name: "value with quotes and newlines",
                entries: vec![("local", "line one\nline \"two\"")],
            },
        ];
        for case in cases {
            let want = entries(&case.entries);
            let sealed = seal(&want, "pass", SALT, NONCE, TEST_PARAMS).expect(case.name);
            let got = open(&sealed, "pass").expect(case.name);
            assert_eq!(got, want, "{}", case.name);
        }
    }

    #[test]
    fn the_sealed_bytes_never_contain_the_plaintext_key() {
        let sealed = sealed_fixture();
        let haystack = String::from_utf8_lossy(&sealed).to_string();
        assert!(!haystack.contains("sk-ant-secret"), "{haystack}");
        assert!(!haystack.contains("sk-openai"), "{haystack}");
    }

    #[test]
    fn the_header_advertises_the_engine_names_without_a_passphrase() {
        assert_eq!(
            stored_names(&sealed_fixture()),
            vec!["anthropic".to_string(), "openai".to_string()]
        );
    }

    #[test]
    fn an_empty_vault_advertises_no_names() {
        let sealed = seal(&BTreeMap::new(), "pass", SALT, NONCE, TEST_PARAMS).expect("seal");
        assert!(stored_names(&sealed).is_empty());
    }

    #[test]
    fn stored_names_is_empty_for_bytes_that_are_not_a_vault() {
        struct Case {
            name: &'static str,
            bytes: Vec<u8>,
        }
        let cases = [
            Case {
                name: "empty",
                bytes: Vec::new(),
            },
            Case {
                name: "wrong magic",
                bytes: vec![0u8; HEADER_FIXED + 4],
            },
            Case {
                name: "truncated names",
                bytes: {
                    let mut bytes = sealed_fixture();
                    bytes.truncate(HEADER_FIXED + 2);
                    bytes
                },
            },
        ];
        for case in cases {
            assert!(stored_names(&case.bytes).is_empty(), "{}", case.name);
        }
    }

    #[test]
    fn opening_rejects_a_tampered_or_wrong_input() {
        struct Case {
            name: &'static str,
            passphrase: &'static str,
            mutate: fn(&mut Vec<u8>),
            want: VaultError,
        }
        let cases = [
            Case {
                name: "wrong passphrase",
                passphrase: "wrong horse",
                mutate: |_| {},
                want: VaultError::Decrypt,
            },
            Case {
                name: "flipped ciphertext byte",
                passphrase: "correct horse",
                mutate: |bytes| {
                    let last = bytes.len() - 1;
                    bytes[last] ^= 0x01;
                },
                want: VaultError::Decrypt,
            },
            Case {
                name: "tampered engine name in the header",
                passphrase: "correct horse",
                mutate: |bytes| {
                    bytes[HEADER_FIXED] = b'X';
                },
                want: VaultError::Decrypt,
            },
            Case {
                name: "tampered salt",
                passphrase: "correct horse",
                mutate: |bytes| {
                    bytes[MAGIC.len() + 1 + 12] ^= 0xff;
                },
                want: VaultError::Decrypt,
            },
            Case {
                name: "downgraded kdf cost",
                passphrase: "correct horse",
                mutate: |bytes| {
                    bytes[MAGIC.len() + 1] = 8;
                },
                want: VaultError::Decrypt,
            },
            Case {
                name: "wrong magic",
                passphrase: "correct horse",
                mutate: |bytes| bytes[0] = b'X',
                want: VaultError::BadMagic,
            },
            Case {
                name: "unknown version",
                passphrase: "correct horse",
                mutate: |bytes| bytes[MAGIC.len()] = 9,
                want: VaultError::BadVersion(9),
            },
            Case {
                name: "truncated body",
                passphrase: "correct horse",
                mutate: Vec::clear,
                want: VaultError::Truncated,
            },
            Case {
                name: "absurd memory cost",
                passphrase: "correct horse",
                mutate: |bytes| {
                    bytes[MAGIC.len() + 1..MAGIC.len() + 5]
                        .copy_from_slice(&u32::MAX.to_le_bytes());
                },
                want: VaultError::BadParams,
            },
        ];
        for case in cases {
            let mut sealed = sealed_fixture();
            (case.mutate)(&mut sealed);
            assert_eq!(
                open(&sealed, case.passphrase),
                Err(case.want),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn cutting_the_tag_short_is_reported_as_truncation() {
        let mut sealed = sealed_fixture();
        let names_end = header_len(&sealed).expect("header");
        sealed.truncate(names_end + TAG_LEN - 1);
        assert_eq!(open(&sealed, "correct horse"), Err(VaultError::Truncated));
    }

    #[test]
    fn sealing_rejects_engine_names_that_would_corrupt_the_header() {
        struct Case {
            name: &'static str,
            engine: String,
        }
        let cases = [
            Case {
                name: "embedded newline",
                engine: "open\nai".to_string(),
            },
            Case {
                name: "empty name",
                engine: String::new(),
            },
            Case {
                name: "longer than the length field",
                engine: "a".repeat(usize::from(u16::MAX) + 1),
            },
        ];
        for case in cases {
            let map = BTreeMap::from([(case.engine, "sk-1".to_string())]);
            assert_eq!(
                seal(&map, "pass", SALT, NONCE, TEST_PARAMS),
                Err(VaultError::BadName),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn sealing_rejects_key_derivation_parameters_out_of_range() {
        struct Case {
            name: &'static str,
            params: KdfParams,
        }
        let cases = [
            Case {
                name: "zero memory",
                params: KdfParams {
                    m_cost: 0,
                    t_cost: 1,
                    p_cost: 1,
                },
            },
            Case {
                name: "zero iterations",
                params: KdfParams {
                    m_cost: 64,
                    t_cost: 0,
                    p_cost: 1,
                },
            },
            Case {
                name: "zero lanes",
                params: KdfParams {
                    m_cost: 64,
                    t_cost: 1,
                    p_cost: 0,
                },
            },
            Case {
                name: "memory below eight times the lanes",
                params: KdfParams {
                    m_cost: 8,
                    t_cost: 1,
                    p_cost: 4,
                },
            },
            Case {
                name: "memory beyond the cap",
                params: KdfParams {
                    m_cost: MAX_M_COST + 1,
                    t_cost: 1,
                    p_cost: 1,
                },
            },
        ];
        for case in cases {
            assert_eq!(
                seal(&BTreeMap::new(), "pass", SALT, NONCE, case.params),
                Err(VaultError::BadParams),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn the_default_parameters_meet_the_owasp_argon2id_floor() {
        let params = KdfParams::default();
        assert!(params.m_cost >= 19_456, "{}", params.m_cost);
        assert!(params.t_cost >= 2, "{}", params.t_cost);
        assert_eq!(params.p_cost, 1);
        assert_eq!(check_params(params), Ok(()));
    }

    #[test]
    fn a_different_salt_yields_different_bytes_for_the_same_passphrase() {
        let map = entries(&[("openai", "sk-1")]);
        let first = seal(&map, "pass", SALT, NONCE, TEST_PARAMS).expect("seal");
        let second = seal(&map, "pass", [8u8; SALT_LEN], NONCE, TEST_PARAMS).expect("seal");
        assert_ne!(first, second);
        assert_eq!(open(&second, "pass").expect("open"), map);
    }

    #[test]
    fn a_passphrase_never_prints_itself() {
        let passphrase = Passphrase::new("open sesame");
        assert_eq!(format!("{passphrase:?}"), "Passphrase(***)");
        assert_eq!(passphrase.expose(), "open sesame");
    }

    #[test]
    fn masking_hides_everything_but_the_last_four_characters() {
        struct Case {
            name: &'static str,
            value: &'static str,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "empty",
                value: "",
                want: "",
            },
            Case {
                name: "shorter than the tail",
                value: "abc",
                want: "•••",
            },
            Case {
                name: "exactly the tail is still hidden",
                value: "abcd",
                want: "••••",
            },
            Case {
                name: "one past the tail",
                value: "xabcd",
                want: "•abcd",
            },
            Case {
                name: "a long key caps the dots",
                value: "sk-ant-api03-verylongsecret-4f2a",
                want: "••••••••4f2a",
            },
            Case {
                name: "multibyte characters are counted, not bytes",
                value: "ключabcd",
                want: "••••abcd",
            },
        ];
        for case in cases {
            assert_eq!(mask(case.value), case.want, "{}", case.name);
        }
    }

    #[test]
    fn masking_never_reveals_the_leading_secret() {
        let secret = "sk-ant-api03-donotleak-4f2a";
        let masked = mask(secret);
        assert!(!masked.contains("donotleak"), "{masked}");
        assert!(!masked.contains("sk-ant"), "{masked}");
    }

    #[test]
    fn keys_path_prefers_the_state_directory() {
        struct Case {
            name: &'static str,
            home: &'static str,
            xdg: Option<&'static str>,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "xdg state set",
                home: "/home/user",
                xdg: Some("/home/user/.state"),
                want: "/home/user/.state/muaddib/keys.enc",
            },
            Case {
                name: "xdg state unset",
                home: "/home/user",
                xdg: None,
                want: "/home/user/.local/state/muaddib/keys.enc",
            },
        ];
        for case in cases {
            assert_eq!(
                keys_path(Path::new(case.home), case.xdg.map(Path::new)),
                PathBuf::from(case.want),
                "{}",
                case.name
            );
        }
    }
}
