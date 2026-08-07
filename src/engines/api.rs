use crate::core::api::{
    ApiCall, ApiKey, ApiSpec, endpoint, failure, models_endpoint, models_from_tags, redact,
    request_body, request_headers, response_text, response_usage, retry_delay,
};
use crate::core::stream::{ASKING_LABEL, EngineActivity, RETRY_LABEL};
use crate::engines::{
    ActivitySink, BoxedEngineFuture, Engine, EngineError, EngineJob, EngineOutput, EngineSpec,
    EngineStatus,
};
use serde_json::Value;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const TRANSPORT_TAIL_CHARS: usize = 200;

pub struct ApiEngine {
    spec: &'static EngineSpec,
    api: &'static ApiSpec,
    client: reqwest::Client,
    endpoint: String,
    model: String,
    key: Option<ApiKey>,
    max_tokens: u32,
}

struct Attempt {
    status: u16,
    retry_after: Option<String>,
    body: String,
}

pub fn build_api_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("muaddib/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()
}

impl ApiEngine {
    pub fn new(
        spec: &'static EngineSpec,
        api: &'static ApiSpec,
        base_url: &str,
        model: String,
        key: Option<ApiKey>,
        max_tokens: u32,
    ) -> Option<Self> {
        Some(Self {
            spec,
            api,
            client: build_api_client()?,
            endpoint: endpoint(api, base_url, &model),
            model,
            key,
            max_tokens,
        })
    }

    pub fn from_status(
        status: &EngineStatus,
        key: Option<ApiKey>,
        model: String,
        max_tokens: Option<u32>,
    ) -> Option<Self> {
        let api = status.spec.api()?;
        let base_url = status.endpoint.as_deref()?;
        Self::new(
            status.spec,
            api,
            base_url,
            model,
            key,
            max_tokens.unwrap_or(api.default_max_tokens),
        )
    }

    async fn send(&self, job: &EngineJob) -> Result<Attempt, EngineError> {
        let call = ApiCall {
            spec: self.api,
            model: &self.model,
            prompt: &job.prompt,
            schema: job.schema,
            max_tokens: self.max_tokens,
        };
        let body = request_body(&call).to_string();
        let request = request_headers(self.api, self.key.as_ref())
            .into_iter()
            .fold(
                self.client.post(&self.endpoint),
                |request, (name, value)| request.header(name, value),
            );
        let response = request
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| self.transport_error(&error, job))?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string);
        let body = response
            .text()
            .await
            .map_err(|error| self.transport_error(&error, job))?;
        Ok(Attempt {
            status,
            retry_after,
            body,
        })
    }

    fn transport_error(&self, error: &reqwest::Error, job: &EngineJob) -> EngineError {
        if error.is_timeout() {
            return EngineError::TimedOut(job.timeout);
        }
        EngineError::Spawn(redact(
            &error.to_string(),
            self.key.as_ref(),
            TRANSPORT_TAIL_CHARS,
        ))
    }

    fn finish(&self, body: &str) -> Result<EngineOutput, EngineError> {
        let parsed = serde_json::from_str::<Value>(body).map_err(|_| {
            EngineError::Reported("the endpoint returned a non-JSON body".to_string())
        })?;
        Ok(EngineOutput {
            text: response_text(self.api.wire, &parsed)?,
            usage: response_usage(self.api.wire, &parsed, self.spec.prices, &self.model),
        })
    }

    async fn attempts(
        &self,
        job: &EngineJob,
        activity: Option<&ActivitySink>,
    ) -> Result<EngineOutput, EngineError> {
        report(activity, ASKING_LABEL, self.model.clone());
        let mut attempt = 0;
        loop {
            let outcome = self.send(job).await?;
            if outcome.status < 400 {
                return self.finish(&outcome.body);
            }
            let Some(delay) = retry_delay(attempt, outcome.status, outcome.retry_after.as_deref())
            else {
                return Err(failure(outcome.status, &outcome.body, self.key.as_ref()));
            };
            report(
                activity,
                RETRY_LABEL,
                format!("{} in {}s", self.model, delay.as_secs()),
            );
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }

    async fn run_api(
        &self,
        job: &EngineJob,
        activity: Option<ActivitySink>,
    ) -> Result<EngineOutput, EngineError> {
        tokio::time::timeout(job.timeout, self.attempts(job, activity.as_ref()))
            .await
            .map_err(|_| EngineError::TimedOut(job.timeout))?
    }
}

fn report(activity: Option<&ActivitySink>, label: &'static str, target: String) {
    if let Some(sink) = activity {
        let _ = sink.try_send(EngineActivity { label, target });
    }
}

impl Engine for ApiEngine {
    fn name(&self) -> &str {
        self.spec.name
    }

    fn supports_json_schema(&self) -> bool {
        self.spec.supports_json_schema
    }

    fn run<'a>(&'a self, job: &'a EngineJob) -> BoxedEngineFuture<'a> {
        Box::pin(self.run_api(job, None))
    }

    fn run_reporting<'a>(
        &'a self,
        job: &'a EngineJob,
        activity: ActivitySink,
    ) -> BoxedEngineFuture<'a> {
        Box::pin(self.run_api(job, Some(activity)))
    }
}

pub async fn installed_models(api: &ApiSpec, base_url: &str) -> Vec<String> {
    let Some(client) = build_api_client() else {
        return Vec::new();
    };
    let url = models_endpoint(api, base_url);
    let Ok(Ok(response)) = tokio::time::timeout(PROBE_TIMEOUT, client.get(&url).send()).await
    else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(Ok(body)) = tokio::time::timeout(PROBE_TIMEOUT, response.text()).await else {
        return Vec::new();
    };
    serde_json::from_str::<Value>(&body)
        .map(|parsed| models_from_tags(&parsed))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::api::{SchemaMode, Wire};
    use crate::core::engine::{ENGINES, EngineId, Transport, engine_by_name};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    type Captured = Arc<Mutex<Vec<String>>>;

    static SCHEMA_CAPABLE_API: ApiSpec = ApiSpec {
        wire: Wire::OpenAiChat,
        base_url: "https://api.example",
        base_url_env: &[],
        path: "/v1/chat/completions",
        models_path: "",
        auth_header: None,
        auth_prefix: "",
        key_env: &[],
        extra_headers: &[],
        schema_mode: SchemaMode::NativeSchema,
        default_max_tokens: 1024,
        probes_models: false,
    };

    static SCHEMA_CAPABLE: EngineSpec = EngineSpec {
        id: EngineId::OpenAi,
        prices: &[],
        name: "schema-capable",
        transport: Transport::Api(&SCHEMA_CAPABLE_API),
        supports_json_schema: true,
        models: &[],
        fast_model: None,
        auto_select: false,
        missing_label: "not configured",
        install_hint: "a test-only row",
    };

    async fn serve(script: Vec<(u16, &'static str)>) -> (String, Captured) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let captured: Captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        tokio::spawn(async move {
            for (status, body) in script {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = vec![0_u8; 65_536];
                let read = stream.read(&mut buffer).await.unwrap_or_default();
                sink.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[..read]).into_owned());
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            }
        });
        (base, captured)
    }

    fn engine_at(base: &str, engine: &'static str, key: Option<&str>) -> ApiEngine {
        let spec = engine_by_name(engine).unwrap();
        ApiEngine::new(
            spec,
            spec.api().unwrap(),
            base,
            "the-model".to_string(),
            key.and_then(ApiKey::new),
            0,
        )
        .unwrap()
    }

    fn job(timeout_ms: u64) -> EngineJob {
        EngineJob {
            prompt: "the prompt".to_string(),
            schema: None,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    #[tokio::test]
    async fn a_successful_call_returns_text_and_usage() {
        let body = include_str!("../../tests/fixtures/api/openai_chat.json");
        let (base, captured) = serve(vec![(200, body)]).await;
        let engine = engine_at(&base, "openai", Some("sk-test"));
        let output = engine.run(&job(5_000)).await.unwrap();
        assert_eq!(output.text, "{\"summary\":\"openai payload\"}");
        assert_eq!(output.usage.unwrap().input_tokens, 120);
        let requests = captured.lock().unwrap();
        assert!(requests[0].contains("authorization: Bearer sk-test"));
        assert!(requests[0].contains("the prompt"));
    }

    #[tokio::test]
    async fn anthropic_sends_its_version_header_and_a_token_budget() {
        let body = include_str!("../../tests/fixtures/api/anthropic_messages.json");
        let (base, captured) = serve(vec![(200, body)]).await;
        let spec = engine_by_name("anthropic").unwrap();
        let engine = ApiEngine::new(
            spec,
            spec.api().unwrap(),
            &base,
            "claude-sonnet-5".to_string(),
            ApiKey::new("sk-ant"),
            4096,
        )
        .unwrap();
        let output = engine.run(&job(5_000)).await.unwrap();
        assert_eq!(output.text, "{\"summary\":\"anthropic payload\"}");
        let requests = captured.lock().unwrap();
        assert!(requests[0].contains("anthropic-version: 2023-06-01"));
        assert!(requests[0].contains("x-api-key: sk-ant"));
        assert!(requests[0].contains("\"max_tokens\":4096"));
    }

    #[tokio::test]
    async fn rate_limiting_is_retried_and_then_succeeds() {
        let body = include_str!("../../tests/fixtures/api/openai_chat.json");
        let (base, captured) = serve(vec![(429, "{}"), (200, body)]).await;
        let engine = engine_at(&base, "openai", Some("sk-test"));
        let output = engine.run(&job(10_000)).await.unwrap();
        assert_eq!(output.text, "{\"summary\":\"openai payload\"}");
        assert_eq!(captured.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_bad_request_is_never_retried() {
        let body = include_str!("../../tests/fixtures/api/openai_error.json");
        let (base, captured) = serve(vec![(400, body), (200, "{}")]).await;
        let engine = engine_at(&base, "openai", Some("sk-test"));
        let error = engine.run(&job(5_000)).await.unwrap_err();
        assert!(matches!(error, EngineError::Failed { status: 400, .. }));
        assert_eq!(captured.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn server_errors_stop_after_the_attempt_budget() {
        let (base, captured) =
            serve(vec![(500, "{}"), (500, "{}"), (500, "{}"), (200, "{}")]).await;
        let engine = engine_at(&base, "openai", Some("sk-test"));
        let error = engine.run(&job(10_000)).await.unwrap_err();
        assert!(matches!(error, EngineError::Failed { status: 500, .. }));
        assert_eq!(captured.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn an_error_body_never_echoes_the_key_back() {
        let (base, _) = serve(vec![(
            401,
            "{\"error\":{\"message\":\"invalid key sk-leaky\"}}",
        )])
        .await;
        let engine = engine_at(&base, "openai", Some("sk-leaky"));
        let error = engine.run(&job(5_000)).await.unwrap_err();
        let rendered = error.to_string();
        assert!(!rendered.contains("sk-leaky"), "{rendered}");
        assert!(rendered.contains("invalid key"), "{rendered}");
    }

    #[tokio::test]
    async fn an_unreachable_endpoint_reports_a_spawn_failure() {
        let engine = engine_at("http://127.0.0.1:1", "openai", Some("sk-test"));
        let error = engine.run(&job(5_000)).await.unwrap_err();
        assert!(matches!(error, EngineError::Spawn(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_non_json_body_is_reported_rather_than_parsed() {
        let (base, _) = serve(vec![(200, "<html>gateway</html>")]).await;
        let engine = engine_at(&base, "openai", Some("sk-test"));
        let error = engine.run(&job(5_000)).await.unwrap_err();
        assert_eq!(
            error,
            EngineError::Reported("the endpoint returned a non-JSON body".to_string())
        );
    }

    #[tokio::test]
    async fn the_probe_lists_installed_models_and_survives_a_dead_daemon() {
        let body = include_str!("../../tests/fixtures/api/ollama_tags.json");
        let (base, _) = serve(vec![(200, body)]).await;
        let api = engine_by_name("ollama").unwrap().api().unwrap();
        assert_eq!(
            installed_models(api, &base).await,
            vec!["qwen3:8b".to_string(), "llama3.2:3b".to_string()]
        );
        assert!(installed_models(api, "http://127.0.0.1:1").await.is_empty());
    }

    #[tokio::test]
    async fn every_api_engine_can_be_constructed_from_its_row() {
        for spec in ENGINES.iter().filter(|spec| spec.api().is_some()) {
            let engine = ApiEngine::new(
                spec,
                spec.api().unwrap(),
                "http://127.0.0.1:9",
                "m".to_string(),
                ApiKey::new("sk-test"),
                0,
            );
            assert!(engine.is_some(), "{}", spec.name);
            assert_eq!(engine.unwrap().name(), spec.name);
        }
    }

    #[tokio::test]
    async fn the_client_identifies_itself_and_refuses_to_chase_redirects() {
        let body = include_str!("../../tests/fixtures/api/openai_chat.json");
        let (base, captured) = serve(vec![(200, body)]).await;
        let engine = engine_at(&base, "openai", Some("sk-test"));
        engine.run(&job(5_000)).await.expect("the call succeeds");
        let requests = captured.lock().unwrap();
        assert!(
            requests[0].contains(concat!("user-agent: muaddib/", env!("CARGO_PKG_VERSION"))),
            "{}",
            requests[0]
        );
    }

    #[tokio::test]
    async fn a_redirect_is_never_chased_to_a_second_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let body = include_str!("../../tests/fixtures/api/openai_chat.json");
        tokio::spawn(async move {
            let redirect = "HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
            let followed = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            for response in [redirect, followed] {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut request = vec![0_u8; 8192];
                let _ = stream.read(&mut request).await;
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            }
        });
        let engine = engine_at(&base, "openai", Some("sk-test"));
        assert!(
            engine.run(&job(5_000)).await.is_err(),
            "the redirect target must never be reached"
        );
    }

    #[test]
    fn an_engine_is_built_from_a_status_only_when_it_has_an_endpoint() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            endpoint: Option<&'static str>,
            want: bool,
        }
        let cases = [
            Case {
                name: "an api engine with a resolved endpoint",
                engine: "openai",
                endpoint: Some("https://api.openai.com"),
                want: true,
            },
            Case {
                name: "an api engine whose base url never resolved",
                engine: "local",
                endpoint: None,
                want: false,
            },
            Case {
                name: "a cli engine has no api spec at all",
                engine: "claude",
                endpoint: Some("https://api.openai.com"),
                want: false,
            },
        ];
        for case in cases {
            let spec = engine_by_name(case.engine).unwrap();
            let status = EngineStatus {
                endpoint: case.endpoint.map(ToString::to_string),
                ..EngineStatus::unavailable(spec)
            };
            let engine =
                ApiEngine::from_status(&status, ApiKey::new("sk-test"), "m".to_string(), None);
            assert_eq!(engine.is_some(), case.want, "{}", case.name);
        }
    }

    #[tokio::test]
    async fn a_reporting_run_announces_the_model_it_is_asking() {
        let body = include_str!("../../tests/fixtures/api/openai_chat.json");
        let (base, _) = serve(vec![(200, body)]).await;
        let engine = engine_at(&base, "openai", Some("sk-test"));
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        engine
            .run_reporting(&job(5_000), tx)
            .await
            .expect("the call succeeds");
        let activity = rx.try_recv().expect("the sink was told about the call");
        assert_eq!(activity.target, "the-model");
    }

    #[test]
    fn json_schema_support_is_read_from_the_row_rather_than_assumed() {
        struct Case {
            name: &'static str,
            spec: &'static EngineSpec,
            want: bool,
        }
        let cases = [
            Case {
                name: "no shipped api row claims native schema support",
                spec: engine_by_name("openai").unwrap(),
                want: false,
            },
            Case {
                name: "a row that claims it is believed",
                spec: &SCHEMA_CAPABLE,
                want: true,
            },
        ];
        for case in cases {
            let engine = ApiEngine::new(
                case.spec,
                case.spec.api().unwrap(),
                "http://127.0.0.1:9",
                "m".to_string(),
                ApiKey::new("sk-test"),
                0,
            )
            .expect("the engine builds");
            assert_eq!(engine.supports_json_schema(), case.want, "{}", case.name);
        }
    }
}
