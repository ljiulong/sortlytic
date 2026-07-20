use std::io::Read;
use std::time::{Duration, Instant};

use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::{StatusCode, Url};
use serde_json::{json, Value};

use crate::api_profiles::{AiApiFormat, AiProviderType};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

use super::collection_intent_schema::collection_intent_schema;
use super::collection_plan_schema::collection_plan_schema;

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
    schema_name: "collection_plan_v4".to_string(),
    output_schema: collection_plan_schema(),
  }
}

#[allow(dead_code)]
pub(crate) fn collection_intent_request(prompt_content: &str, intent_text: &str) -> ModelRequest {
  ModelRequest {
    system_prompt: format!("{prompt_content}\n\n{}", authoritative_intent_contract()),
    user_prompt: json!({ "input_json": { "text": intent_text } }).to_string(),
    schema_name: "collection_intent_v1".to_string(),
    output_schema: collection_intent_schema(),
  }
}

pub(crate) fn connection_test_request() -> ModelRequest {
  ModelRequest {
    system_prompt:
      r#"这是连通性测试，不执行采集任务。只返回 JSON：{"ok":true}，不得返回其他字段或文本。"#
        .to_string(),
    user_prompt: r#"{"ping":"sortlytic"}"#.to_string(),
    schema_name: "sortlytic_connection_test".to_string(),
    output_schema: json!({
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "ok": { "type": "boolean", "const": true }
      },
      "required": ["ok"]
    }),
  }
}

fn authoritative_collection_contract() -> &'static str {
  r#"你必须把 input_json.text 转为 collection_plan_v4 账号计划 JSON，只输出 JSON，不得输出 Markdown。
每条计划只能选择 tiktok、douyin、xiaohongshu 中的一个平台和一个 account_source。三平台支持 user_search、content_search_authors、direct_account、item_author、comment_authors；followers 和 followings 只支持 TikTok、抖音；similar_accounts 只支持 TikTok。
发现步骤 operation_key 必须为 discover.<account_source>，role 为 discovery。补全步骤只能使用 enrich.profile、enrich.extended_demographics、enrich.account_country、enrich.account_posts，role 为 enrichment，并依赖 discover。
TikTok 和小红书不得选择 gender、age；抖音选择 gender 或 age 时必须共用 enrich.extended_demographics。TikTok 的 country_region 使用 enrich.account_country。不得生成平台不支持的来源、字段或补全步骤。
基础身份字段由系统固定采集，不放入 selected_fields。selected_fields 只包含用户明确要求的公开业务字段；不得把分页游标、日志 ID、缓存 URL、临时 CDN 地址或签名令牌作为业务字段。
output_rules 必须区分未选择的“任务未设置”和已选择但未返回的“未采集到”，并要求逐字段证据。数值 0 是合法值；性别 0、年龄 -1 或超出 0 至 130 为未知。
年龄和性别只允许使用公开接口明确返回值，禁止根据头像、姓名、简介或模型推断；筛选必须在账号合并后执行，未知值不能通过已启用筛选。
用户给出的预算必须精确换算为 USD 微美元写入 budget_limit.amount_micros，不得使用固定默认预算覆盖用户输入。
任何缺失或不确定字段必须写入 missing_fields，不得猜测；requires_user_confirmation 必须为 true。"#
}

fn authoritative_intent_contract() -> &'static str {
  r#"以下规则是当前 collection_intent_v1 的最高优先级契约；若前文仍提到完整执行计划，以本段为准。
你只负责从 input_json.text 提取账号采集意图和翻译实际检索词，只输出一个 JSON 对象，不得输出 Markdown。
必须完整输出 schema_version、platform、account_source、source_input、query_locale、region_code、selected_fields、time_range_days、age_range、gender_filter、record_limit、budget_limit_micros、missing_fields、confidence。schema_version 必须为 1。
不得输出 endpoint_key、端点、步骤、步骤依赖、请求参数白名单、分页、补全和成本估算；这些执行安全信息全部由后端能力目录确定。
关键词搜索、用户搜索和内容搜索的 source_input 必须翻译为目标地区适合平台检索的一个主语言，query_locale 使用 language-REGION 格式，例如英国为 en-GB。翻译只改变 source_input，不得改写原始输入证据。
用户名、账号 ID、作品 ID、URL、分享链接必须原样保留，禁止翻译。品牌名或专有名词只有存在明确通用本地写法时才转换；不确定时保留原文并写入 missing_fields。
region_code 只能使用明确地区对应的大写 ISO 两位代码，英国必须是 GB，不得写 UK。检索词语言不能作为账号地区证据。
没有明确平台、地区、账号来源、来源输入、目标语言、记录数或预算时，对应字段写 null，并把字段名写入 missing_fields；不得擅自猜测。预算必须换算成正整数 USD 微美元。"#
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
      "choices": [{ "message": { "content": "{\"schema_version\":4}" } }],
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

    assert_eq!(response.output_json["schema_version"], 4);
    assert_eq!(response.input_tokens, Some(23));
    assert_eq!(response.output_tokens, Some(7));
  }

  #[test]
  fn official_openai_sends_the_strict_collection_plan_schema() {
    let response_body = json!({
      "choices": [{ "message": { "content": "{\"schema_version\":4}" } }]
    })
    .to_string();
    let (base_url, server) = serve_once(200, response_body, |request| {
      let body = request
        .split("\r\n\r\n")
        .nth(1)
        .expect("request body should exist");
      let payload: Value = serde_json::from_str(body).expect("request body should be JSON");
      assert_eq!(
        payload.pointer("/response_format/type"),
        Some(&json!("json_schema"))
      );
      assert_eq!(
        payload.pointer("/response_format/json_schema/strict"),
        Some(&json!(true))
      );
      assert_eq!(
        payload.pointer(
          "/response_format/json_schema/schema/properties/steps/items/properties/params/additionalProperties"
        ),
        Some(&json!(false))
      );
      assert_eq!(
        payload.pointer(
          "/response_format/json_schema/schema/properties/output_rules/additionalProperties"
        ),
        Some(&json!(false))
      );
    });

    call_model(
      &config(AiProviderType::Openai, base_url),
      &collection_plan_request("真实提示词正文", "采集 TikTok 汽车账号"),
    )
    .expect("official OpenAI request should succeed");
    server.join().expect("test server should finish");
  }

  #[test]
  fn connection_test_request_requires_a_closed_ok_true_result() {
    let request = connection_test_request();

    assert!(request.system_prompt.contains(r#"{"ok":true}"#));
    assert_eq!(request.schema_name, "sortlytic_connection_test");
    assert_eq!(
      request.output_schema,
      json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "ok": { "type": "boolean", "const": true }
        },
        "required": ["ok"]
      })
    );
  }

  #[test]
  fn collection_intent_request_excludes_execution_plan_authority() {
    let request = collection_intent_request("当前激活提示词", "用中文找英国 TikTok 宠物用品账号");

    assert_eq!(request.schema_name, "collection_intent_v1");
    assert_eq!(
      request.output_schema["properties"]["schema_version"]["const"],
      json!(1)
    );
    assert!(request.output_schema["properties"].get("steps").is_none());
    assert!(request.output_schema["properties"]
      .get("endpoint_key")
      .is_none());
    assert!(request.output_schema["properties"]
      .get("cost_estimate")
      .is_none());
    assert!(request.system_prompt.contains("当前激活提示词"));
    assert!(request.system_prompt.contains("不得输出 endpoint_key"));
    assert!(request.system_prompt.contains("分页、补全和成本估算"));
    assert!(request.system_prompt.contains("翻译为目标地区"));
    assert!(request.system_prompt.contains("URL、分享链接"));
    assert!(request
      .user_prompt
      .contains("用中文找英国 TikTok 宠物用品账号"));
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
