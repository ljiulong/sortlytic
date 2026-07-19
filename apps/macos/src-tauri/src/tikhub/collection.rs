use std::time::Duration;

use super::{
  error_for_status, normalize_tikhub_base_url, read_limited_response_body, reqwest_request_error,
  safe_body_summary,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

mod cursor;
mod request;
mod response;

pub use request::{build_collection_request, RequestMethod, TikHubCollectionRequest};
pub use response::{parse_collection_page, CollectionPage};

pub fn send_collection_request(
  base_url: Option<String>,
  token: &str,
  request: &TikHubCollectionRequest,
) -> AppResult<CollectionPage> {
  if token.trim().is_empty() {
    return Err(AppError::validation(
      "TikHub Token 不能为空",
      AppErrorStage::Collection,
    ));
  }
  if request.paths.is_empty() {
    return Err(AppError::validation(
      "TikHub 请求缺少 endpoint",
      AppErrorStage::Collection,
    ));
  }

  let base_url = normalize_tikhub_base_url(base_url)?;
  let client = reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(30))
    .build()
    .map_err(reqwest_request_error)?;
  let mut last_error = None;

  for (index, path) in request.paths.iter().enumerate() {
    if !path.starts_with("/api/v1/") {
      return Err(AppError::validation(
        "TikHub endpoint 必须位于 /api/v1/ 下",
        AppErrorStage::Collection,
      ));
    }

    match send_single_request(&client, &base_url, token, path, request) {
      Ok(page) => return Ok(page),
      Err(error) if should_try_video_fallback(request, index, &error) => {
        last_error = Some(error);
      }
      Err(error) => return Err(error),
    }
  }

  Err(last_error.unwrap_or_else(|| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      "TikHub 请求未返回可用结果",
      AppErrorStage::Collection,
      true,
    )
  }))
}

fn send_single_request(
  client: &reqwest::blocking::Client,
  base_url: &str,
  token: &str,
  path: &str,
  request: &TikHubCollectionRequest,
) -> AppResult<CollectionPage> {
  let mut url = reqwest::Url::parse(&format!("{base_url}{path}"))
    .map_err(|_| AppError::validation("TikHub endpoint URL 无效", AppErrorStage::Collection))?;
  if url.path() != path || url.query().is_some() || url.fragment().is_some() {
    return Err(AppError::validation(
      "TikHub endpoint 未通过规范化路径校验",
      AppErrorStage::Collection,
    ));
  }
  let request_builder = match request.method {
    RequestMethod::Get => {
      {
        let mut query = url.query_pairs_mut();
        for (key, value) in &request.query {
          query.append_pair(key, value);
        }
      }
      client.get(url).bearer_auth(token)
    }
    RequestMethod::Post => {
      let builder = client.post(url).bearer_auth(token);
      match request.body.as_ref() {
        Some(body) => builder.json(body),
        None => builder,
      }
    }
  };
  let request_builder = match request.idempotency_key.as_deref() {
    Some(idempotency_key) => request_builder.header("Idempotency-Key", idempotency_key),
    None => request_builder,
  };
  let response = request_builder.send().map_err(reqwest_request_error)?;
  let status = response.status();
  let body = read_limited_response_body(response)?;

  if !status.is_success() {
    return Err(error_for_status(status, safe_body_summary(&body)));
  }

  let response = serde_json::from_str(&body).map_err(|error| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      format!("TikHub 返回内容不是合法 JSON：{error}"),
      AppErrorStage::Collection,
      false,
    )
  })?;
  parse_collection_page(request, response)
}

pub(super) fn should_try_video_fallback(
  request: &TikHubCollectionRequest,
  path_index: usize,
  error: &AppError,
) -> bool {
  request.platform == "xiaohongshu"
    && request.data_type == "item_detail"
    && path_index == 0
    && request.paths.len() == 2
    && error.safe_details.get("response_issue").map(String::as_str) == Some("empty_detail_data")
}
