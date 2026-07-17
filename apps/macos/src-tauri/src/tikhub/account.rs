use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
  error_for_status, get_tikhub_connector, get_tikhub_json, normalize_tikhub_base_url, number_field,
  read_limited_response_body, reqwest_request_error, safe_body_summary,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::secrets::read_secret_for_backend;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TikhubPriceQuote {
  pub endpoint: String,
  pub request_per_day: i64,
  pub base_unit_price: Option<f64>,
  pub total_price: f64,
  pub currency: String,
  pub quote_json: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TikhubAccountQuota {
  pub balance: f64,
  pub free_credit: f64,
  pub available_credit: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct AccountQuota {
  pub(super) balance: Option<f64>,
  pub(super) free_credit: Option<f64>,
  pub(super) available_credit: Option<f64>,
}

pub(super) fn validate_business_response(response: Value) -> AppResult<Value> {
  let code = response_code(&response).ok_or_else(|| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      "TikHub 响应缺少业务状态码 code",
      AppErrorStage::Collection,
      false,
    )
  })?;
  if code != 200 {
    return Err(business_response_error(code));
  }
  Ok(response)
}

pub fn quote_tikhub_connector_price(
  root_path: impl AsRef<Path>,
  endpoint: &str,
  request_per_day: i64,
) -> AppResult<TikhubPriceQuote> {
  let root_path = root_path.as_ref();
  let connector = get_tikhub_connector(root_path)?
    .filter(|connector| connector.enabled)
    .ok_or_else(|| AppError::validation("TikHub 连接器尚未启用", AppErrorStage::Collection))?;
  let secret_ref_id = connector
    .secret_ref_id
    .as_deref()
    .ok_or_else(|| AppError::validation("TikHub 连接器缺少密钥引用", AppErrorStage::Collection))?;
  let token = read_secret_for_backend(root_path, secret_ref_id, "tikhub")?;
  calculate_tikhub_price(&connector.base_url, &token, endpoint, request_per_day)
}

pub fn get_tikhub_account_quota(root_path: impl AsRef<Path>) -> AppResult<TikhubAccountQuota> {
  let root_path = root_path.as_ref();
  let connector = get_tikhub_connector(root_path)?
    .filter(|connector| connector.enabled)
    .ok_or_else(|| AppError::validation("TikHub 连接器尚未启用", AppErrorStage::Collection))?;
  let secret_ref_id = connector
    .secret_ref_id
    .as_deref()
    .ok_or_else(|| AppError::validation("TikHub 连接器缺少密钥引用", AppErrorStage::Collection))?;
  let token = read_secret_for_backend(root_path, secret_ref_id, "tikhub")?;
  let client = reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(20))
    .build()
    .map_err(reqwest_request_error)?;
  let response = get_tikhub_json(
    &client,
    &connector.base_url,
    "/api/v1/tikhub/user/get_user_info",
    &token,
  )?;
  require_account_quota(&response)
}

fn calculate_tikhub_price(
  base_url: &str,
  token: &str,
  endpoint: &str,
  request_per_day: i64,
) -> AppResult<TikhubPriceQuote> {
  let endpoint = endpoint.trim();
  if !endpoint.starts_with("/api/v1/") || endpoint.contains(['?', '#']) {
    return Err(AppError::validation(
      "计价 endpoint 必须是 /api/v1/ 下不带查询串的路径",
      AppErrorStage::Collection,
    ));
  }
  if request_per_day <= 0 {
    return Err(AppError::validation(
      "每日请求次数必须大于 0",
      AppErrorStage::Collection,
    ));
  }
  let base_url = normalize_tikhub_base_url(Some(base_url.to_string()))?;
  let client = reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(20))
    .build()
    .map_err(reqwest_request_error)?;
  let mut url = reqwest::Url::parse(&format!("{base_url}/api/v1/tikhub/user/calculate_price"))
    .map_err(|_| AppError::validation("TikHub 计价 URL 无效", AppErrorStage::Collection))?;
  url
    .query_pairs_mut()
    .append_pair("endpoint", endpoint)
    .append_pair("request_per_day", &request_per_day.to_string());
  let response = client
    .get(url)
    .bearer_auth(token)
    .send()
    .map_err(reqwest_request_error)?;
  let status = response.status();
  let body = read_limited_response_body(response)?;
  if !status.is_success() {
    return Err(error_for_status(status, safe_body_summary(&body)));
  }
  let response = serde_json::from_str(&body).map_err(|error| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      format!("TikHub 计价响应不是合法 JSON：{error}"),
      AppErrorStage::Collection,
      false,
    )
  })?;
  parse_price_quote(endpoint, request_per_day, response)
}

pub(super) fn parse_account_quota(user_info: &Value) -> AccountQuota {
  let user_data = user_info.get("user_data").unwrap_or(&Value::Null);
  let balance = number_field(user_data, "balance");
  let free_credit = number_field(user_data, "free_credit");
  let available_credit = balance
    .zip(free_credit)
    .map(|(balance, free_credit)| balance + free_credit);
  AccountQuota {
    balance,
    free_credit,
    available_credit,
  }
}

fn require_account_quota(user_info: &Value) -> AppResult<TikhubAccountQuota> {
  let quota = parse_account_quota(user_info);
  let (Some(balance), Some(free_credit), Some(available_credit)) =
    (quota.balance, quota.free_credit, quota.available_credit)
  else {
    return Err(AppError::new(
      AppErrorCode::CostLimitError,
      "TikHub 充值余额或免费额度未知，已禁止发出采集请求",
      AppErrorStage::Collection,
      false,
    ));
  };
  if [balance, free_credit, available_credit]
    .iter()
    .any(|value| !value.is_finite() || *value < 0.0)
  {
    return Err(AppError::new(
      AppErrorCode::CostLimitError,
      "TikHub 账户额度格式异常，已禁止发出采集请求",
      AppErrorStage::Collection,
      false,
    ));
  }
  Ok(TikhubAccountQuota {
    balance,
    free_credit,
    available_credit,
  })
}

pub(super) fn parse_price_quote(
  endpoint: &str,
  request_per_day: i64,
  response: Value,
) -> AppResult<TikhubPriceQuote> {
  let response = validate_business_response(response)?;
  let data = response.get("data").unwrap_or(&Value::Null);
  let total_price = ["total_price", "final_price", "total_cost"]
    .iter()
    .find_map(|field| number_field(data, field))
    .filter(|value| value.is_finite() && *value >= 0.0)
    .ok_or_else(|| {
      AppError::new(
        AppErrorCode::CostLimitError,
        "TikHub 实时计价结果缺少明确总价，已禁止按零成本运行",
        AppErrorStage::Collection,
        false,
      )
    })?;
  let currency = data
    .get("currency")
    .or_else(|| data.get("currency_unit"))
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .unwrap_or("USD")
    .to_ascii_uppercase();
  if currency != "USD" {
    return Err(AppError::new(
      AppErrorCode::CostLimitError,
      "TikHub 计价币种不是 USD，无法与任务预算安全比较",
      AppErrorStage::Collection,
      false,
    ));
  }
  Ok(TikhubPriceQuote {
    endpoint: endpoint.to_string(),
    request_per_day,
    base_unit_price: ["base_price", "unit_price", "original_price"]
      .iter()
      .find_map(|field| number_field(data, field)),
    total_price,
    currency,
    quote_json: response,
  })
}

fn response_code(response: &Value) -> Option<i64> {
  response.get("code").and_then(|value| {
    value
      .as_i64()
      .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
  })
}

fn business_response_error(code: i64) -> AppError {
  let (error_code, retryable) = match code {
    401 | 403 => (AppErrorCode::TikhubAuthError, false),
    402 => (AppErrorCode::CostLimitError, false),
    408 | 425 => (AppErrorCode::TikhubRequestError, true),
    429 => (AppErrorCode::TikhubRateLimit, true),
    500..=599 => (AppErrorCode::TikhubRequestError, true),
    _ => (AppErrorCode::TikhubRequestError, false),
  };

  AppError::new(
    error_code,
    format!("TikHub 业务请求失败，code {code}"),
    AppErrorStage::Collection,
    retryable,
  )
  .with_safe_detail("business_code", code.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn account_quota_includes_recharge_free_and_available_credit() {
    let quota = parse_account_quota(&serde_json::json!({
      "user_data": { "balance": 1.25, "free_credit": 0.05 }
    }));

    assert_eq!(quota.balance, Some(1.25));
    assert_eq!(quota.free_credit, Some(0.05));
    assert_eq!(quota.available_credit, Some(1.30));
    assert_eq!(
      parse_account_quota(&serde_json::json!({
        "user_data": { "balance": 0, "free_credit": 0 }
      }))
      .available_credit,
      Some(0.0)
    );
    assert_eq!(
      parse_account_quota(&serde_json::json!({})).available_credit,
      None
    );
    assert_eq!(
      require_account_quota(&serde_json::json!({
        "user_data": { "balance": 1.25, "free_credit": 0.05 }
      }))
      .expect("完整额度应通过"),
      TikhubAccountQuota {
        balance: 1.25,
        free_credit: 0.05,
        available_credit: 1.30,
      }
    );
    assert_eq!(
      require_account_quota(&serde_json::json!({ "user_data": { "free_credit": 0.05 } }))
        .expect_err("缺少充值余额必须失败")
        .code,
      AppErrorCode::CostLimitError
    );
  }

  #[test]
  fn realtime_price_quote_requires_an_explicit_total() {
    let quote = parse_price_quote(
      "/api/v1/tiktok/app/v3/fetch_video_comments",
      12,
      serde_json::json!({
        "code": 200,
        "data": {
          "endpoint": "/api/v1/tiktok/app/v3/fetch_video_comments",
          "request_per_day": 12,
          "base_price": 0.001,
          "total_price": 0.012,
          "currency": "USD"
        }
      }),
    )
    .expect("明确报价应被解析");

    assert_eq!(quote.total_price, 0.012);
    assert_eq!(quote.base_unit_price, Some(0.001));
    assert_eq!(quote.currency, "USD");

    let error = parse_price_quote(
      "/api/v1/tiktok/app/v3/fetch_video_comments",
      12,
      serde_json::json!({ "code": 200, "data": null }),
    )
    .expect_err("未知价格不得被当作零成本");
    assert_eq!(error.code, AppErrorCode::CostLimitError);
  }
}
