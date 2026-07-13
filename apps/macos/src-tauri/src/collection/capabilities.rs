use crate::domain::AppResult;

use super::{collection_error, DataTypeCapabilityView, PlatformCapabilityView};

#[derive(Debug, Clone, Copy)]
pub(super) struct EndpointDefinition {
  pub(super) platform: &'static str,
  pub(super) platform_name: &'static str,
  pub(super) data_type: &'static str,
  pub(super) data_type_name: &'static str,
  pub(super) endpoint_key: &'static str,
  pub(super) required_params: &'static [&'static str],
  pub(super) optional_params: &'static [&'static str],
  pub(super) supports_region: bool,
  pub(super) max_page_size: i64,
  pub(super) max_request_count: i64,
}

const ENDPOINTS: &[EndpointDefinition] = &[
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "tiktok.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 50,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "comments",
    data_type_name: "评论采集",
    endpoint_key: "tiktok.comments",
    required_params: &["item_id"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 100,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "account_profile",
    data_type_name: "账号公开信息",
    endpoint_key: "tiktok.account_profile",
    required_params: &["account_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 50,
  },
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "tiktok.item_detail",
    required_params: &["item_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "douyin.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 50,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "comments",
    data_type_name: "评论采集",
    endpoint_key: "douyin.comments",
    required_params: &["item_id"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 100,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "account_profile",
    data_type_name: "账号公开信息",
    endpoint_key: "douyin.account_profile",
    required_params: &["account_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 50,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "douyin.item_detail",
    required_params: &["item_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "xiaohongshu.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 50,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "comments",
    data_type_name: "评论采集",
    endpoint_key: "xiaohongshu.comments",
    required_params: &["item_id"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 100,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "account_profile",
    data_type_name: "账号公开信息",
    endpoint_key: "xiaohongshu.account_profile",
    required_params: &["account_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 50,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "xiaohongshu.item_detail",
    required_params: &["item_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 100,
  },
];

pub(super) fn list_supported_platforms() -> Vec<PlatformCapabilityView> {
  ["tiktok", "douyin", "xiaohongshu"]
    .iter()
    .filter_map(|platform| {
      let endpoints = ENDPOINTS
        .iter()
        .filter(|endpoint| endpoint.platform == *platform)
        .collect::<Vec<_>>();
      endpoints.first().map(|first| PlatformCapabilityView {
        platform: (*platform).to_string(),
        display_name: first.platform_name.to_string(),
        data_types: endpoints
          .iter()
          .map(|endpoint| endpoint.data_type.to_string())
          .collect(),
      })
    })
    .collect()
}

pub(super) fn list_platform_data_types(platform: &str) -> AppResult<Vec<DataTypeCapabilityView>> {
  let platform = normalize_platform(platform)?;
  let items = ENDPOINTS
    .iter()
    .filter(|endpoint| endpoint.platform == platform)
    .map(endpoint_to_view)
    .collect::<Vec<_>>();

  if items.is_empty() {
    return Err(collection_error("平台不受支持"));
  }

  Ok(items)
}

pub(super) fn endpoint_for(
  platform: &str,
  data_type: &str,
) -> AppResult<&'static EndpointDefinition> {
  let platform = normalize_platform(platform)?;
  let data_type = data_type.trim();

  find_endpoint(&platform, data_type).ok_or_else(|| collection_error("平台或数据类型不受支持"))
}

pub(super) fn find_endpoint(
  platform: &str,
  data_type: &str,
) -> Option<&'static EndpointDefinition> {
  ENDPOINTS
    .iter()
    .find(|endpoint| endpoint.platform == platform && endpoint.data_type == data_type)
}

pub(super) fn normalize_platform(platform: &str) -> AppResult<String> {
  match platform.trim() {
    "tiktok" | "douyin" | "xiaohongshu" => Ok(platform.trim().to_string()),
    _ => Err(collection_error("MVP 只支持 TikTok、抖音、小红书")),
  }
}

fn endpoint_to_view(endpoint: &EndpointDefinition) -> DataTypeCapabilityView {
  DataTypeCapabilityView {
    platform: endpoint.platform.to_string(),
    data_type: endpoint.data_type.to_string(),
    display_name: endpoint.data_type_name.to_string(),
    endpoint_key: endpoint.endpoint_key.to_string(),
    required_params: endpoint
      .required_params
      .iter()
      .map(|value| (*value).to_string())
      .collect(),
    optional_params: endpoint
      .optional_params
      .iter()
      .map(|value| (*value).to_string())
      .collect(),
    supports_region: endpoint.supports_region,
    max_page_size: endpoint.max_page_size,
    max_request_count: endpoint.max_request_count,
  }
}
