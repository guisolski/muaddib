use crate::engines::parse::envelope_text;
use crate::engines::{
    BoxedEngineFuture, Engine, EngineError, EngineJob, EngineOutput, EngineSpec, EngineStatus,
    build_args,
};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

pub struct CliEngine {
    spec: &'static EngineSpec,
    bin: PathBuf,
}

impl CliEngine {
    pub fn new(spec: &'static EngineSpec, bin: PathBuf) -> Self {
        Self { spec, bin }
    }

    pub fn from_status(status: &EngineStatus) -> Option<Self> {
        status.path.clone().map(|bin| Self::new(status.spec, bin))
    }

    async fn run_cli(&self, job: &EngineJob) -> Result<EngineOutput, EngineError> {
        let output = tokio::time::timeout(job.timeout, self.command(job).output())
            .await
            .map_err(|_| EngineError::TimedOut(job.timeout))?
            .map_err(|error| EngineError::Spawn(error.to_string()))?;
        if !output.status.success() {
            return Err(failure_from_output(&output));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        envelope_text(self.spec.parse, &stdout).map(|text| EngineOutput { text })
    }

    fn command(&self, job: &EngineJob) -> Command {
        let mut command = Command::new(&self.bin);
        command
            .args(build_args(self.spec, job))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }
}

impl Engine for CliEngine {
    fn name(&self) -> &str {
        self.spec.name
    }

    fn supports_json_schema(&self) -> bool {
        self.spec.supports_json_schema
    }

    fn run<'a>(&'a self, job: &'a EngineJob) -> BoxedEngineFuture<'a> {
        Box::pin(self.run_cli(job))
    }
}

fn failure_from_output(output: &std::process::Output) -> EngineError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    EngineError::Failed {
        status: output.status.code().unwrap_or(-1),
        stderr_tail: tail(&stderr, 400),
    }
}

fn tail(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let total = trimmed.chars().count();
    trimmed
        .chars()
        .skip(total.saturating_sub(max_chars))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::EngineId;
    use crate::engines::parse::ParseStrategy;
    use std::time::Duration;

    const ECHO_SPEC: EngineSpec = EngineSpec {
        id: EngineId::Opencode,
        name: "echo-fake",
        bin: "echo",
        args: &[],
        parse: ParseStrategy::RawText,
        supports_json_schema: false,
        install_hint: "",
    };

    const FAILING_SPEC: EngineSpec = EngineSpec {
        id: EngineId::Opencode,
        name: "failing-fake",
        bin: "sh",
        args: &["-c", "echo boom >&2; exit 3"],
        parse: ParseStrategy::RawText,
        supports_json_schema: false,
        install_hint: "",
    };

    const SLEEPING_SPEC: EngineSpec = EngineSpec {
        id: EngineId::Opencode,
        name: "sleeping-fake",
        bin: "sh",
        args: &["-c", "sleep 5"],
        parse: ParseStrategy::RawText,
        supports_json_schema: false,
        install_hint: "",
    };

    fn job(prompt: &str, timeout_ms: u64) -> EngineJob {
        EngineJob {
            prompt: prompt.to_string(),
            schema: None,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    #[tokio::test]
    async fn run_captures_stdout_of_a_successful_process() {
        let engine = CliEngine::new(&ECHO_SPEC, PathBuf::from("/bin/echo"));
        let output = engine.run(&job("hello world", 5_000)).await.unwrap();
        assert_eq!(output.text.trim(), "hello world");
    }

    #[tokio::test]
    async fn run_reports_exit_status_and_stderr_tail() {
        let engine = CliEngine::new(&FAILING_SPEC, PathBuf::from("/bin/sh"));
        let error = engine.run(&job("ignored", 5_000)).await.unwrap_err();
        assert_eq!(
            error,
            EngineError::Failed {
                status: 3,
                stderr_tail: "boom".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn run_times_out_and_kills_slow_processes() {
        let engine = CliEngine::new(&SLEEPING_SPEC, PathBuf::from("/bin/sh"));
        let error = engine.run(&job("ignored", 50)).await.unwrap_err();
        assert!(matches!(error, EngineError::TimedOut(_)));
    }

    #[tokio::test]
    async fn run_reports_spawn_failures_for_missing_binaries() {
        let engine = CliEngine::new(&ECHO_SPEC, PathBuf::from("/nonexistent/binary"));
        let error = engine.run(&job("ignored", 5_000)).await.unwrap_err();
        assert!(matches!(error, EngineError::Spawn(_)));
    }

    #[test]
    fn tail_keeps_only_the_last_characters() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
        assert_eq!(tail("  spaced  ", 20), "spaced");
    }
}
