use uuid::Uuid;

use crate::api_profiles::{AiApiFormat, AiProviderType, ApiProfileStatus, CredentialProviderType};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

pub(super) fn validate_ai_format(provider: AiProviderType, format: AiApiFormat) -> AppResult<()> {
  let expected = match provider {
    AiProviderType::Openai | AiProviderType::CustomOpenaiCompatible => {
      AiApiFormat::OpenaiCompatible
    }
    AiProviderType::Anthropic => AiApiFormat::AnthropicMessages,
    AiProviderType::Gemini => AiApiFormat::Gemini,
    AiProviderType::Ollama => AiApiFormat::Ollama,
  };
  if format == expected {
    Ok(())
  } else {
    Err(error("AI 供应商类型与 API 格式不匹配"))
  }
}

pub(super) fn credential_type(provider: AiProviderType) -> CredentialProviderType {
  match provider {
    AiProviderType::Openai => CredentialProviderType::Openai,
    AiProviderType::Anthropic => CredentialProviderType::Anthropic,
    AiProviderType::Gemini => CredentialProviderType::Gemini,
    AiProviderType::CustomOpenaiCompatible => CredentialProviderType::CustomOpenaiCompatible,
    AiProviderType::Ollama => CredentialProviderType::Ollama,
  }
}

pub(super) fn completeness_status(
  provider: AiProviderType,
  base_url: &str,
  model: &str,
  has_key: bool,
) -> ApiProfileStatus {
  if !base_url.is_empty() && !model.is_empty() && (provider == AiProviderType::Ollama || has_key) {
    ApiProfileStatus::Untested
  } else {
    ApiProfileStatus::NeedsRebind
  }
}

pub(super) fn key_status(has_key: bool) -> ApiProfileStatus {
  if has_key {
    ApiProfileStatus::Untested
  } else {
    ApiProfileStatus::NeedsRebind
  }
}

pub(super) fn tikhub_url(value: &str) -> AppResult<String> {
  let value = value.trim().trim_end_matches('/');
  match value {
    "https://api.tikhub.io" | "https://api.tikhub.dev" => Ok(value.to_string()),
    _ => Err(error(
      "TikHub Base URL 只允许 https://api.tikhub.io 或 https://api.tikhub.dev",
    )),
  }
}

pub(super) fn ai_url(provider: AiProviderType, value: &str) -> AppResult<String> {
  let value = value.trim().trim_end_matches('/');
  let value = if !value.is_empty() {
    value.to_string()
  } else {
    match provider {
      AiProviderType::Openai => "https://api.openai.com/v1".to_string(),
      AiProviderType::Anthropic => "https://api.anthropic.com".to_string(),
      AiProviderType::Gemini => "https://generativelanguage.googleapis.com".to_string(),
      AiProviderType::Ollama => "http://localhost:11434".to_string(),
      AiProviderType::CustomOpenaiCompatible => String::new(),
    }
  };
  if value.is_empty() {
    return Ok(value);
  }
  validate_ai_url(provider, &value)?;
  Ok(value)
}

pub(super) fn validate_ai_url(provider: AiProviderType, value: &str) -> AppResult<()> {
  let url = reqwest::Url::parse(value).map_err(|_| error("AI Base URL 不是完整的 HTTP(S) 地址"))?;
  if !matches!(url.scheme(), "http" | "https")
    || url.host_str().is_none()
    || !url.username().is_empty()
    || url.password().is_some()
    || url.query().is_some()
    || url.fragment().is_some()
  {
    return Err(error(
      "AI Base URL 必须包含主机且不能携带凭据、查询串或片段",
    ));
  }
  let host = url.host_str().unwrap_or_default();
  let official_url_valid = |expected_host: &str| {
    url.scheme() == "https"
      && host.eq_ignore_ascii_case(expected_host)
      && url.port_or_known_default() == Some(443)
  };
  let valid = match provider {
    AiProviderType::Openai => official_url_valid("api.openai.com"),
    AiProviderType::Anthropic => official_url_valid("api.anthropic.com"),
    AiProviderType::Gemini => official_url_valid("generativelanguage.googleapis.com"),
    AiProviderType::CustomOpenaiCompatible => {
      url.scheme() == "https" || cfg!(test) && url.scheme() == "http" && is_loopback_host(host)
    }
    AiProviderType::Ollama => {
      url.scheme() == "https" || url.scheme() == "http" && is_loopback_host(host)
    }
  };
  if valid {
    Ok(())
  } else {
    Err(error(match provider {
      AiProviderType::Openai => "OpenAI 配置只允许官方 HTTPS 端点 api.openai.com",
      AiProviderType::Anthropic => "Anthropic 配置只允许官方 HTTPS 端点 api.anthropic.com",
      AiProviderType::Gemini => {
        "Gemini 配置只允许官方 HTTPS 端点 generativelanguage.googleapis.com"
      }
      AiProviderType::CustomOpenaiCompatible => "自定义 AI Base URL 必须使用 HTTPS",
      AiProviderType::Ollama => {
        "Ollama 的 HTTP Base URL 只允许本机回环地址，远程地址必须使用 HTTPS"
      }
    }))
  }
}

pub(super) fn same_url_authority(left: &str, right: &str) -> bool {
  let Ok(left) = reqwest::Url::parse(left) else {
    return false;
  };
  let Ok(right) = reqwest::Url::parse(right) else {
    return false;
  };
  left.scheme() == right.scheme()
    && left
      .host_str()
      .zip(right.host_str())
      .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
    && left.port_or_known_default() == right.port_or_known_default()
}

fn is_loopback_host(host: &str) -> bool {
  host.eq_ignore_ascii_case("localhost")
    || host
      .parse::<std::net::IpAddr>()
      .is_ok_and(|address| address.is_loopback())
}

pub(super) fn required(value: &str, label: &str) -> AppResult<String> {
  let value = value.trim();
  if value.is_empty() {
    Err(error(format!("{label}不能为空")))
  } else {
    Ok(value.to_string())
  }
}

pub(super) fn optional_id(value: Option<String>) -> AppResult<Option<String>> {
  let value = value
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());
  if let Some(value) = value.as_deref() {
    Uuid::parse_str(value).map_err(|_| error("API 配置 ID 必须是 UUID"))?;
  }
  Ok(value)
}

pub(super) fn secret(value: Option<String>) -> Option<String> {
  value
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

pub(super) fn next_revision(value: u64) -> AppResult<u64> {
  value
    .checked_add(1)
    .ok_or_else(|| error("API 配置修订号已达到上限"))
}

pub(super) fn error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    message,
    AppErrorStage::SecretStore,
    false,
  )
}
