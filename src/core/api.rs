use crate::core::cost::{EngineUsage, ModelPrice, estimate_cost, parse_usage};
use crate::core::engine::EngineError;
use serde_json::{Value, json};
use std::fmt;
use std::time::Duration;

pub const MODEL_PLACEHOLDER: &str = "{model}";
pub const ERROR_TAIL_CHARS: usize = 400;
pub const MAX_ATTEMPTS: u32 = 3;
pub const BASE_BACKOFF: Duration = Duration::from_millis(500);
pub const MAX_BACKOFF: Duration = Duration::from_secs(20);

const RETRY_STATUSES: &[u16] = &[408, 409, 429, 500, 502, 503, 504];
const ERROR_POINTERS: &[&str] = &["/error/message", "/error", "/message", "/detail"];
const STOP_REASON_KEYS: &[&str] = &["stop_reason", "finishReason", "done_reason"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    OpenAiChat,
    AnthropicMessages,
    GeminiGenerate,
    OllamaChat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchemaMode {
    Prompt,
    JsonObject,
    NativeSchema,
}

#[derive(Debug)]
pub struct ApiSpec {
    pub wire: Wire,
    pub base_url: &'static str,
    pub base_url_env: &'static [&'static str],
    pub path: &'static str,
    pub models_path: &'static str,
    pub auth_header: Option<&'static str>,
    pub auth_prefix: &'static str,
    pub key_env: &'static [&'static str],
    pub extra_headers: &'static [(&'static str, &'static str)],
    pub schema_mode: SchemaMode,
    pub default_max_tokens: u32,
    pub probes_models: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| Self(trimmed.to_string()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey(***)")
    }
}

pub struct ApiCall<'a> {
    pub spec: &'static ApiSpec,
    pub model: &'a str,
    pub prompt: &'a str,
    pub schema: Option<&'a str>,
    pub max_tokens: u32,
}

pub fn endpoint(spec: &ApiSpec, base_url: &str, model: &str) -> String {
    format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        spec.path.replace(MODEL_PLACEHOLDER, model)
    )
}

pub fn models_endpoint(spec: &ApiSpec, base_url: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), spec.models_path)
}

pub fn request_headers(spec: &ApiSpec, key: Option<&ApiKey>) -> Vec<(&'static str, String)> {
    let authorization = spec
        .auth_header
        .zip(key)
        .map(|(header, key)| (header, format!("{}{}", spec.auth_prefix, key.expose())));
    spec.extra_headers
        .iter()
        .map(|(name, value)| (*name, (*value).to_string()))
        .chain(authorization)
        .collect()
}

pub fn key_env_candidates<'a>(spec: &'a ApiSpec, configured: Option<&'a str>) -> Vec<&'a str> {
    configured
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .into_iter()
        .chain(spec.key_env.iter().copied())
        .collect()
}

pub fn base_url_candidates<'a>(spec: &'a ApiSpec, configured: Option<&'a str>) -> Vec<&'a str> {
    configured
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .into_iter()
        .chain(spec.base_url_env.iter().copied())
        .collect()
}

pub fn api_available(
    spec: &ApiSpec,
    key: Option<&ApiKey>,
    models: &[String],
    vaulted: bool,
) -> bool {
    if spec.probes_models {
        !models.is_empty()
    } else {
        key.is_some() || vaulted
    }
}

fn user_message(prompt: &str) -> Value {
    json!([{ "role": "user", "content": prompt }])
}

fn schema_value(call: &ApiCall) -> Option<Value> {
    call.schema
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
}

pub fn request_body(call: &ApiCall) -> Value {
    match call.spec.wire {
        Wire::OpenAiChat => openai_body(call),
        Wire::AnthropicMessages => anthropic_body(call),
        Wire::GeminiGenerate => gemini_body(call),
        Wire::OllamaChat => ollama_body(call),
    }
}

fn openai_response_format(call: &ApiCall) -> Value {
    match (call.spec.schema_mode, schema_value(call)) {
        (SchemaMode::NativeSchema, Some(schema)) => json!({
            "type": "json_schema",
            "json_schema": { "name": "muaddib_answer", "strict": true, "schema": schema },
        }),
        _ => json!({ "type": "json_object" }),
    }
}

fn openai_body(call: &ApiCall) -> Value {
    let mut body = json!({
        "model": call.model,
        "messages": user_message(call.prompt),
    });
    if call.max_tokens > 0 {
        body["max_completion_tokens"] = json!(call.max_tokens);
    }
    if call.schema.is_some() && call.spec.schema_mode >= SchemaMode::JsonObject {
        body["response_format"] = openai_response_format(call);
    }
    body
}

fn anthropic_body(call: &ApiCall) -> Value {
    let mut body = json!({
        "model": call.model,
        "max_tokens": call.max_tokens.max(1),
        "messages": user_message(call.prompt),
    });
    if call.spec.schema_mode == SchemaMode::NativeSchema
        && let Some(schema) = schema_value(call)
    {
        body["output_config"] = json!({ "format": { "type": "json_schema", "schema": schema } });
    }
    body
}

fn gemini_body(call: &ApiCall) -> Value {
    let mut config = serde_json::Map::new();
    if call.max_tokens > 0 {
        config.insert("maxOutputTokens".to_string(), json!(call.max_tokens));
    }
    if call.schema.is_some() && call.spec.schema_mode >= SchemaMode::JsonObject {
        config.insert("responseMimeType".to_string(), json!("application/json"));
    }
    if call.spec.schema_mode == SchemaMode::NativeSchema
        && let Some(schema) = schema_value(call)
    {
        config.insert("responseSchema".to_string(), schema);
    }
    json!({
        "contents": [{ "parts": [{ "text": call.prompt }] }],
        "generationConfig": Value::Object(config),
    })
}

fn ollama_body(call: &ApiCall) -> Value {
    let mut body = json!({
        "model": call.model,
        "messages": user_message(call.prompt),
        "stream": false,
    });
    if call.schema.is_some() {
        match (call.spec.schema_mode, schema_value(call)) {
            (SchemaMode::NativeSchema, Some(schema)) => body["format"] = schema,
            (SchemaMode::JsonObject, _) => body["format"] = json!("json"),
            _ => {}
        }
    }
    if call.max_tokens > 0 {
        body["options"] = json!({ "num_predict": call.max_tokens });
    }
    body
}

pub fn error_message(body: &Value) -> Option<String> {
    ERROR_POINTERS
        .iter()
        .find_map(|pointer| body.pointer(pointer).and_then(Value::as_str))
        .map(ToString::to_string)
}

fn empty_reason(body: &Value) -> String {
    STOP_REASON_KEYS
        .iter()
        .find_map(|key| body.get(*key).and_then(Value::as_str))
        .map_or_else(
            || "the model returned no text".to_string(),
            |reason| format!("the model returned no text ({reason})"),
        )
}

fn openai_text(body: &Value) -> Option<String> {
    body.pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn anthropic_text(body: &Value) -> Option<String> {
    Some(
        body.get("content")?
            .as_array()?
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
    )
}

fn gemini_text(body: &Value) -> Option<String> {
    Some(
        body.pointer("/candidates/0/content/parts")?
            .as_array()?
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
    )
}

fn ollama_text(body: &Value) -> Option<String> {
    body.pointer("/message/content")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

pub fn response_text(wire: Wire, body: &Value) -> Result<String, EngineError> {
    if let Some(message) = error_message(body) {
        return Err(EngineError::Reported(message));
    }
    let text = match wire {
        Wire::OpenAiChat => openai_text(body),
        Wire::AnthropicMessages => anthropic_text(body),
        Wire::GeminiGenerate => gemini_text(body),
        Wire::OllamaChat => ollama_text(body),
    };
    text.filter(|found| !found.trim().is_empty())
        .ok_or_else(|| EngineError::Reported(empty_reason(body)))
}

fn counted_usage(body: &Value, input_key: &str, output_key: &str) -> Option<EngineUsage> {
    let read = |key: &str| {
        body.pointer(key)
            .and_then(Value::as_u64)
            .unwrap_or_default()
    };
    let usage = EngineUsage {
        input_tokens: read(input_key),
        output_tokens: read(output_key),
        cost_usd: 0.0,
    };
    (!usage.is_empty()).then_some(usage)
}

pub fn response_usage(
    wire: Wire,
    body: &Value,
    prices: &[ModelPrice],
    model: &str,
) -> Option<EngineUsage> {
    let usage = wire_usage(wire, body)?;
    Some(EngineUsage {
        cost_usd: if usage.cost_usd > 0.0 {
            usage.cost_usd
        } else {
            estimate_cost(prices, model, usage)
        },
        ..usage
    })
}

fn wire_usage(wire: Wire, body: &Value) -> Option<EngineUsage> {
    match wire {
        Wire::OpenAiChat => counted_usage(body, "/usage/prompt_tokens", "/usage/completion_tokens"),
        Wire::AnthropicMessages => parse_usage(body),
        Wire::GeminiGenerate => counted_usage(
            body,
            "/usageMetadata/promptTokenCount",
            "/usageMetadata/candidatesTokenCount",
        ),
        Wire::OllamaChat => counted_usage(body, "/prompt_eval_count", "/eval_count"),
    }
}

pub fn models_from_tags(body: &Value) -> Vec<String> {
    body.get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|entry| entry.get("name").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_retry_after(header: &str) -> Option<Duration> {
    header.trim().parse::<u64>().ok().map(Duration::from_secs)
}

pub fn retry_delay(attempt: u32, status: u16, retry_after: Option<&str>) -> Option<Duration> {
    if attempt + 1 >= MAX_ATTEMPTS || !RETRY_STATUSES.contains(&status) {
        return None;
    }
    let delay = retry_after
        .and_then(parse_retry_after)
        .unwrap_or_else(|| BASE_BACKOFF * 2u32.pow(attempt.min(8)));
    Some(delay.min(MAX_BACKOFF))
}

pub fn redact(text: &str, key: Option<&ApiKey>, max_chars: usize) -> String {
    let masked = key.map_or_else(|| text.to_string(), |key| text.replace(key.expose(), "***"));
    let trimmed = masked.trim();
    let total = trimmed.chars().count();
    trimmed
        .chars()
        .skip(total.saturating_sub(max_chars))
        .collect()
}

pub fn failure(status: u16, body: &str, key: Option<&ApiKey>) -> EngineError {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .as_ref()
        .and_then(error_message);
    EngineError::Failed {
        status: i32::from(status),
        stderr_tail: redact(detail.as_deref().unwrap_or(body), key, ERROR_TAIL_CHARS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::engine::{ENGINES, EngineSpec};

    const OPENAI_CHAT: &str = include_str!("../../tests/fixtures/api/openai_chat.json");
    const ANTHROPIC_MESSAGES: &str =
        include_str!("../../tests/fixtures/api/anthropic_messages.json");
    const ANTHROPIC_REFUSAL: &str = include_str!("../../tests/fixtures/api/anthropic_refusal.json");
    const GEMINI_GENERATE: &str = include_str!("../../tests/fixtures/api/gemini_generate.json");
    const OLLAMA_CHAT: &str = include_str!("../../tests/fixtures/api/ollama_chat.json");
    const OLLAMA_TAGS: &str = include_str!("../../tests/fixtures/api/ollama_tags.json");
    const OPENAI_ERROR: &str = include_str!("../../tests/fixtures/api/openai_error.json");

    fn parse(raw: &str) -> Value {
        serde_json::from_str(raw).unwrap()
    }

    fn api_by_name(name: &str) -> &'static ApiSpec {
        ENGINES
            .iter()
            .find(|spec| spec.name == name)
            .and_then(EngineSpec::api)
            .unwrap()
    }

    fn call<'a>(spec: &'static ApiSpec, schema: Option<&'a str>, max_tokens: u32) -> ApiCall<'a> {
        ApiCall {
            spec,
            model: "the-model",
            prompt: "the prompt",
            schema,
            max_tokens,
        }
    }

    const fn native_spec(wire: Wire) -> ApiSpec {
        ApiSpec {
            wire,
            base_url: "https://api.example",
            base_url_env: &[],
            path: "/v1/chat",
            models_path: "",
            auth_header: None,
            auth_prefix: "",
            key_env: &[],
            extra_headers: &[],
            schema_mode: SchemaMode::NativeSchema,
            default_max_tokens: 1024,
            probes_models: false,
        }
    }

    static NATIVE_OPENAI: ApiSpec = native_spec(Wire::OpenAiChat);
    static NATIVE_ANTHROPIC: ApiSpec = native_spec(Wire::AnthropicMessages);
    static NATIVE_GEMINI: ApiSpec = native_spec(Wire::GeminiGenerate);
    static NATIVE_OLLAMA: ApiSpec = native_spec(Wire::OllamaChat);

    const OBJECT_SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"}}}"#;

    #[test]
    fn a_native_schema_engine_carries_the_schema_in_the_body_of_every_wire() {
        struct Case {
            name: &'static str,
            spec: &'static ApiSpec,
            pointer: &'static str,
        }
        let cases = [
            Case {
                name: "openai nests it under response_format",
                spec: &NATIVE_OPENAI,
                pointer: "/response_format/json_schema/schema/type",
            },
            Case {
                name: "anthropic nests it under output_config",
                spec: &NATIVE_ANTHROPIC,
                pointer: "/output_config/format/schema/type",
            },
            Case {
                name: "gemini nests it under generationConfig",
                spec: &NATIVE_GEMINI,
                pointer: "/generationConfig/responseSchema/type",
            },
            Case {
                name: "ollama puts it straight into format",
                spec: &NATIVE_OLLAMA,
                pointer: "/format/type",
            },
        ];
        for case in cases {
            let body = request_body(&call(case.spec, Some(OBJECT_SCHEMA), 256));
            assert_eq!(
                body.pointer(case.pointer).and_then(Value::as_str),
                Some("object"),
                "{}: {body}",
                case.name
            );
        }
    }

    #[test]
    fn a_native_schema_engine_falls_back_when_the_schema_will_not_parse() {
        struct Case {
            name: &'static str,
            spec: &'static ApiSpec,
            absent: &'static str,
        }
        let cases = [
            Case {
                name: "openai drops to a plain json object",
                spec: &NATIVE_OPENAI,
                absent: "/response_format/json_schema",
            },
            Case {
                name: "anthropic sends no output_config",
                spec: &NATIVE_ANTHROPIC,
                absent: "/output_config",
            },
            Case {
                name: "gemini sends no responseSchema",
                spec: &NATIVE_GEMINI,
                absent: "/generationConfig/responseSchema",
            },
            Case {
                name: "ollama sends no format",
                spec: &NATIVE_OLLAMA,
                absent: "/format",
            },
        ];
        for case in cases {
            let body = request_body(&call(case.spec, Some("{not json"), 256));
            assert!(body.pointer(case.absent).is_none(), "{}: {body}", case.name);
        }
    }

    #[test]
    fn a_reported_cost_wins_over_the_price_table_and_an_absent_one_is_estimated() {
        const PRICES: &[ModelPrice] = &[ModelPrice {
            prefix: "the-model",
            input_per_million: 3.0,
            output_per_million: 15.0,
        }];
        struct Case {
            name: &'static str,
            body: &'static str,
            want: f64,
        }
        let cases = [
            Case {
                name: "the provider's own total is kept",
                body: r#"{"total_cost_usd":0.42,"usage":{"input_tokens":1000000,"output_tokens":1000000}}"#,
                want: 0.42,
            },
            Case {
                name: "without a reported total the tokens are priced",
                body: r#"{"usage":{"input_tokens":1000000,"output_tokens":1000000}}"#,
                want: 18.0,
            },
        ];
        for case in cases {
            let usage = response_usage(
                Wire::AnthropicMessages,
                &parse(case.body),
                PRICES,
                "the-model",
            )
            .expect("the body reports usage");
            assert!(
                (usage.cost_usd - case.want).abs() < 1e-9,
                "{}: got {}",
                case.name,
                usage.cost_usd
            );
        }
    }

    #[test]
    fn endpoints_interpolate_the_model_and_never_double_the_separator() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            base: &'static str,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "openai leaves the path alone",
                engine: "openai",
                base: "https://api.openai.com",
                want: "https://api.openai.com/v1/chat/completions",
            },
            Case {
                name: "a trailing slash does not double up",
                engine: "openai",
                base: "https://api.openai.com/",
                want: "https://api.openai.com/v1/chat/completions",
            },
            Case {
                name: "gemini interpolates the model",
                engine: "gemini",
                base: "https://generativelanguage.googleapis.com",
                want: "https://generativelanguage.googleapis.com/v1beta/models/the-model:generateContent",
            },
        ];
        for case in cases {
            let built = endpoint(api_by_name(case.engine), case.base, "the-model");
            assert_eq!(built, case.want, "{}", case.name);
        }
    }

    #[test]
    fn no_endpoint_ever_carries_the_key() {
        let key = ApiKey::new("sk-secret-value").unwrap();
        for spec in ENGINES.iter().filter_map(EngineSpec::api) {
            let built = endpoint(spec, spec.base_url, "the-model");
            assert!(!built.contains(key.expose()), "{built}");
            assert!(!built.contains("key="), "{built}");
        }
    }

    #[test]
    fn auth_headers_carry_the_exact_prefix_each_provider_expects() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            want: Option<(&'static str, &'static str)>,
        }
        let cases = [
            Case {
                name: "openai uses a bearer prefix",
                engine: "openai",
                want: Some(("authorization", "Bearer sk-test")),
            },
            Case {
                name: "anthropic sends the raw key",
                engine: "anthropic",
                want: Some(("x-api-key", "sk-test")),
            },
            Case {
                name: "gemini sends the raw key in its own header",
                engine: "gemini",
                want: Some(("x-goog-api-key", "sk-test")),
            },
            Case {
                name: "ollama needs no auth",
                engine: "ollama",
                want: None,
            },
        ];
        let key = ApiKey::new("sk-test").unwrap();
        for case in cases {
            let spec = api_by_name(case.engine);
            let headers = request_headers(spec, Some(&key));
            let found = headers
                .iter()
                .find(|(name, _)| Some(*name) == spec.auth_header)
                .map(|(name, value)| (*name, value.as_str()));
            assert_eq!(found, case.want, "{}", case.name);
        }
    }

    #[test]
    fn anthropic_always_sends_its_version_header() {
        let headers = request_headers(api_by_name("anthropic"), None);
        assert!(
            headers
                .iter()
                .any(|(name, value)| *name == "anthropic-version" && value == "2023-06-01")
        );
    }

    #[test]
    fn headers_omit_authorization_when_no_key_is_available() {
        for spec in ENGINES.iter().filter_map(EngineSpec::api) {
            let headers = request_headers(spec, None);
            assert!(
                !headers
                    .iter()
                    .any(|(name, _)| Some(*name) == spec.auth_header),
                "{:?}",
                spec.wire
            );
        }
    }

    #[test]
    fn every_wire_carries_the_prompt_and_addresses_the_model_somewhere() {
        for spec in ENGINES.iter().filter_map(EngineSpec::api) {
            let body = request_body(&call(spec, None, 0)).to_string();
            let url = endpoint(spec, "https://host", "the-model");
            assert!(body.contains("the prompt"), "{:?}", spec.wire);
            assert!(
                body.contains("the-model") || url.contains("the-model"),
                "{:?}",
                spec.wire
            );
        }
    }

    #[test]
    fn json_mode_is_requested_only_when_a_schema_is_asked_for() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            pointer: &'static str,
            want: Value,
        }
        let cases = [
            Case {
                name: "openai asks for a json object",
                engine: "openai",
                pointer: "/response_format/type",
                want: json!("json_object"),
            },
            Case {
                name: "gemini asks for a json mime type",
                engine: "gemini",
                pointer: "/generationConfig/responseMimeType",
                want: json!("application/json"),
            },
            Case {
                name: "ollama asks for json",
                engine: "ollama",
                pointer: "/format",
                want: json!("json"),
            },
        ];
        for case in cases {
            let spec = api_by_name(case.engine);
            let with_schema = request_body(&call(spec, Some("{\"type\":\"object\"}"), 0));
            let without = request_body(&call(spec, None, 0));
            assert_eq!(
                with_schema.pointer(case.pointer),
                Some(&case.want),
                "{}",
                case.name
            );
            assert_eq!(without.pointer(case.pointer), None, "{}", case.name);
        }
    }

    #[test]
    fn anthropic_always_sends_a_positive_max_tokens() {
        let body = request_body(&call(api_by_name("anthropic"), None, 0));
        assert_eq!(body.pointer("/max_tokens"), Some(&json!(1)));
        let sized = request_body(&call(api_by_name("anthropic"), None, 4096));
        assert_eq!(sized.pointer("/max_tokens"), Some(&json!(4096)));
    }

    #[test]
    fn a_zero_budget_omits_the_token_cap_everywhere_else() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            pointer: &'static str,
        }
        let cases = [
            Case {
                name: "openai",
                engine: "openai",
                pointer: "/max_completion_tokens",
            },
            Case {
                name: "gemini",
                engine: "gemini",
                pointer: "/generationConfig/maxOutputTokens",
            },
            Case {
                name: "ollama",
                engine: "ollama",
                pointer: "/options/num_predict",
            },
        ];
        for case in cases {
            let spec = api_by_name(case.engine);
            assert_eq!(
                request_body(&call(spec, None, 0)).pointer(case.pointer),
                None,
                "{}",
                case.name
            );
            assert_eq!(
                request_body(&call(spec, None, 512)).pointer(case.pointer),
                Some(&json!(512)),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn response_text_reads_every_wire_from_its_fixture() {
        struct Case {
            name: &'static str,
            wire: Wire,
            body: &'static str,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "openai message content",
                wire: Wire::OpenAiChat,
                body: OPENAI_CHAT,
                want: "{\"summary\":\"openai payload\"}",
            },
            Case {
                name: "anthropic skips the thinking block",
                wire: Wire::AnthropicMessages,
                body: ANTHROPIC_MESSAGES,
                want: "{\"summary\":\"anthropic payload\"}",
            },
            Case {
                name: "gemini joins its parts",
                wire: Wire::GeminiGenerate,
                body: GEMINI_GENERATE,
                want: "{\"summary\":\"gemini payload\"}",
            },
            Case {
                name: "ollama message content",
                wire: Wire::OllamaChat,
                body: OLLAMA_CHAT,
                want: "{\"summary\":\"ollama payload\"}",
            },
        ];
        for case in cases {
            let text = response_text(case.wire, &parse(case.body)).unwrap();
            assert_eq!(text, case.want, "{}", case.name);
        }
    }

    #[test]
    fn an_empty_answer_surfaces_the_stop_reason_instead_of_looking_blank() {
        let error = response_text(Wire::AnthropicMessages, &parse(ANTHROPIC_REFUSAL)).unwrap_err();
        let EngineError::Reported(message) = error else {
            panic!("expected a reported error");
        };
        assert!(message.contains("refusal"), "{message}");
    }

    #[test]
    fn an_error_envelope_becomes_a_reported_error() {
        let error = response_text(Wire::OpenAiChat, &parse(OPENAI_ERROR)).unwrap_err();
        assert_eq!(
            error,
            EngineError::Reported("Incorrect API key provided".to_string())
        );
    }

    #[test]
    fn usage_is_read_from_each_wire_and_absent_when_unreported() {
        struct Case {
            name: &'static str,
            wire: Wire,
            body: &'static str,
            want: Option<EngineUsage>,
        }
        let cases = [
            Case {
                name: "openai prompt and completion tokens",
                wire: Wire::OpenAiChat,
                body: OPENAI_CHAT,
                want: Some(EngineUsage {
                    input_tokens: 120,
                    output_tokens: 45,
                    cost_usd: 0.0,
                }),
            },
            Case {
                name: "anthropic counts cache tokens as input",
                wire: Wire::AnthropicMessages,
                body: ANTHROPIC_MESSAGES,
                want: Some(EngineUsage {
                    input_tokens: 1_140,
                    output_tokens: 64,
                    cost_usd: 0.0,
                }),
            },
            Case {
                name: "gemini usage metadata",
                wire: Wire::GeminiGenerate,
                body: GEMINI_GENERATE,
                want: Some(EngineUsage {
                    input_tokens: 88,
                    output_tokens: 21,
                    cost_usd: 0.0,
                }),
            },
            Case {
                name: "ollama eval counts",
                wire: Wire::OllamaChat,
                body: OLLAMA_CHAT,
                want: Some(EngineUsage {
                    input_tokens: 31,
                    output_tokens: 12,
                    cost_usd: 0.0,
                }),
            },
            Case {
                name: "a body without usage reports nothing",
                wire: Wire::OpenAiChat,
                body: "{\"choices\":[]}",
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(
                response_usage(case.wire, &parse(case.body), &[], ""),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn installed_models_are_read_from_the_tag_listing() {
        assert_eq!(
            models_from_tags(&parse(OLLAMA_TAGS)),
            vec!["qwen3:8b".to_string(), "llama3.2:3b".to_string()]
        );
        assert!(models_from_tags(&parse("{\"models\":\"nope\"}")).is_empty());
        assert!(models_from_tags(&parse("{}")).is_empty());
    }

    #[test]
    fn retry_delay_backs_off_only_for_retryable_statuses() {
        struct Case {
            name: &'static str,
            attempt: u32,
            status: u16,
            retry_after: Option<&'static str>,
            want: Option<Duration>,
        }
        let cases = [
            Case {
                name: "a bad request is never retried",
                attempt: 0,
                status: 400,
                retry_after: None,
                want: None,
            },
            Case {
                name: "an unauthorized key is never retried",
                attempt: 0,
                status: 401,
                retry_after: None,
                want: None,
            },
            Case {
                name: "rate limiting backs off",
                attempt: 0,
                status: 429,
                retry_after: None,
                want: Some(BASE_BACKOFF),
            },
            Case {
                name: "backoff doubles with each attempt",
                attempt: 1,
                status: 429,
                retry_after: None,
                want: Some(Duration::from_secs(1)),
            },
            Case {
                name: "the last attempt does not retry",
                attempt: MAX_ATTEMPTS - 1,
                status: 429,
                retry_after: None,
                want: None,
            },
            Case {
                name: "retry-after is honoured",
                attempt: 0,
                status: 429,
                retry_after: Some("3"),
                want: Some(Duration::from_secs(3)),
            },
            Case {
                name: "a long retry-after is clamped",
                attempt: 0,
                status: 503,
                retry_after: Some("600"),
                want: Some(MAX_BACKOFF),
            },
            Case {
                name: "an http date falls back to backoff",
                attempt: 0,
                status: 503,
                retry_after: Some("Wed, 21 Oct 2026 07:28:00 GMT"),
                want: Some(BASE_BACKOFF),
            },
        ];
        for case in cases {
            assert_eq!(
                retry_delay(case.attempt, case.status, case.retry_after),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn redaction_removes_the_key_but_keeps_the_surrounding_message() {
        let key = ApiKey::new("sk-secret").unwrap();
        let redacted = redact("bad key sk-secret rejected", Some(&key), 400);
        assert_eq!(redacted, "bad key *** rejected");
        assert_eq!(redact("plain message", None, 400), "plain message");
        assert_eq!(redact("abcdef", None, 3), "def");
    }

    #[test]
    fn a_failure_never_leaks_the_key_and_keeps_the_provider_detail() {
        let key = ApiKey::new("sk-secret").unwrap();
        let error = failure(
            401,
            "{\"error\":{\"message\":\"bad key sk-secret\"}}",
            Some(&key),
        );
        let rendered = error.to_string();
        assert!(!rendered.contains("sk-secret"), "{rendered}");
        assert!(rendered.contains("bad key"), "{rendered}");
        assert!(rendered.contains("401"), "{rendered}");
    }

    #[test]
    fn a_key_never_reveals_itself_through_debug() {
        let key = ApiKey::new("sk-secret").unwrap();
        assert_eq!(format!("{key:?}"), "ApiKey(***)");
        assert!(ApiKey::new("   ").is_none());
        assert_eq!(ApiKey::new("  sk-padded  ").unwrap().expose(), "sk-padded");
    }

    #[test]
    fn key_candidates_put_the_configured_variable_first() {
        let spec = api_by_name("gemini");
        assert_eq!(
            key_env_candidates(spec, Some("WORK_KEY")),
            vec!["WORK_KEY", "GEMINI_API_KEY", "GOOGLE_API_KEY"]
        );
        assert_eq!(
            key_env_candidates(spec, Some("  ")),
            vec!["GEMINI_API_KEY", "GOOGLE_API_KEY"]
        );
    }

    #[test]
    fn availability_follows_the_probe_for_local_engines_and_the_key_for_hosted_ones() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            key: Option<&'static str>,
            models: &'static [&'static str],
            vaulted: bool,
            want: bool,
        }
        let cases = [
            Case {
                name: "a running ollama with a pulled model is available",
                engine: "ollama",
                key: None,
                models: &["qwen3:8b"],
                vaulted: false,
                want: true,
            },
            Case {
                name: "a stopped ollama is unavailable",
                engine: "ollama",
                key: None,
                models: &[],
                vaulted: false,
                want: false,
            },
            Case {
                name: "a stopped ollama stays unavailable even with a vaulted key",
                engine: "ollama",
                key: None,
                models: &[],
                vaulted: true,
                want: false,
            },
            Case {
                name: "openai with a key is available",
                engine: "openai",
                key: Some("sk-test"),
                models: &[],
                vaulted: false,
                want: true,
            },
            Case {
                name: "openai without a key is unavailable",
                engine: "openai",
                key: None,
                models: &[],
                vaulted: false,
                want: false,
            },
            Case {
                name: "openai with only a vaulted key is available",
                engine: "openai",
                key: None,
                models: &[],
                vaulted: true,
                want: true,
            },
            Case {
                name: "a blank key does not count",
                engine: "openai",
                key: Some("   "),
                models: &[],
                vaulted: false,
                want: false,
            },
        ];
        for case in cases {
            let key = case.key.and_then(ApiKey::new);
            let models: Vec<String> = case.models.iter().map(ToString::to_string).collect();
            assert_eq!(
                api_available(
                    api_by_name(case.engine),
                    key.as_ref(),
                    &models,
                    case.vaulted
                ),
                case.want,
                "{}",
                case.name
            );
        }
    }
}
