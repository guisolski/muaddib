pub mod cli;
pub mod parse;

use crate::core::config::Config;
use parse::ParseStrategy;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineId {
    Claude,
    CursorAgent,
    Codex,
    Opencode,
}

#[derive(Debug)]
pub struct EngineSpec {
    pub id: EngineId,
    pub name: &'static str,
    pub bin: &'static str,
    pub args: &'static [&'static str],
    pub parse: ParseStrategy,
    pub supports_json_schema: bool,
    pub model_flag: Option<&'static str>,
    pub models: &'static [&'static str],
    pub fast_model: Option<&'static str>,
    pub install_hint: &'static str,
}

pub const ENGINES: &[EngineSpec] = &[
    EngineSpec {
        id: EngineId::Claude,
        name: "claude",
        bin: "claude",
        args: &[
            "-p",
            "--output-format",
            "json",
            "--allowedTools=WebSearch,WebFetch",
        ],
        parse: ParseStrategy::ClaudeJson,
        supports_json_schema: true,
        model_flag: Some("--model"),
        models: &["opus", "sonnet", "haiku"],
        fast_model: Some("haiku"),
        install_hint: "npm install -g @anthropic-ai/claude-code",
    },
    EngineSpec {
        id: EngineId::CursorAgent,
        name: "cursor-agent",
        bin: "cursor-agent",
        args: &["-p", "--output-format", "json"],
        parse: ParseStrategy::GenericJson,
        supports_json_schema: false,
        model_flag: Some("--model"),
        models: &["auto", "gpt-5", "sonnet-4.5"],
        fast_model: None,
        install_hint: "curl https://cursor.com/install -fsS | bash",
    },
    EngineSpec {
        id: EngineId::Codex,
        name: "codex",
        bin: "codex",
        args: &["exec", "--skip-git-repo-check"],
        parse: ParseStrategy::RawText,
        supports_json_schema: false,
        model_flag: Some("--model"),
        models: &["gpt-5-codex", "gpt-5"],
        fast_model: None,
        install_hint: "npm install -g @openai/codex",
    },
    EngineSpec {
        id: EngineId::Opencode,
        name: "opencode",
        bin: "opencode",
        args: &["run"],
        parse: ParseStrategy::RawText,
        supports_json_schema: false,
        model_flag: Some("--model"),
        models: &["anthropic/claude-sonnet-4-5", "openai/gpt-5"],
        fast_model: None,
        install_hint: "npm install -g opencode-ai",
    },
];

pub fn engine_by_name(name: &str) -> Option<&'static EngineSpec> {
    ENGINES.iter().find(|spec| spec.name == name)
}

#[derive(Debug, Clone)]
pub struct EngineJob {
    pub prompt: String,
    pub schema: Option<&'static str>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOutput {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("engine timed out after {0:?}")]
    TimedOut(Duration),
    #[error("engine exited with status {status}: {stderr_tail}")]
    Failed { status: i32, stderr_tail: String },
    #[error("engine reported an error: {0}")]
    Reported(String),
    #[error("failed to spawn engine: {0}")]
    Spawn(String),
}

pub type BoxedEngineFuture<'a> =
    Pin<Box<dyn Future<Output = Result<EngineOutput, EngineError>> + Send + 'a>>;

pub trait Engine: Send + Sync {
    fn name(&self) -> &str;

    fn supports_json_schema(&self) -> bool {
        false
    }

    fn run<'a>(&'a self, job: &'a EngineJob) -> BoxedEngineFuture<'a>;
}

pub fn build_args(spec: &EngineSpec, model: Option<&str>, job: &EngineJob) -> Vec<String> {
    let mut args: Vec<String> = spec.args.iter().map(ToString::to_string).collect();
    if let (Some(flag), Some(model)) = (spec.model_flag, model) {
        args.push(flag.to_string());
        args.push(model.to_string());
    }
    if spec.supports_json_schema
        && let Some(schema) = job.schema
    {
        args.push("--json-schema".to_string());
        args.push(schema.to_string());
    }
    args.push(job.prompt.clone());
    args
}

#[derive(Debug, Clone)]
pub struct EngineStatus {
    pub spec: &'static EngineSpec,
    pub available: bool,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no supported engine CLI is installed")]
pub struct NoEngineAvailable;

pub fn choose_engine<'a>(
    statuses: &'a [EngineStatus],
    requested: &str,
) -> Result<(&'a EngineStatus, Option<String>), NoEngineAvailable> {
    let exact = statuses
        .iter()
        .find(|status| status.spec.name == requested && status.available);
    if let Some(status) = exact {
        return Ok((status, None));
    }
    let fallback = statuses
        .iter()
        .find(|status| status.available)
        .ok_or(NoEngineAvailable)?;
    let notice = format!(
        "engine '{requested}' is not available; using '{}'",
        fallback.spec.name
    );
    Ok((fallback, Some(notice)))
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
    use std::collections::BTreeSet;

    fn job(prompt: &str, schema: Option<&'static str>) -> EngineJob {
        EngineJob {
            prompt: prompt.to_string(),
            schema,
            timeout: Duration::from_secs(1),
        }
    }

    fn status(spec: &'static EngineSpec, available: bool) -> EngineStatus {
        EngineStatus {
            spec,
            available,
            path: available.then(|| PathBuf::from("/fake/bin")),
        }
    }

    #[test]
    fn engines_table_names_are_unique_and_resolvable() {
        let names: BTreeSet<&str> = ENGINES.iter().map(|spec| spec.name).collect();
        assert_eq!(names.len(), ENGINES.len());
        for spec in ENGINES {
            assert_eq!(engine_by_name(spec.name).unwrap().id, spec.id);
            assert!(!spec.install_hint.is_empty(), "{}", spec.name);
        }
    }

    #[test]
    fn build_args_places_the_prompt_last() {
        for spec in ENGINES {
            let args = build_args(spec, Some("some-model"), &job("the prompt", None));
            assert_eq!(
                args.last().map(String::as_str),
                Some("the prompt"),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn build_args_adds_the_model_flag_only_when_a_model_is_set() {
        for spec in ENGINES {
            let with_model = build_args(spec, Some("some-model"), &job("p", None));
            let without_model = build_args(spec, None, &job("p", None));
            assert!(
                with_model.windows(2).any(|pair| {
                    pair[0] == spec.model_flag.unwrap_or_default() && pair[1] == "some-model"
                }),
                "{}",
                spec.name
            );
            assert!(
                !without_model.iter().any(|arg| arg == "some-model"),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn build_args_adds_schema_flag_only_for_supporting_engines() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            schema: Option<&'static str>,
            want_flag: bool,
        }
        let cases = [
            Case {
                name: "claude with schema",
                engine: "claude",
                schema: Some("{}"),
                want_flag: true,
            },
            Case {
                name: "claude without schema",
                engine: "claude",
                schema: None,
                want_flag: false,
            },
            Case {
                name: "codex with schema",
                engine: "codex",
                schema: Some("{}"),
                want_flag: false,
            },
        ];
        for case in cases {
            let spec = engine_by_name(case.engine).unwrap();
            let args = build_args(spec, None, &job("p", case.schema));
            assert_eq!(
                args.iter().any(|arg| arg == "--json-schema"),
                case.want_flag,
                "{}",
                case.name
            );
        }
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

    #[test]
    fn choose_engine_prefers_the_requested_available_engine() {
        let statuses = vec![status(&ENGINES[0], true), status(&ENGINES[1], true)];
        let (chosen, notice) = choose_engine(&statuses, "cursor-agent").unwrap();
        assert_eq!(chosen.spec.name, "cursor-agent");
        assert!(notice.is_none());
    }

    #[test]
    fn choose_engine_falls_back_to_first_available_with_notice() {
        let statuses = vec![status(&ENGINES[0], false), status(&ENGINES[1], true)];
        let (chosen, notice) = choose_engine(&statuses, "claude").unwrap();
        assert_eq!(chosen.spec.name, "cursor-agent");
        assert!(notice.unwrap().contains("claude"));
    }

    #[test]
    fn choose_engine_errors_when_nothing_is_installed() {
        let statuses = vec![status(&ENGINES[0], false), status(&ENGINES[1], false)];
        assert_eq!(
            choose_engine(&statuses, "claude").unwrap_err(),
            NoEngineAvailable
        );
    }
}
