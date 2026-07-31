use crate::core::config::{Config, config_path, parse_config, to_toml};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn resolve_path_with_fallback(
    explicit_var: &str,
    xdg_var: &str,
    build: impl FnOnce(&Path, Option<&Path>) -> PathBuf,
) -> PathBuf {
    if let Some(explicit) = env::var_os(explicit_var) {
        return PathBuf::from(explicit);
    }
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let xdg = env::var_os(xdg_var).map(PathBuf::from);
    build(&home, xdg.as_deref())
}

pub fn resolve_path() -> PathBuf {
    resolve_path_with_fallback("FARO_CONFIG", "XDG_CONFIG_HOME", config_path)
}

pub fn load_or_default() -> (Config, Option<String>) {
    let path = resolve_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return (Config::default(), None);
    };
    match parse_config(&text) {
        Ok(config) => (config, None),
        Err(error) => {
            let notice = format!("ignored invalid config at {}: {error}", path.display());
            (Config::default(), Some(notice))
        }
    }
}

pub fn save(config: &Config) -> std::io::Result<()> {
    let path = resolve_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, to_toml(config))
}
