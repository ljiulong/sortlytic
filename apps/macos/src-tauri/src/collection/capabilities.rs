use crate::domain::AppResult;

use super::{
  collection_error, DataTypeCapabilityView, FilterExecution, PaginationMode, PlatformCapabilityView,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct EndpointDefinition {
  pub(super) platform: &'static str,
  pub(super) platform_name: &'static str,
  pub(super) data_type: &'static str,
  pub(super) data_type_name: &'static str,
  pub(super) endpoint_key: &'static str,
  pub(super) required_params: &'static [&'static str],
  pub(super) optional_params: &'static [&'static str],
  pub(super) pagination_mode: PaginationMode,
  pub(super) region_filter: FilterExecution,
  pub(super) time_range_filter: FilterExecution,
  pub(super) provider_time_ranges: &'static [&'static str],
  pub(super) max_page_size: i64,
  pub(super) max_request_count: i64,
}

const NO_TIME_RANGES: &[&str] = &[];
const TIKTOK_TIME_RANGES: &[&str] = &["1", "7", "30", "180"];
const DOUYIN_XIAOHONGSHU_TIME_RANGES: &[&str] = &["1", "7", "180"];

const ENDPOINTS: &[EndpointDefinition] = &[
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "tiktok.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range", "page_size"],
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Provider,
    time_range_filter: FilterExecution::Provider,
    provider_time_ranges: TIKTOK_TIME_RANGES,
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
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Local,
    time_range_filter: FilterExecution::Local,
    provider_time_ranges: NO_TIME_RANGES,
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
    optional_params: &[],
    pagination_mode: PaginationMode::Single,
    region_filter: FilterExecution::Unsupported,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 1,
    max_request_count: 1,
  },
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "account_posts",
    data_type_name: "账号作品所属账号",
    endpoint_key: "tiktok.account_posts",
    required_params: &["account_id"],
    optional_params: &["region", "page_size"],
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Provider,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 20,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "tiktok.item_detail",
    required_params: &["item_id"],
    optional_params: &[],
    pagination_mode: PaginationMode::Single,
    region_filter: FilterExecution::Unsupported,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 1,
    max_request_count: 1,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "douyin.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range"],
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Local,
    time_range_filter: FilterExecution::Provider,
    provider_time_ranges: DOUYIN_XIAOHONGSHU_TIME_RANGES,
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
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Local,
    time_range_filter: FilterExecution::Local,
    provider_time_ranges: NO_TIME_RANGES,
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
    optional_params: &[],
    pagination_mode: PaginationMode::Single,
    region_filter: FilterExecution::Unsupported,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 1,
    max_request_count: 1,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "account_posts",
    data_type_name: "账号作品所属账号",
    endpoint_key: "douyin.account_posts",
    required_params: &["account_id"],
    optional_params: &["page_size"],
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Unsupported,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 20,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "douyin.item_detail",
    required_params: &["item_id"],
    optional_params: &[],
    pagination_mode: PaginationMode::Single,
    region_filter: FilterExecution::Unsupported,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 1,
    max_request_count: 1,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "xiaohongshu.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range"],
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Local,
    time_range_filter: FilterExecution::Provider,
    provider_time_ranges: DOUYIN_XIAOHONGSHU_TIME_RANGES,
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
    optional_params: &["region", "time_range"],
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Local,
    time_range_filter: FilterExecution::Local,
    provider_time_ranges: NO_TIME_RANGES,
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
    optional_params: &[],
    pagination_mode: PaginationMode::Single,
    region_filter: FilterExecution::Unsupported,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 1,
    max_request_count: 1,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "account_posts",
    data_type_name: "账号作品所属账号",
    endpoint_key: "xiaohongshu.account_posts",
    required_params: &["account_id"],
    optional_params: &[],
    pagination_mode: PaginationMode::Cursor,
    region_filter: FilterExecution::Unsupported,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 20,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "xiaohongshu.item_detail",
    required_params: &["item_id"],
    optional_params: &[],
    pagination_mode: PaginationMode::Single,
    region_filter: FilterExecution::Unsupported,
    time_range_filter: FilterExecution::Unsupported,
    provider_time_ranges: NO_TIME_RANGES,
    max_page_size: 1,
    max_request_count: 1,
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
    pagination_mode: endpoint.pagination_mode,
    region_filter: endpoint.region_filter,
    time_range_filter: endpoint.time_range_filter,
    provider_time_ranges: endpoint
      .provider_time_ranges
      .iter()
      .map(|value| (*value).to_string())
      .collect(),
    max_page_size: endpoint.max_page_size,
    max_request_count: endpoint.max_request_count,
  }
}

#[cfg(test)]
mod tests {
  use super::list_platform_data_types;

  #[test]
  fn every_platform_exposes_paginated_account_posts() {
    for platform in ["tiktok", "douyin", "xiaohongshu"] {
      let capabilities = list_platform_data_types(platform).expect("MVP 平台应提供采集能力");
      let account_posts = capabilities
        .iter()
        .find(|capability| capability.data_type == "account_posts")
        .expect("每个平台都应支持账号作品采集");

      assert_eq!(account_posts.required_params, vec!["account_id"]);
      assert_eq!(account_posts.pagination_mode, super::PaginationMode::Cursor);
      assert!(account_posts.max_request_count > 1);
    }
  }
}
