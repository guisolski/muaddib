use crate::core::engine::{CliSpec, build_args, envelope_output};
use crate::core::stream::activities_in;
use crate::engines::{
    ActivitySink, BoxedEngineFuture, Engine, EngineError, EngineJob, EngineOutput, EngineSpec,
    EngineStatus,
};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

pub struct CliEngine {
    spec: &'static EngineSpec,
    cli: &'static CliSpec,
    bin: PathBuf,
    model: Option<String>,
}

impl CliEngine {
    pub fn new(spec: &'static EngineSpec, cli: &'static CliSpec, bin: PathBuf) -> Self {
        Self {
            spec,
            cli,
            bin,
            model: None,
        }
    }

    pub fn from_status(status: &EngineStatus) -> Option<Self> {
        let cli = status.spec.cli()?;
        let bin = status.path.clone()?;
        Some(Self::new(status.spec, cli, bin))
    }

    #[must_use]
    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model = model;
        self
    }

    async fn run_cli(
        &self,
        job: &EngineJob,
        activity: Option<ActivitySink>,
    ) -> Result<EngineOutput, EngineError> {
        let stdout = tokio::time::timeout(job.timeout, self.capture(job, activity))
            .await
            .map_err(|_| EngineError::TimedOut(job.timeout))??;
        envelope_output(self.cli.parse, &stdout)
    }

    async fn capture(
        &self,
        job: &EngineJob,
        activity: Option<ActivitySink>,
    ) -> Result<String, EngineError> {
        let mut child = self
            .command(job)
            .spawn()
            .map_err(|error| EngineError::Spawn(error.to_string()))?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let narrate = self.cli.streams.then_some(activity).flatten();
        let (stdout, stderr) = tokio::join!(read_stdout(stdout, narrate), read_all(stderr));
        let status = child
            .wait()
            .await
            .map_err(|error| EngineError::Spawn(error.to_string()))?;
        if !status.success() {
            return Err(EngineError::Failed {
                status: status.code().unwrap_or(-1),
                stderr_tail: tail(&stderr, 400),
            });
        }
        Ok(stdout)
    }

    fn command(&self, job: &EngineJob) -> Command {
        let mut command = Command::new(&self.bin);
        command
            .args(build_args(self.spec, self.cli, self.model.as_deref(), job))
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

    fn has_web_tools(&self) -> bool {
        self.cli.web_tools
    }

    fn run<'a>(&'a self, job: &'a EngineJob) -> BoxedEngineFuture<'a> {
        Box::pin(self.run_cli(job, None))
    }

    fn run_reporting<'a>(
        &'a self,
        job: &'a EngineJob,
        activity: ActivitySink,
    ) -> BoxedEngineFuture<'a> {
        Box::pin(self.run_cli(job, Some(activity)))
    }
}

async fn read_stdout<R: AsyncRead + Unpin>(reader: R, activity: Option<ActivitySink>) -> String {
    let Some(sink) = activity else {
        return read_all(reader).await;
    };
    let mut lines = BufReader::new(reader).lines();
    let mut collected = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        for reported in activities_in(&line) {
            let _ = sink.try_send(reported);
        }
        collected.push_str(&line);
        collected.push('\n');
    }
    collected
}

async fn read_all<R: AsyncRead + Unpin>(reader: R) -> String {
    let mut buffer = Vec::new();
    let mut reader = BufReader::new(reader);
    let _ = tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buffer).await;
    String::from_utf8_lossy(&buffer).into_owned()
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
    use crate::core::engine::{ParseStrategy, Transport};
    use crate::core::stream::EngineActivity;
    use crate::engines::EngineId;
    use std::time::Duration;

    const ECHO_CLI: CliSpec = CliSpec {
        bin: "echo",
        args: &[],
        streams: false,
        parse: ParseStrategy::RawText,
        model_flag: None,
        web_tools: false,
    };

    const ECHO_SPEC: EngineSpec = EngineSpec {
        prices: &[],
        id: EngineId::Opencode,
        name: "echo-fake",
        transport: Transport::Cli(&ECHO_CLI),
        supports_json_schema: false,
        models: &[],
        fast_model: None,
        auto_select: true,
        missing_label: "",
        install_hint: "",
    };

    const FAILING_CLI: CliSpec = CliSpec {
        bin: "sh",
        args: &["-c", "echo boom >&2; exit 3"],
        streams: false,
        parse: ParseStrategy::RawText,
        model_flag: None,
        web_tools: false,
    };

    const FAILING_SPEC: EngineSpec = EngineSpec {
        prices: &[],
        id: EngineId::Opencode,
        name: "failing-fake",
        transport: Transport::Cli(&FAILING_CLI),
        supports_json_schema: false,
        models: &[],
        fast_model: None,
        auto_select: true,
        missing_label: "",
        install_hint: "",
    };

    const SLEEPING_CLI: CliSpec = CliSpec {
        bin: "sh",
        args: &["-c", "sleep 5"],
        streams: false,
        parse: ParseStrategy::RawText,
        model_flag: None,
        web_tools: false,
    };

    const SLEEPING_SPEC: EngineSpec = EngineSpec {
        prices: &[],
        id: EngineId::Opencode,
        name: "sleeping-fake",
        transport: Transport::Cli(&SLEEPING_CLI),
        supports_json_schema: false,
        models: &[],
        fast_model: None,
        auto_select: true,
        missing_label: "",
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
        let engine = CliEngine::new(&ECHO_SPEC, &ECHO_CLI, PathBuf::from("/bin/echo"));
        let output = engine.run(&job("hello world", 5_000)).await.unwrap();
        assert_eq!(output.text.trim(), "hello world");
    }

    const MODEL_ECHO_CLI: CliSpec = CliSpec {
        bin: "echo",
        args: &[],
        streams: false,
        parse: ParseStrategy::RawText,
        model_flag: Some("--model"),
        web_tools: false,
    };

    const MODEL_ECHO_SPEC: EngineSpec = EngineSpec {
        prices: &[],
        id: EngineId::Opencode,
        name: "model-echo-fake",
        transport: Transport::Cli(&MODEL_ECHO_CLI),
        supports_json_schema: false,
        models: &["fast"],
        fast_model: None,
        auto_select: true,
        missing_label: "",
        install_hint: "",
    };

    #[tokio::test]
    async fn run_forwards_the_configured_model_to_the_process() {
        let engine = CliEngine::new(
            &MODEL_ECHO_SPEC,
            &MODEL_ECHO_CLI,
            PathBuf::from("/bin/echo"),
        )
        .with_model(Some("fast".to_string()));
        let output = engine.run(&job("hello", 5_000)).await.unwrap();
        assert_eq!(output.text.trim(), "--model fast hello");
    }

    #[tokio::test]
    async fn run_reports_exit_status_and_stderr_tail() {
        let engine = CliEngine::new(&FAILING_SPEC, &FAILING_CLI, PathBuf::from("/bin/sh"));
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
        let engine = CliEngine::new(&SLEEPING_SPEC, &SLEEPING_CLI, PathBuf::from("/bin/sh"));
        let error = engine.run(&job("ignored", 50)).await.unwrap_err();
        assert!(matches!(error, EngineError::TimedOut(_)));
    }

    #[tokio::test]
    async fn run_reports_spawn_failures_for_missing_binaries() {
        let engine = CliEngine::new(&ECHO_SPEC, &ECHO_CLI, PathBuf::from("/nonexistent/binary"));
        let error = engine.run(&job("ignored", 5_000)).await.unwrap_err();
        assert!(matches!(error, EngineError::Spawn(_)));
    }

    const STREAM_LINES: &str = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\"}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":",
        "[{\"type\":\"tool_use\",\"name\":\"WebSearch\",\"input\":{\"query\":\"rust async\"}}]}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":",
        "[{\"type\":\"tool_use\",\"name\":\"WebFetch\",\"input\":{\"url\":\"https://a.example\"}}]}}\n",
        "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\"}\n"
    );

    const STREAMING_CLI: CliSpec = CliSpec {
        bin: "sh",
        args: &["-c"],
        streams: true,
        parse: ParseStrategy::ClaudeJson,
        model_flag: None,
        web_tools: true,
    };

    const STREAMING_SPEC: EngineSpec = EngineSpec {
        prices: &[],
        id: EngineId::Claude,
        name: "streaming-fake",
        transport: Transport::Cli(&STREAMING_CLI),
        supports_json_schema: false,
        models: &[],
        fast_model: None,
        auto_select: true,
        missing_label: "",
        install_hint: "",
    };

    const QUIET_CLI: CliSpec = CliSpec {
        bin: "sh",
        args: &["-c"],
        streams: false,
        parse: ParseStrategy::ClaudeJson,
        model_flag: None,
        web_tools: true,
    };

    const QUIET_SPEC: EngineSpec = EngineSpec {
        prices: &[],
        id: EngineId::Claude,
        name: "quiet-fake",
        transport: Transport::Cli(&QUIET_CLI),
        supports_json_schema: false,
        models: &[],
        fast_model: None,
        auto_select: true,
        missing_label: "",
        install_hint: "",
    };

    async fn drain_activity(
        spec: &'static EngineSpec,
        cli: &'static CliSpec,
    ) -> (Result<EngineOutput, EngineError>, Vec<EngineActivity>) {
        let engine = CliEngine::new(spec, cli, PathBuf::from("/bin/sh"));
        let (activity_tx, mut activity_rx) = tokio::sync::mpsc::channel(16);
        let script = format!("printf '%s' '{STREAM_LINES}'");
        let job = job(&script, 5_000);
        let collect = async {
            let mut seen = Vec::new();
            while let Some(reported) = activity_rx.recv().await {
                seen.push(reported);
            }
            seen
        };
        let (result, seen) = tokio::join!(engine.run_reporting(&job, activity_tx), collect);
        (result, seen)
    }

    #[tokio::test]
    async fn a_streaming_engine_narrates_its_tool_calls_and_still_returns_the_envelope() {
        let (result, seen) = drain_activity(&STREAMING_SPEC, &STREAMING_CLI).await;
        assert_eq!(result.unwrap().text, "done");
        assert_eq!(
            seen,
            vec![
                EngineActivity {
                    label: "searching",
                    target: "rust async".to_string(),
                },
                EngineActivity {
                    label: "reading",
                    target: "https://a.example".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn an_engine_that_does_not_stream_narrates_nothing() {
        let (result, seen) = drain_activity(&QUIET_SPEC, &QUIET_CLI).await;
        assert_eq!(result.unwrap().text, "done");
        assert!(seen.is_empty());
    }

    #[tokio::test]
    async fn a_full_activity_channel_drops_lines_instead_of_stalling_the_engine() {
        let engine = CliEngine::new(&STREAMING_SPEC, &STREAMING_CLI, PathBuf::from("/bin/sh"));
        let (activity_tx, activity_rx) = tokio::sync::mpsc::channel(1);
        let script = format!("printf '%s' '{}'", STREAM_LINES.repeat(20));
        let output = engine
            .run_reporting(&job(&script, 5_000), activity_tx)
            .await
            .unwrap();
        assert_eq!(output.text, "done");
        drop(activity_rx);
    }

    #[test]
    fn tail_keeps_only_the_last_characters() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
        assert_eq!(tail("  spaced  ", 20), "spaced");
    }
}
