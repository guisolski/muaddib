pub mod api;
pub mod cli;

pub use crate::core::api::{ApiKey, ApiSpec, SchemaMode, Wire};
use crate::core::api::{api_available, base_url_candidates, key_env_candidates};
use crate::core::config::Config;
pub use crate::core::engine::{
    CliSpec, ENGINES, EngineError, EngineId, EngineJob, EngineOutput, EngineSpec, EngineStatus,
    NoEngineAvailable, ParseStrategy, Transport, build_args, choose_engine, engine_by_name,
    resolve_model,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub type BoxedEngineFuture<'a> =
    Pin<Box<dyn Future<Output = Result<EngineOutput, EngineError>> + Send + 'a>>;

pub type ActivitySink = tokio::sync::mpsc::Sender<crate::core::stream::EngineActivity>;

pub trait Engine: Send + Sync {
    fn name(&self) -> &str;

    fn supports_json_schema(&self) -> bool {
        false
    }

    fn has_web_tools(&self) -> bool {
        false
    }

    fn run<'a>(&'a self, job: &'a EngineJob) -> BoxedEngineFuture<'a>;

    fn run_reporting<'a>(
        &'a self,
        job: &'a EngineJob,
        activity: ActivitySink,
    ) -> BoxedEngineFuture<'a> {
        drop(activity);
        self.run(job)
    }
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
    let vaulted = crate::vault_store::names();
    ENGINES
        .iter()
        .map(|spec| match spec.transport {
            Transport::Cli(cli) => cli_status(spec, cli, config),
            Transport::Api(api) => api_status(spec, api, config, &vaulted),
        })
        .collect()
}

fn cli_status(spec: &'static EngineSpec, cli: &'static CliSpec, config: &Config) -> EngineStatus {
    let path = resolve_engine_bin(cli, config, spec.name);
    EngineStatus {
        available: path.is_some(),
        path,
        endpoint: None,
        models: spec.models.iter().map(ToString::to_string).collect(),
        key_from_env: false,
        spec,
    }
}

fn api_status(
    spec: &'static EngineSpec,
    api: &'static ApiSpec,
    config: &Config,
    vaulted: &[String],
) -> EngineStatus {
    let endpoint = resolve_base_url(api, config, spec.name);
    let key = resolve_api_key(api, config, spec.name, None);
    let models: Vec<String> = spec.models.iter().map(ToString::to_string).collect();
    let stored = has_stored_key(vaulted, spec.name);
    EngineStatus {
        available: endpoint.is_some() && api_available(api, key.as_ref(), &models, stored),
        path: None,
        endpoint,
        models,
        key_from_env: key.is_some(),
        spec,
    }
}

pub type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

fn from_process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn has_stored_key(vaulted: &[String], engine_name: &str) -> bool {
    vaulted.iter().any(|name| name == engine_name)
}

pub fn resolve_base_url(api: &ApiSpec, config: &Config, engine_name: &str) -> Option<String> {
    resolve_base_url_from(
        api,
        config.base_url_override(engine_name),
        &from_process_env,
    )
}

pub fn resolve_base_url_from(
    api: &ApiSpec,
    configured: Option<&str>,
    lookup: EnvLookup<'_>,
) -> Option<String> {
    let base = base_url_candidates(api, configured)
        .into_iter()
        .find_map(|candidate| {
            if candidate.starts_with("http") {
                Some(candidate.to_string())
            } else {
                lookup(candidate)
            }
        })
        .unwrap_or_else(|| api.base_url.to_string());
    let trimmed = base.trim().trim_end_matches('/').to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub fn resolve_api_key(
    api: &ApiSpec,
    config: &Config,
    engine_name: &str,
    unlocked: Option<&BTreeMap<String, String>>,
) -> Option<ApiKey> {
    resolve_api_key_from(
        api,
        config.api_key_env_override(engine_name),
        engine_name,
        unlocked,
        &from_process_env,
    )
}

pub fn resolve_api_key_from(
    api: &ApiSpec,
    configured: Option<&str>,
    engine_name: &str,
    unlocked: Option<&BTreeMap<String, String>>,
    lookup: EnvLookup<'_>,
) -> Option<ApiKey> {
    key_env_candidates(api, configured)
        .into_iter()
        .find_map(lookup)
        .as_deref()
        .and_then(ApiKey::new)
        .or_else(|| {
            unlocked
                .and_then(|entries| entries.get(engine_name))
                .map(String::as_str)
                .and_then(ApiKey::new)
        })
}

pub fn resolve_engine_bin(cli: &CliSpec, config: &Config, engine_name: &str) -> Option<PathBuf> {
    match config.bin_override(engine_name) {
        Some(explicit) => is_executable_file(explicit).then(|| explicit.to_path_buf()),
        None => find_executable(cli.bin),
    }
}

pub fn engine_from_status(
    status: &EngineStatus,
    config: &Config,
    fast: bool,
    unlocked: Option<&BTreeMap<String, String>>,
) -> Option<std::sync::Arc<dyn Engine>> {
    let model = resolve_model(config, status, fast);
    match status.spec.transport {
        Transport::Cli(_) => cli::CliEngine::from_status(status).map(|engine| {
            std::sync::Arc::new(engine.with_model(model)) as std::sync::Arc<dyn Engine>
        }),
        Transport::Api(spec) => api::ApiEngine::from_status(
            status,
            resolve_api_key(spec, config, status.spec.name, unlocked),
            model?,
            config.max_tokens_override(status.spec.name),
        )
        .map(|engine| std::sync::Arc::new(engine) as std::sync::Arc<dyn Engine>),
    }
}

pub fn needs_unlock(status: &EngineStatus) -> bool {
    status.available
        && !status.key_from_env
        && status
            .spec
            .api()
            .is_some_and(|api| api.auth_header.is_some())
}

pub async fn refresh_installed_models(statuses: &mut [EngineStatus]) {
    for status in statuses.iter_mut() {
        let Some(spec) = status.spec.api().filter(|spec| spec.probes_models) else {
            continue;
        };
        let Some(base) = status.endpoint.clone() else {
            continue;
        };
        status.models = api::installed_models(spec, &base).await;
        status.available = crate::core::api::api_available(spec, None, &status.models, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::engine::engine_by_name;

    fn api_of(engine: &str) -> &'static ApiSpec {
        engine_by_name(engine)
            .and_then(EngineSpec::api)
            .expect("an api engine")
    }

    fn status_of(engine: &str) -> EngineStatus {
        EngineStatus::unavailable(engine_by_name(engine).expect("an engine"))
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_process_environment_lookup_reads_real_variables() {
        assert_eq!(from_process_env("MUADDIB_DEFINITELY_UNSET_VARIABLE"), None);
        let path = from_process_env("PATH").expect("PATH is set for a test process");
        assert!(!path.is_empty(), "PATH must not come back blank");
        assert!(path.contains('/'), "PATH holds real directories: {path}");
    }

    #[test]
    fn the_vault_name_list_is_matched_exactly() {
        let vaulted = vec!["anthropic".to_string(), "openai".to_string()];
        assert!(has_stored_key(&vaulted, "anthropic"));
        assert!(has_stored_key(&vaulted, "openai"));
        assert!(!has_stored_key(&vaulted, "gemini"));
        assert!(!has_stored_key(&vaulted, "anthropi"));
        assert!(!has_stored_key(&[], "anthropic"));
    }

    #[test]
    fn the_base_url_prefers_config_then_environment_then_the_table() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            configured: Option<&'static str>,
            env: &'static [(&'static str, &'static str)],
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "the table default when nothing is set",
                engine: "openai",
                configured: None,
                env: &[],
                want: Some("https://api.openai.com"),
            },
            Case {
                name: "an environment override wins over the table",
                engine: "openai",
                configured: None,
                env: &[("OPENAI_BASE_URL", "https://env.example")],
                want: Some("https://env.example"),
            },
            Case {
                name: "config wins over the environment",
                engine: "openai",
                configured: Some("https://config.example"),
                env: &[("OPENAI_BASE_URL", "https://env.example")],
                want: Some("https://config.example"),
            },
            Case {
                name: "a trailing slash is trimmed",
                engine: "openai",
                configured: Some("https://config.example/"),
                env: &[],
                want: Some("https://config.example"),
            },
            Case {
                name: "surrounding whitespace is trimmed",
                engine: "openai",
                configured: Some("  https://config.example  "),
                env: &[],
                want: Some("https://config.example"),
            },
            Case {
                name: "a table row with no default and no environment has no endpoint",
                engine: "local",
                configured: None,
                env: &[],
                want: None,
            },
            Case {
                name: "an environment value that is only whitespace is no endpoint",
                engine: "local",
                configured: None,
                env: &[("MUADDIB_LOCAL_BASE_URL", "   ")],
                want: None,
            },
            Case {
                name: "the first environment candidate wins",
                engine: "local",
                configured: None,
                env: &[
                    ("MUADDIB_LOCAL_BASE_URL", "http://first.example"),
                    ("OPENAI_BASE_URL", "http://second.example"),
                ],
                want: Some("http://first.example"),
            },
            Case {
                name: "the second environment candidate is used when the first is unset",
                engine: "local",
                configured: None,
                env: &[("OPENAI_BASE_URL", "http://second.example")],
                want: Some("http://second.example"),
            },
        ];
        for case in cases {
            let lookup = |name: &str| {
                case.env
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_string())
            };
            assert_eq!(
                resolve_base_url_from(api_of(case.engine), case.configured, &lookup).as_deref(),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn the_api_key_prefers_the_environment_then_the_vault() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            configured: Option<&'static str>,
            env: &'static [(&'static str, &'static str)],
            vault: &'static [(&'static str, &'static str)],
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "nothing anywhere",
                engine: "anthropic",
                configured: None,
                env: &[],
                vault: &[],
                want: None,
            },
            Case {
                name: "the engine's own variable",
                engine: "anthropic",
                configured: None,
                env: &[("ANTHROPIC_API_KEY", "sk-env")],
                vault: &[],
                want: Some("sk-env"),
            },
            Case {
                name: "a configured variable name wins",
                engine: "anthropic",
                configured: Some("WORK_KEY"),
                env: &[("WORK_KEY", "sk-work"), ("ANTHROPIC_API_KEY", "sk-env")],
                vault: &[],
                want: Some("sk-work"),
            },
            Case {
                name: "the vault fills in when no variable is set",
                engine: "anthropic",
                configured: None,
                env: &[],
                vault: &[("anthropic", "sk-vaulted")],
                want: Some("sk-vaulted"),
            },
            Case {
                name: "the environment beats the vault",
                engine: "anthropic",
                configured: None,
                env: &[("ANTHROPIC_API_KEY", "sk-env")],
                vault: &[("anthropic", "sk-vaulted")],
                want: Some("sk-env"),
            },
            Case {
                name: "another engine's vault entry is not borrowed",
                engine: "anthropic",
                configured: None,
                env: &[],
                vault: &[("openai", "sk-other")],
                want: None,
            },
        ];
        for case in cases {
            let lookup = |name: &str| {
                case.env
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_string())
            };
            let vault: BTreeMap<String, String> = case
                .vault
                .iter()
                .map(|(name, key)| ((*name).to_string(), (*key).to_string()))
                .collect();
            assert_eq!(
                resolve_api_key_from(
                    api_of(case.engine),
                    case.configured,
                    case.engine,
                    Some(&vault),
                    &lookup
                )
                .as_ref()
                .map(ApiKey::expose),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn a_blank_environment_value_falls_through_to_the_vault() {
        let vault = BTreeMap::from([("anthropic".to_string(), "sk-vaulted".to_string())]);
        let blank = |_: &str| Some("   ".to_string());
        assert_eq!(
            resolve_api_key_from(api_of("anthropic"), None, "anthropic", Some(&vault), &blank)
                .as_ref()
                .map(ApiKey::expose),
            Some("sk-vaulted")
        );
    }

    #[test]
    fn gemini_falls_back_to_the_google_variable() {
        let google = |name: &str| (name == "GOOGLE_API_KEY").then(|| "sk-google".to_string());
        assert_eq!(
            resolve_api_key_from(api_of("gemini"), None, "gemini", None, &google)
                .as_ref()
                .map(ApiKey::expose),
            Some("sk-google")
        );
    }

    #[test]
    fn the_wrappers_read_the_config_before_the_process_environment() {
        let config = crate::core::config::parse_config(
            "[engines.openai]\nbase_url = \"https://proxy.example/\"",
        )
        .expect("config parses");
        assert_eq!(
            resolve_base_url(api_of("openai"), &config, "openai"),
            Some("https://proxy.example".to_string())
        );
        let vault = BTreeMap::from([("ollama".to_string(), "sk-vaulted".to_string())]);
        assert_eq!(
            resolve_api_key(api_of("ollama"), &config, "ollama", Some(&vault))
                .as_ref()
                .map(ApiKey::expose),
            Some("sk-vaulted")
        );
        assert_eq!(
            resolve_api_key(api_of("ollama"), &config, "ollama", None),
            None
        );
    }

    #[test]
    fn only_an_available_keyed_engine_whose_key_is_not_in_the_environment_needs_unlocking() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            available: bool,
            key_from_env: bool,
            want: bool,
        }
        let cases = [
            Case {
                name: "a vaulted key must be unlocked",
                engine: "anthropic",
                available: true,
                key_from_env: false,
                want: true,
            },
            Case {
                name: "an environment key needs nothing",
                engine: "anthropic",
                available: true,
                key_from_env: true,
                want: false,
            },
            Case {
                name: "an unavailable engine is not worth unlocking",
                engine: "anthropic",
                available: false,
                key_from_env: false,
                want: false,
            },
            Case {
                name: "a keyless local engine never asks",
                engine: "ollama",
                available: true,
                key_from_env: false,
                want: false,
            },
            Case {
                name: "a cli engine never asks",
                engine: "claude",
                available: true,
                key_from_env: false,
                want: false,
            },
        ];
        for case in cases {
            let mut status = status_of(case.engine);
            status.available = case.available;
            status.key_from_env = case.key_from_env;
            assert_eq!(needs_unlock(&status), case.want, "{}", case.name);
        }
    }
    #[tokio::test]
    async fn probing_replaces_the_curated_models_with_the_installed_ones() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let body = r#"{"models":[{"name":"qwen3:8b"},{"name":"llama3.2:3b"}]}"#;
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0_u8; 8192];
            let _ = stream.read(&mut buffer).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;
        });

        let mut ollama = status_of("ollama");
        ollama.endpoint = Some(base);
        ollama.available = false;
        let mut claude = status_of("claude");
        claude.models = vec!["opus".to_string()];
        let mut unreachable = status_of("local");
        unreachable.endpoint = None;
        unreachable.models = vec!["untouched".to_string()];

        let mut statuses = vec![ollama, claude, unreachable];
        refresh_installed_models(&mut statuses).await;

        assert_eq!(
            statuses[0].models,
            vec!["qwen3:8b".to_string(), "llama3.2:3b".to_string()]
        );
        assert!(statuses[0].available, "a probed daemon becomes available");
        assert_eq!(
            statuses[1].models,
            vec!["opus".to_string()],
            "cli engines are left alone"
        );
        assert_eq!(
            statuses[2].models,
            vec!["untouched".to_string()],
            "an engine with no endpoint is left alone"
        );
    }
    #[tokio::test]
    async fn a_probe_that_finds_nothing_marks_the_engine_unavailable() {
        let mut ollama = status_of("ollama");
        ollama.endpoint = Some("http://127.0.0.1:1".to_string());
        ollama.available = true;
        ollama.models = vec!["stale".to_string()];
        let mut statuses = vec![ollama];
        refresh_installed_models(&mut statuses).await;
        assert!(statuses[0].models.is_empty());
        assert!(!statuses[0].available);
    }

    #[test]
    fn resolving_an_absent_key_from_the_real_environment_is_none() {
        let spec = api_of("anthropic");
        assert_eq!(
            resolve_api_key_from(spec, Some("MUADDIB_ABSENT_KEY"), "anthropic", None, &no_env),
            None
        );
    }

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

    struct BareEngine;

    impl Engine for BareEngine {
        fn name(&self) -> &'static str {
            "bare"
        }

        fn run<'a>(&'a self, _job: &'a EngineJob) -> BoxedEngineFuture<'a> {
            Box::pin(async { Err(EngineError::Reported("unused".to_string())) })
        }
    }

    #[test]
    fn the_default_engine_trait_impl_does_not_claim_web_tools() {
        assert!(!BareEngine.has_web_tools());
    }
}
