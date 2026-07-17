use std::io::Read;
use std::time::{Duration, Instant};

use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::{StatusCode, Url};
use serde_json::{json, Value};

use crate::api_profiles::{AiApiFormat, AiProviderType};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

const MAX_MODEL_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct ProviderConfig {
  pub provider_type: AiProviderType,
  pub api_format: AiApiFormat,
  pub base_url: String,
  pub model_id: String,
  pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ModelRequest {
  pub system_prompt: String,
  pub user_prompt: String,
  pub schema_name: String,
  pub output_schema: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelResponse {
  pub output_json: Value,
  pub input_tokens: Option<i64>,
  pub output_tokens: Option<i64>,
  pub latency_ms: i64,
}

pub(crate) fn call_model(
  config: &ProviderConfig,
  request: &ModelRequest,
) -> AppResult<ModelResponse> {
  validate_config(config)?;
  let client = Client::builder()
    .connect_timeout(Duration::from_secs(10))
    .timeout(Duration::from_secs(30))
    .redirect(reqwest::redirect::Policy::none())
    .build()
    .map_err(transport_error)?;
  let started_at = Instant::now();
  let response = match config.api_format {
    AiApiFormat::OpenaiCompatible => send_openai(&client, config, request),
    AiApiFormat::AnthropicMessages => send_anthropic(&client, config, request),
    AiApiFormat::Gemini => send_gemini(&client, config, request),
    AiApiFormat::Ollama => send_ollama(&client, config, request),
  }?;
  let latency_ms = i64::try_from(started_at.elapsed().as_millis()).unwrap_or(i64::MAX);
  parse_response(config.api_format, response, latency_ms)
}

pub(crate) fn collection_plan_request(prompt_content: &str, intent_text: &str) -> ModelRequest {
  ModelRequest {
    system_prompt: format!(
      "{}\n\n{}",
      prompt_content,
      authoritative_collection_contract()
    ),
    user_prompt: json!({ "input_json": { "text": intent_text } }).to_string(),
    schema_name: "collection_plan_v3".to_string(),
    output_schema: collection_plan_schema(),
  }
}

fn authoritative_collection_contract() -> &'static str {
  r#"你必须把 input_json.text 转为 collection_plan_v3 JSON，只输出 JSON，不得输出 Markdown。
支持平台仅为 tiktok、douyin、xiaohongshu。数据类型仅为 keyword_search、comments、account_profile、account_posts、item_detail。
每个可执行步骤必须包含唯一 step_key、正确的 endpoint_key、平台、数据类型、params、request_limit 和 output_selected。
端点规则：keyword_search 需要 keyword；comments 需要 item_id；account_profile 和 account_posts 需要 account_id；item_detail 需要 item_id。需要先搜索再采集详情时，把 keyword_search 放入 internal_data_types，并用 depends_on_step_key 与 input_binding 表达依赖。
TikTok 关键词搜索的时间范围只能是 1、7、30、180；抖音和小红书关键词搜索只能是 1、7、180。地区提交值使用 ISO 两位代码。
年龄只允许来自公开接口的明确年龄并按闭区间过滤；性别只允许来自公开接口的明确规范值，禁止根据头像、姓名或简介推断。
用户给出的预算必须精确换算为 USD 微美元写入 budget_limit.amount_micros，不得使用固定默认预算覆盖用户输入。
任何缺失或不确定字段必须写入 missing_fields，不得猜测；requires_user_confirmation 必须为 true。"#
}

fn collection_plan_schema() -> Value {
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "schema_version": { "type": "integer", "const": 3 },
      "platforms": {
        "type": "array",
        "items": { "type": "string", "enum": ["tiktok", "douyin", "xiaohongshu"] }
      },
      "data_types": { "type": "array", "items": { "$ref": "#/$defs/data_type" } },
      "internal_data_types": { "type": "array", "items": { "$ref": "#/$defs/data_type" } },
      "region": {
        "anyOf": [
          { "type": "string" },
          { "type": "null" },
          {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "value": { "type": "string" },
              "validation_status": { "type": "string", "enum": ["verified", "unverified"] }
            },
            "required": ["value", "validation_status"]
          }
        ]
      },
      "keywords": { "type": "array", "items": { "type": "string" } },
      "accounts": { "type": "array", "items": { "type": "string" } },
      "time_range": { "type": ["string", "null"] },
      "age_range": {
        "anyOf": [
          { "type": "null" },
          {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "min": { "type": "integer", "minimum": 0, "maximum": 130 },
              "max": { "type": "integer", "minimum": 0, "maximum": 130 }
            },
            "required": ["min", "max"]
          }
        ]
      },
      "gender_filter": {
        "anyOf": [
          { "type": "null" },
          {
            "type": "array",
            "items": { "type": "string", "enum": ["male", "female", "other"] }
          }
        ]
      },
      "steps": {
        "type": "array",
        "minItems": 1,
        "items": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "step_key": { "type": "string" },
            "role": { "type": "string", "enum": ["entry", "target"] },
            "depends_on_step_key": { "type": ["string", "null"] },
            "input_binding": { "type": ["object", "null"] },
            "endpoint_key": { "type": "string" },
            "platform": { "type": "string", "enum": ["tiktok", "douyin", "xiaohongshu"] },
            "data_type": { "$ref": "#/$defs/data_type" },
            "params": { "type": "object" },
            "request_limit": { "type": "integer", "minimum": 1 },
            "output_selected": { "type": "boolean" }
          },
          "required": [
            "step_key", "role", "depends_on_step_key", "input_binding", "endpoint_key",
            "platform", "data_type", "params", "request_limit", "output_selected"
          ]
        }
      },
      "record_limit": { "type": "integer", "minimum": 1 },
      "request_limit": { "type": "integer", "minimum": 1 },
      "budget_limit": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "currency": { "type": "string", "const": "USD" },
          "amount_micros": { "type": "integer", "minimum": 1 }
        },
        "required": ["currency", "amount_micros"]
      },
      "output_rules": { "type": "object" },
      "missing_fields": { "type": "array", "items": { "type": "string" } },
      "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
      "requires_user_confirmation": { "type": "boolean", "const": true }
    },
    "required": [
      "schema_version", "platforms", "data_types", "internal_data_types", "region",
      "keywords", "accounts", "time_range", "age_range", "gender_filter", "steps",
      "record_limit", "request_limit", "budget_limit", "output_rules", "missing_fields",
      "confidence", "requires_user_confirmation"
    ],
    "$defs": {
      "data_type": {
        "type": "string",
        "enum": ["keyword_search", "comments", "account_profile", "account_posts", "item_detail"]
      }
    }
  })
}

fn validate_config(config: &ProviderConfig) -> AppResult<()> {
  let expected_format = match config.provider_type {
    AiProviderType::Openai | AiProviderType::CustomOpenaiCompatible => {
      AiApiFormat::OpenaiCompatible
    }
    AiProviderType::Anthropic => AiApiFormat::AnthropicMessages,
    AiProviderType::Gemini => AiApiFormat::Gemini,
    AiProviderType::Ollama => AiApiFormat::Ollama,
  };
  if config.api_format != expected_format {
    return Err(model_error(
      AppErrorCode::ModelProtocolError,
      "AI 供应商类型与协议不匹配",
      false,
    ));
  }
  if config.model_id.trim().is_empty() {
    return Err(model_error(
      AppErrorCode::ModelProtocolError,
      "AI 模型 ID 不能为空",
      false,
    ));
  }
  if config.provider_type != AiProviderType::Ollama
    && config.api_key.as_deref().is_none_or(str::is_empty)
  {
    return Err(model_error(
      AppErrorCode::ModelAuthError,
      "AI API Key 缺失，请重新输入后测试连通性",
      false,
    ));
  }
  Ok(())
}

fn send_openai(
  client: &Client,
  config: &ProviderConfig,
  request: &ModelRequest,
) -> AppResult<Response> {
  let endpoint = endpoint_url(&config.base_url, "v1", "chat/completions")?;
  let response_format = if config.provider_type == AiProviderType::Openai {
    json!({
      "type": "json_schema",
      "json_schema": {
        "name": request.schema_name,
        "strict": true,
        "schema": request.output_schema
      }
    })
  } else {
    json!({ "type": "json_object" })
  };
  let body = json!({
    "model": config.model_id,
    "temperature": 0,
    "messages": [
      { "role": "system", "content": request.system_prompt },
      { "role": "user", "content": request.user_prompt }
    ],
    "response_format": response_format
  });
  send(
    client
      .post(endpoint)
      .bearer_auth(required_key(config)?)
      .json(&body),
  )
}

fn send_anthropic(
  client: &Client,
  config: &ProviderConfig,
  request: &ModelRequest,
) -> AppResult<Response> {
  let endpoint = endpoint_url(&config.base_url, "v1", "messages")?;
  let body = json!({
    "model": config.model_id,
    "max_tokens": 4096,
    "temperature": 0,
    "system": request.system_prompt,
    "messages": [{ "role": "user", "content": request.user_prompt }]
  });
  send(
    client
      .post(endpoint)
      .header("x-api-key", required_key(config)?)
      .header("anthropic-version", "2023-06-01")
      .json(&body),
  )
}

fn send_gemini(
  client: &Client,
  config: &ProviderConfig,
  request: &ModelRequest,
) -> AppResult<Response> {
  let model_id = config
    .model_id
    .trim()
    .strip_prefix("models/")
    .unwrap_or(config.model_id.trim());
  if !model_id
    .chars()
    .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
  {
    return Err(model_error(
      AppErrorCode::ModelProtocolError,
      "Gemini 模型 ID 包含不支持的路径字符",
      false,
    ));
  }
  let endpoint = endpoint_url(
    &config.base_url,
    "v1beta",
    &format!("models/{model_id}:generateContent"),
  )?;
  let body = json!({
    "systemInstruction": { "parts": [{ "text": request.system_prompt }] },
    "contents": [{
      "role": "user",
      "parts": [{ "text": request.user_prompt }]
    }],
    "generationConfig": {
      "temperature": 0,
      "responseMimeType": "application/json",
      "responseSchema": request.output_schema
    }
  });
  send(
    client
      .post(endpoint)
      .header("x-goog-api-key", required_key(config)?)
      .json(&body),
  )
}

fn send_ollama(
  client: &Client,
  config: &ProviderConfig,
  request: &ModelRequest,
) -> AppResult<Response> {
  let endpoint = endpoint_url(&config.base_url, "api", "chat")?;
  let body = json!({
    "model": config.model_id,
    "stream": false,
    "messages": [
      { "role": "system", "content": request.system_prompt },
      { "role": "user", "content": request.user_prompt }
    ],
    "format": request.output_schema,
    "options": { "temperature": 0 }
  });
  send(client.post(endpoint).json(&body))
}

fn send(request: RequestBuilder) -> AppResult<Response> {
  request.send().map_err(transport_error)
}

fn parse_response(
  format: AiApiFormat,
  response: Response,
  latency_ms: i64,
) -> AppResult<ModelResponse> {
  let status = response.status();
  let body = read_limited_body(response)?;
  if !status.is_success() {
    return Err(status_error(status));
  }
  let envelope: Value = serde_json::from_str(&body).map_err(|_| {
    model_error(
      AppErrorCode::ModelProtocolError,
      "AI 服务返回了无法解析的协议响应",
      true,
    )
  })?;
  let (content, input_tokens, output_tokens) = match format {
    AiApiFormat::OpenaiCompatible => (
      envelope
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str),
      integer_at(&envelope, "/usage/prompt_tokens"),
      integer_at(&envelope, "/usage/completion_tokens"),
    ),
    AiApiFormat::AnthropicMessages => (
      envelope
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| {
          blocks.iter().find_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
              .then(|| block.get("text").and_then(Value::as_str))
              .flatten()
          })
        }),
      integer_at(&envelope, "/usage/input_tokens"),
      integer_at(&envelope, "/usage/output_tokens"),
    ),
    AiApiFormat::Gemini => (
      envelope
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(Value::as_str),
      integer_at(&envelope, "/usageMetadata/promptTokenCount"),
      integer_at(&envelope, "/usageMetadata/candidatesTokenCount"),
    ),
    AiApiFormat::Ollama => (
      envelope.pointer("/message/content").and_then(Value::as_str),
      integer_at(&envelope, "/prompt_eval_count"),
      integer_at(&envelope, "/eval_count"),
    ),
  };
  let content = content.ok_or_else(|| {
    model_error(
      AppErrorCode::ModelProtocolError,
      "AI 服务响应缺少模型输出内容",
      true,
    )
  })?;
  let output_json = serde_json::from_str(content).map_err(|_| {
    model_error(
      AppErrorCode::ModelSchemaError,
      "AI 模型未返回符合要求的 JSON",
      false,
    )
  })?;
  Ok(ModelResponse {
    output_json,
    input_tokens,
    output_tokens,
    latency_ms,
  })
}

fn endpoint_url(base_url: &str, version: &str, operation: &str) -> AppResult<Url> {
  let mut url = Url::parse(base_url)
    .map_err(|_| model_error(AppErrorCode::ModelProtocolError, "AI Base URL 无效", false))?;
  if !matches!(url.scheme(), "http" | "https")
    || url.host_str().is_none()
    || !url.username().is_empty()
    || url.password().is_some()
    || url.query().is_some()
    || url.fragment().is_some()
  {
    return Err(model_error(
      AppErrorCode::ModelProtocolError,
      "AI Base URL 必须是无凭据、查询串和片段的 HTTP(S) 地址",
      false,
    ));
  }
  let current_path = url.path().trim_end_matches('/');
  let path = if current_path.ends_with(&format!("/{version}")) {
    format!("{current_path}/{operation}")
  } else if current_path.is_empty() || current_path == "/" {
    format!("/{version}/{operation}")
  } else {
    format!("{current_path}/{version}/{operation}")
  };
  url.set_path(&path);
  Ok(url)
}

fn required_key(config: &ProviderConfig) -> AppResult<&str> {
  config
    .api_key
    .as_deref()
    .filter(|key| !key.is_empty())
    .ok_or_else(|| {
      model_error(
        AppErrorCode::ModelAuthError,
        "AI API Key 缺失，请重新输入后测试连通性",
        false,
      )
    })
}

fn read_limited_body(reader: impl Read) -> AppResult<String> {
  let mut reader = reader.take(MAX_MODEL_RESPONSE_BYTES + 1);
  let mut body = Vec::new();
  reader.read_to_end(&mut body).map_err(|_| {
    model_error(
      AppErrorCode::ModelProtocolError,
      "读取 AI 服务响应失败",
      true,
    )
  })?;
  if body.len() as u64 > MAX_MODEL_RESPONSE_BYTES {
    return Err(model_error(
      AppErrorCode::ModelProtocolError,
      "AI 服务响应超过 2 MiB 安全上限",
      false,
    ));
  }
  String::from_utf8(body).map_err(|_| {
    model_error(
      AppErrorCode::ModelProtocolError,
      "AI 服务响应不是合法 UTF-8",
      false,
    )
  })
}

fn integer_at(value: &Value, pointer: &str) -> Option<i64> {
  value.pointer(pointer).and_then(|value| {
    value
      .as_i64()
      .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
  })
}

fn status_error(status: StatusCode) -> AppError {
  let (code, message, retryable) = match status.as_u16() {
    401 | 403 => (
      AppErrorCode::ModelAuthError,
      "AI 服务鉴权失败，请检查 API Key 和访问权限",
      false,
    ),
    429 => (
      AppErrorCode::ModelRateLimit,
      "AI 服务请求过于频繁或额度不足，请稍后重试",
      true,
    ),
    500..=599 => (
      AppErrorCode::ModelProtocolError,
      "AI 服务暂时不可用，请稍后重试",
      true,
    ),
    _ => (
      AppErrorCode::ModelProtocolError,
      "AI 服务拒绝了请求，请检查 Base URL、模型 ID 和协议",
      false,
    ),
  };
  model_error(code, message, retryable).with_safe_detail("http_status", status.as_u16().to_string())
}

fn transport_error(error: reqwest::Error) -> AppError {
  let (message, retryable, kind) = if error.is_timeout() {
    ("AI 服务请求超时", true, "timeout")
  } else if error.is_connect() {
    ("无法连接 AI 服务，请检查 Base URL 和网络", true, "connect")
  } else if error.is_redirect() {
    ("AI 服务返回重定向，已按安全策略拒绝", false, "redirect")
  } else if error.is_body() {
    ("读取 AI 服务响应失败", true, "body")
  } else {
    ("AI 服务请求失败", true, "request")
  };
  model_error(AppErrorCode::ModelProtocolError, message, retryable)
    .with_safe_detail("transport_kind", kind)
}

fn model_error(code: AppErrorCode, message: &str, retryable: bool) -> AppError {
  AppError::new(code, message, AppErrorStage::Ai, retryable)
}

#[cfg(test)]
mod tests {
  use std::io::{Read, Write};
  use std::net::TcpListener;
  use std::thread;

  use super::*;

  const SECRET_SENTINEL: &str = "sk-model-secret-sentinel";
  const BODY_SENTINEL: &str = "provider-body-secret-sentinel";

  #[test]
  fn custom_openai_sends_real_request_and_parses_structured_output() {
    let response_body = json!({
      "choices": [{ "message": { "content": "{\"schema_version\":3}" } }],
      "usage": { "prompt_tokens": 23, "completion_tokens": 7 }
    })
    .to_string();
    let (base_url, server) = serve_once(200, response_body, |request| {
      assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
      assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer sk-model-secret-sentinel"));
      assert!(request.contains("json_object"));
      assert!(request.contains("真实提示词正文"));
    });

    let response = call_model(
      &config(AiProviderType::CustomOpenaiCompatible, base_url),
      &model_request(),
    )
    .expect("OpenAI-compatible request should succeed");
    server.join().expect("test server should finish");

    assert_eq!(response.output_json["schema_version"], 3);
    assert_eq!(response.input_tokens, Some(23));
    assert_eq!(response.output_tokens, Some(7));
  }

  #[test]
  fn provider_error_never_exposes_key_or_response_body() {
    let (base_url, server) = serve_once(401, format!(r#"{{"error":"{BODY_SENTINEL}"}}"#), |_| {});
    let error = call_model(
      &config(AiProviderType::CustomOpenaiCompatible, base_url),
      &model_request(),
    )
    .expect_err("401 must fail");
    server.join().expect("test server should finish");

    assert_eq!(error.code, AppErrorCode::ModelAuthError);
    let serialized = serde_json::to_string(&error).expect("error should serialize");
    assert!(!serialized.contains(SECRET_SENTINEL));
    assert!(!serialized.contains(BODY_SENTINEL));
  }

  #[test]
  fn invalid_model_json_fails_schema_closed() {
    let response_body = json!({
      "choices": [{ "message": { "content": "```json\n{}\n```" } }]
    })
    .to_string();
    let (base_url, server) = serve_once(200, response_body, |_| {});
    let error = call_model(
      &config(AiProviderType::CustomOpenaiCompatible, base_url),
      &model_request(),
    )
    .expect_err("markdown-wrapped JSON must not bypass the schema boundary");
    server.join().expect("test server should finish");

    assert_eq!(error.code, AppErrorCode::ModelSchemaError);
  }

  #[test]
  fn anthropic_gemini_and_ollama_protocols_return_json() {
    let cases = [
      (
        AiProviderType::Anthropic,
        AiApiFormat::AnthropicMessages,
        "/v1/messages",
        json!({
          "content": [{ "type": "text", "text": "{\"ok\":true}" }],
          "usage": { "input_tokens": 4, "output_tokens": 2 }
        }),
      ),
      (
        AiProviderType::Gemini,
        AiApiFormat::Gemini,
        "/v1beta/models/model-test:generateContent",
        json!({
          "candidates": [{ "content": { "parts": [{ "text": "{\"ok\":true}" }] } }],
          "usageMetadata": { "promptTokenCount": 4, "candidatesTokenCount": 2 }
        }),
      ),
      (
        AiProviderType::Ollama,
        AiApiFormat::Ollama,
        "/api/chat",
        json!({
          "message": { "content": "{\"ok\":true}" },
          "prompt_eval_count": 4,
          "eval_count": 2
        }),
      ),
    ];

    for (provider_type, api_format, expected_path, body) in cases {
      let (base_url, server) = serve_once(200, body.to_string(), move |request| {
        assert!(request.starts_with(&format!("POST {expected_path} HTTP/1.1")));
      });
      let response = call_model(
        &ProviderConfig {
          provider_type,
          api_format,
          base_url,
          model_id: "model-test".to_string(),
          api_key: Some(SECRET_SENTINEL.to_string()),
        },
        &model_request(),
      )
      .expect("supported provider protocol should parse JSON");
      server.join().expect("test server should finish");

      assert_eq!(response.output_json, json!({ "ok": true }));
      assert_eq!(response.input_tokens, Some(4));
      assert_eq!(response.output_tokens, Some(2));
    }
  }

  fn config(provider_type: AiProviderType, base_url: String) -> ProviderConfig {
    ProviderConfig {
      provider_type,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url,
      model_id: "model-test".to_string(),
      api_key: Some(SECRET_SENTINEL.to_string()),
    }
  }

  fn model_request() -> ModelRequest {
    ModelRequest {
      system_prompt: "真实提示词正文".to_string(),
      user_prompt: r#"{"input_json":{"text":"采集 TikTok"}}"#.to_string(),
      schema_name: "collection_plan".to_string(),
      output_schema: json!({
        "type": "object",
        "properties": { "schema_version": { "type": "integer" } },
        "required": ["schema_version"],
        "additionalProperties": false
      }),
    }
  }

  fn serve_once(
    status: u16,
    body: String,
    inspect: impl FnOnce(&str) + Send + 'static,
  ) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let address = listener
      .local_addr()
      .expect("test server address should resolve");
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().expect("test server should accept");
      let mut request = Vec::new();
      let mut buffer = [0_u8; 8192];
      loop {
        let bytes_read = stream
          .read(&mut buffer)
          .expect("request should be readable");
        if bytes_read == 0 {
          break;
        }
        request.extend_from_slice(&buffer[..bytes_read]);
        let text = String::from_utf8_lossy(&request);
        if let Some(header_end) = text.find("\r\n\r\n") {
          let content_length = text[..header_end]
            .lines()
            .find_map(|line| {
              line
                .to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
          if request.len() >= header_end + 4 + content_length {
            break;
          }
        }
      }
      let request = String::from_utf8_lossy(&request).into_owned();
      inspect(&request);
      let reason = if status == 200 { "OK" } else { "Unauthorized" };
      write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
      )
      .expect("response should be writable");
    });
    (format!("http://{address}"), server)
  }
}
