pub mod cli;

use crate::core::config::Config;
pub use crate::core::engine::{
    ENGINES, EngineError, EngineId, EngineJob, EngineOutput, EngineSpec, EngineStatus,
    NoEngineAvailable, ParseStrategy, build_args, choose_engine, engine_by_name,
};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub type BoxedEngineFuture<'a> =
    Pin<Box<dyn Future<Output = Result<EngineOutput, EngineError>> + Send + 'a>>;

pub trait Engine: Send + Sync {
    fn name(&self) -> &str;

    fn supports_json_schema(&self) -> bool {
        false
    }

    fn run<'a>(&'a self, job: &'a EngineJob) -> BoxedEngineFuture<'a>;
}

pub fn candidate_paths(bin: &str, path_var: &str) -> Vec<PathBuf> {
    std::env::split_paths(path_var)
        .map(|dir| dir.join(bin))
        .collect()
}

pub fn find_executable(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(bin))
        .find(|candidate| is_executable_file(candidate))
}

pub fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

pub fn detect_engines(config: &Config) -> Vec<EngineStatus> {
    ENGINES
        .iter()
        .map(|spec| {
            let path = resolve_engine_bin(spec, config);
            EngineStatus {
                spec,
                available: path.is_some(),
                path,
            }
        })
        .collect()
}

pub fn resolve_engine_bin(spec: &'static EngineSpec, config: &Config) -> Option<PathBuf> {
    match config.bin_override(spec.name) {
        Some(explicit) => is_executable_file(explicit).then(|| explicit.to_path_buf()),
        None => find_executable(spec.bin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_paths_follows_path_variable_order() {
        let candidates = candidate_paths("tool", "/usr/local/bin:/opt/bin");
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/usr/local/bin/tool"),
                PathBuf::from("/opt/bin/tool"),
            ]
        );
    }
}
