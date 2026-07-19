use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::domain::AppResult;

use super::collection_error;

pub const ACCOUNT_CAPABILITY_CATALOG_VERSION: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformCapabilityView {
  pub platform: String,
  pub display_name: String,
  pub data_types: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaginationMode {
  Single,
  Cursor,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilterExecution {
  Provider,
  Local,
  Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataTypeCapabilityView {
  pub platform: String,
  pub data_type: String,
  pub display_name: String,
  pub endpoint_key: String,
  pub required_params: Vec<String>,
  pub optional_params: Vec<String>,
  pub pagination_mode: PaginationMode,
  pub region_filter: FilterExecution,
  pub time_range_filter: FilterExecution,
  pub provider_time_ranges: Vec<String>,
  pub max_page_size: i64,
  pub max_request_count: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountSourceInputKind {
  Keyword,
  Account,
  Item,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountFieldAvailability {
  Direct,
  Enrichment,
  Conditional,
  Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountFieldValueType {
  Text,
  Integer,
  Boolean,
  TextList,
  Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountSourceCapabilityView {
  pub key: String,
  pub label: String,
  pub display_name: String,
  pub description: String,
  pub input_kind: AccountSourceInputKind,
  pub endpoint_key: String,
  pub pagination_mode: PaginationMode,
  pub max_page_size: i64,
  pub max_request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountFieldGroupView {
  pub key: String,
  pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountFieldCapabilityView {
  pub key: String,
  pub group: String,
  pub label: String,
  pub display_name: String,
  pub description: String,
  pub value_type: AccountFieldValueType,
  pub availability: AccountFieldAvailability,
  pub default_selected: bool,
  pub required_operation_keys: Vec<String>,
  pub missing_reason: Option<String>,
  pub supported_platforms: Vec<String>,
  pub covered_by_source_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountCollectionCapabilityView {
  pub catalog_version: i64,
  pub platform: String,
  pub display_name: String,
  pub account_sources: Vec<AccountSourceCapabilityView>,
  pub field_groups: Vec<AccountFieldGroupView>,
  pub fields: Vec<AccountFieldCapabilityView>,
}

type FieldDefinition = (
  &'static str,
  &'static str,
  &'static str,
  &'static str,
  AccountFieldValueType,
  bool,
);

const FIELD_GROUPS: &[(&str, &str)] = &[
  ("profile", "账号资料"),
  ("demographics", "人口属性"),
  ("statistics", "账号统计"),
  ("activity", "账号活跃"),
  ("platform_specific", "平台特有"),
];

#[rustfmt::skip]
const DIRECT_PROFILE_FIELDS: &[&str] = &[
  "secure_user_id", "avatar_url", "profile_url", "bio", "website_url", "verification_status",
  "verification_reason", "account_type", "private_account", "language", "profile_tags",
  "followers_count", "following_count", "friends_count", "posts_count", "likes_received_count",
  "liked_content_count", "account_created_at", "live_status", "live_room_id", "username_modified_at",
  "nickname_modified_at", "commerce_status", "commerce_category", "seller_status", "organization_status",
  "comments_permission", "duet_permission", "stitch_permission", "download_permission", "favorites_visibility",
  "following_visibility", "playlist_visibility",
];

// 静态目录按一字段一行维护，避免纯声明数据被格式化为数百行并越过源码行数门禁。
#[rustfmt::skip]
const FIELD_DEFINITIONS: &[FieldDefinition] = &[
  ("secure_user_id", "profile", "安全用户 ID", "平台返回的安全用户标识。", AccountFieldValueType::Text, false),
  ("avatar_url", "profile", "头像", "账号公开头像地址。", AccountFieldValueType::Text, true),
  ("profile_url", "profile", "主页链接", "账号公开主页地址。", AccountFieldValueType::Text, true),
  ("bio", "profile", "个人简介", "账号公开简介或签名。", AccountFieldValueType::Text, true),
  ("website_url", "profile", "外部网站", "账号公开展示的网站链接。", AccountFieldValueType::Text, false),
  ("verification_status", "profile", "认证状态", "平台明确返回的账号认证状态。", AccountFieldValueType::Boolean, true),
  ("verification_reason", "profile", "认证说明", "企业或个人认证说明。", AccountFieldValueType::Text, false),
  ("account_type", "profile", "账号类型", "普通、企业或创作者等平台账号类型。", AccountFieldValueType::Text, false),
  ("private_account", "profile", "私密账号", "平台明确返回的私密账号状态。", AccountFieldValueType::Boolean, false),
  ("language", "profile", "账号语言", "账号公开语言设置。", AccountFieldValueType::Text, false),
  ("country_region", "profile", "国家或地区", "平台接口明确返回的账号国家或地区。", AccountFieldValueType::Text, true),
  ("profile_tags", "profile", "资料标签", "平台公开展示的账号资料标签。", AccountFieldValueType::TextList, false),
  ("gender", "demographics", "性别", "只使用平台接口明确返回的性别。", AccountFieldValueType::Text, false),
  ("age", "demographics", "年龄", "只使用平台接口明确返回的有效年龄。", AccountFieldValueType::Integer, false),
  ("followers_count", "statistics", "粉丝数", "账号公开粉丝数量。", AccountFieldValueType::Integer, true),
  ("following_count", "statistics", "关注数", "账号公开关注数量。", AccountFieldValueType::Integer, true),
  ("friends_count", "statistics", "好友数", "平台明确返回的好友数量。", AccountFieldValueType::Integer, false),
  ("posts_count", "statistics", "作品数", "账号公开作品、视频或笔记数量。", AccountFieldValueType::Integer, true),
  ("likes_received_count", "statistics", "获赞数", "账号累计获赞或平台等价公开指标。", AccountFieldValueType::Integer, false),
  ("liked_content_count", "statistics", "点赞内容数", "账号公开点赞内容数量。", AccountFieldValueType::Integer, false),
  ("account_created_at", "activity", "账号创建时间", "平台明确返回的账号创建时间。", AccountFieldValueType::Timestamp, false),
  ("last_posted_at", "activity", "最近发文时间", "从账号作品列表确认的最近发文时间。", AccountFieldValueType::Timestamp, true),
  ("live_status", "activity", "直播状态", "账号当前是否正在直播。", AccountFieldValueType::Boolean, false),
  ("live_room_id", "activity", "直播间 ID", "平台明确返回的直播间标识。", AccountFieldValueType::Text, false),
  ("username_modified_at", "activity", "账号名修改时间", "平台明确返回的账号名修改时间。", AccountFieldValueType::Timestamp, false),
  ("nickname_modified_at", "activity", "昵称修改时间", "平台明确返回的昵称修改时间。", AccountFieldValueType::Timestamp, false),
  ("commerce_status", "platform_specific", "商业账号", "平台明确返回的商业账号状态。", AccountFieldValueType::Boolean, false),
  ("commerce_category", "platform_specific", "商业分类", "商业账号公开分类。", AccountFieldValueType::Text, false),
  ("seller_status", "platform_specific", "卖家状态", "平台明确返回的卖家状态。", AccountFieldValueType::Boolean, false),
  ("organization_status", "platform_specific", "组织账号", "平台明确返回的组织账号状态。", AccountFieldValueType::Boolean, false),
  ("comments_permission", "platform_specific", "评论权限", "账号公开评论权限设置。", AccountFieldValueType::Text, false),
  ("duet_permission", "platform_specific", "合拍权限", "TikTok 账号公开合拍权限。", AccountFieldValueType::Text, false),
  ("stitch_permission", "platform_specific", "拼接权限", "TikTok 账号公开拼接权限。", AccountFieldValueType::Text, false),
  ("download_permission", "platform_specific", "下载权限", "账号公开下载权限设置。", AccountFieldValueType::Text, false),
  ("favorites_visibility", "platform_specific", "收藏可见性", "账号公开收藏列表可见性。", AccountFieldValueType::Text, false),
  ("following_visibility", "platform_specific", "关注可见性", "账号公开关注列表可见性。", AccountFieldValueType::Text, false),
  ("playlist_visibility", "platform_specific", "播放列表可见性", "账号公开播放列表可见性。", AccountFieldValueType::Boolean, false),
  ("live_level", "platform_specific", "直播等级", "抖音接口明确返回的直播等级。", AccountFieldValueType::Integer, false),
  ("live_badge", "platform_specific", "直播牌子", "抖音接口明确返回的直播间牌子。", AccountFieldValueType::Text, false),
];

pub fn get_account_collection_capabilities(
  platform: &str,
) -> AppResult<AccountCollectionCapabilityView> {
  let platform = normalize_platform(platform)?;
  let fields = FIELD_DEFINITIONS
    .iter()
    .map(|definition| field_capability(&platform, definition))
    .collect::<Vec<_>>();
  debug_assert_eq!(
    fields
      .iter()
      .map(|field| &field.key)
      .collect::<BTreeSet<_>>()
      .len(),
    fields.len()
  );

  Ok(AccountCollectionCapabilityView {
    catalog_version: ACCOUNT_CAPABILITY_CATALOG_VERSION,
    display_name: platform_display_name(&platform).to_string(),
    account_sources: account_sources(&platform),
    field_groups: FIELD_GROUPS
      .iter()
      .map(|(key, name)| AccountFieldGroupView {
        key: (*key).to_string(),
        display_name: (*name).to_string(),
      })
      .collect(),
    fields,
    platform,
  })
}

fn account_sources(platform: &str) -> Vec<AccountSourceCapabilityView> {
  let mut sources = vec![
    source(
      "user_search",
      "搜索用户",
      "按关键词搜索公开用户账号。",
      AccountSourceInputKind::Keyword,
      platform,
      "user_search",
      PaginationMode::Cursor,
      20,
      100,
    ),
    source(
      "content_search_authors",
      "搜索内容作者",
      "从关键词内容搜索结果提取作者账号。",
      AccountSourceInputKind::Keyword,
      platform,
      "keyword_search",
      PaginationMode::Cursor,
      20,
      100,
    ),
    source(
      "direct_account",
      "指定账号",
      "读取指定公开账号并补全资料。",
      AccountSourceInputKind::Account,
      platform,
      "account_profile",
      PaginationMode::Single,
      1,
      1,
    ),
    source(
      "item_author",
      "指定作品作者",
      "从指定作品、视频或笔记提取作者。",
      AccountSourceInputKind::Item,
      platform,
      "item_detail",
      PaginationMode::Single,
      1,
      1,
    ),
    source(
      "comment_authors",
      "评论用户",
      "从指定作品的公开评论提取账号。",
      AccountSourceInputKind::Item,
      platform,
      "comments",
      PaginationMode::Cursor,
      20,
      200,
    ),
  ];
  if platform != "xiaohongshu" {
    sources.extend([
      source(
        "followers",
        "指定账号的粉丝",
        "读取指定账号的公开粉丝列表。",
        AccountSourceInputKind::Account,
        platform,
        "followers",
        PaginationMode::Cursor,
        20,
        200,
      ),
      source(
        "followings",
        "指定账号的关注",
        "读取指定账号的公开关注列表。",
        AccountSourceInputKind::Account,
        platform,
        "followings",
        PaginationMode::Cursor,
        20,
        200,
      ),
    ]);
  }
  if platform == "tiktok" {
    sources.push(source(
      "similar_accounts",
      "相似账号推荐",
      "读取与指定账号相似的公开账号。",
      AccountSourceInputKind::Account,
      platform,
      "similar_accounts",
      PaginationMode::Cursor,
      20,
      100,
    ));
  }
  sources
}

#[allow(clippy::too_many_arguments)]
fn source(
  key: &str,
  display_name: &str,
  description: &str,
  input_kind: AccountSourceInputKind,
  platform: &str,
  endpoint_suffix: &str,
  pagination_mode: PaginationMode,
  max_page_size: i64,
  max_request_count: i64,
) -> AccountSourceCapabilityView {
  AccountSourceCapabilityView {
    key: key.to_string(),
    label: display_name.to_string(),
    display_name: display_name.to_string(),
    description: description.to_string(),
    input_kind,
    endpoint_key: format!("{platform}.{endpoint_suffix}"),
    pagination_mode,
    max_page_size,
    max_request_count,
  }
}

fn field_capability(platform: &str, definition: &FieldDefinition) -> AccountFieldCapabilityView {
  let (key, group, display_name, description, value_type, default_candidate) = *definition;
  let (availability, operations, missing_reason) = field_support(platform, key);
  AccountFieldCapabilityView {
    key: key.to_string(),
    group: group.to_string(),
    label: display_name.to_string(),
    display_name: display_name.to_string(),
    description: description.to_string(),
    value_type,
    availability,
    default_selected: default_candidate && availability != AccountFieldAvailability::Unsupported,
    required_operation_keys: operations
      .iter()
      .map(|value| (*value).to_string())
      .collect(),
    missing_reason: missing_reason.map(ToString::to_string),
    supported_platforms: ["tiktok", "douyin", "xiaohongshu"]
      .into_iter()
      .filter(|candidate| {
        field_support(candidate, key).0 != AccountFieldAvailability::Unsupported
      })
      .map(ToString::to_string)
      .collect(),
    covered_by_source_keys: account_sources(platform)
      .into_iter()
      .filter(|source| source_covers_field(platform, &source.key, key))
      .map(|source| source.key)
      .collect(),
  }
}

pub(super) fn source_covers_field(platform: &str, account_source: &str, field_key: &str) -> bool {
  if account_source == "direct_account" {
    return DIRECT_PROFILE_FIELDS.contains(&field_key)
      || (platform == "douyin" && field_key == "country_region");
  }
  matches!(
    (platform, account_source, field_key),
    (
      "douyin",
      "user_search",
      "secure_user_id"
        | "avatar_url"
        | "bio"
        | "verification_reason"
        | "gender"
        | "followers_count"
        | "live_status",
    ) | (
      "tiktok",
      "user_search",
      "secure_user_id" | "avatar_url" | "bio" | "followers_count",
    ) | ("xiaohongshu", "user_search", "avatar_url" | "bio")
  )
}

fn field_support(
  platform: &str,
  key: &str,
) -> (
  AccountFieldAvailability,
  &'static [&'static str],
  Option<&'static str>,
) {
  use AccountFieldAvailability::{Conditional, Enrichment, Unsupported};
  const PROFILE: &[&str] = &["enrich.profile"];
  const POSTS: &[&str] = &["enrich.account_posts"];
  const DEMOGRAPHICS: &[&str] = &["enrich.extended_demographics"];
  const COUNTRY: &[&str] = &["enrich.account_country"];

  match (platform, key) {
    (_, "avatar_url" | "bio" | "followers_count" | "following_count" | "posts_count") => {
      (Enrichment, PROFILE, None)
    }
    (_, "profile_url" | "verification_status") => (Conditional, PROFILE, None),
    (_, "last_posted_at") => (Enrichment, POSTS, None),
    (
      "tiktok" | "douyin",
      "secure_user_id"
      | "verification_reason"
      | "account_type"
      | "private_account"
      | "likes_received_count",
    ) => (Conditional, PROFILE, None),
    (
      "tiktok",
      "website_url"
      | "language"
      | "friends_count"
      | "liked_content_count"
      | "account_created_at"
      | "username_modified_at"
      | "nickname_modified_at"
      | "commerce_status"
      | "commerce_category"
      | "seller_status"
      | "organization_status"
      | "comments_permission"
      | "duet_permission"
      | "stitch_permission"
      | "download_permission"
      | "favorites_visibility"
      | "following_visibility"
      | "playlist_visibility",
    ) => (Conditional, PROFILE, None),
    ("tiktok", "country_region") => (Enrichment, COUNTRY, None),
    ("douyin", "country_region") => (Conditional, PROFILE, None),
    ("douyin", "gender" | "age" | "live_level" | "live_badge") => (Enrichment, DEMOGRAPHICS, None),
    ("tiktok", "live_status" | "live_room_id") => (Conditional, PROFILE, None),
    ("douyin", "live_status" | "live_room_id") => (Enrichment, DEMOGRAPHICS, None),
    ("xiaohongshu", "profile_tags" | "likes_received_count") => (Conditional, PROFILE, None),
    (_, "profile_tags") => (Conditional, PROFILE, None),
    (_, "gender" | "age") => (
      Unsupported,
      &[],
      Some("当前平台账号资料端点未明确提供该字段"),
    ),
    (
      "xiaohongshu",
      "secure_user_id"
      | "website_url"
      | "verification_reason"
      | "account_type"
      | "private_account"
      | "language"
      | "country_region"
      | "friends_count"
      | "liked_content_count"
      | "account_created_at"
      | "live_status"
      | "live_room_id"
      | "username_modified_at"
      | "nickname_modified_at",
    ) => (
      Unsupported,
      &[],
      Some("当前小红书稳定账号端点未明确提供该字段"),
    ),
    (
      "douyin",
      "website_url"
      | "language"
      | "friends_count"
      | "liked_content_count"
      | "account_created_at"
      | "username_modified_at"
      | "nickname_modified_at"
      | "commerce_status"
      | "commerce_category"
      | "seller_status"
      | "organization_status"
      | "comments_permission"
      | "duet_permission"
      | "stitch_permission"
      | "download_permission"
      | "favorites_visibility"
      | "following_visibility"
      | "playlist_visibility",
    ) => (Unsupported, &[], Some("当前抖音账号端点未明确提供该字段")),
    (
      _,
      "commerce_status"
      | "commerce_category"
      | "seller_status"
      | "organization_status"
      | "comments_permission"
      | "duet_permission"
      | "stitch_permission"
      | "download_permission"
      | "favorites_visibility"
      | "following_visibility"
      | "playlist_visibility"
      | "live_level"
      | "live_badge",
    ) => (Unsupported, &[], Some("该字段仅由特定平台公开提供")),
    _ => (Unsupported, &[], Some("当前平台没有可验证的公开字段来源")),
  }
}

fn normalize_platform(platform: &str) -> AppResult<String> {
  match platform.trim() {
    "tiktok" | "douyin" | "xiaohongshu" => Ok(platform.trim().to_string()),
    _ => Err(collection_error("MVP 只支持 TikTok、抖音、小红书")),
  }
}

fn platform_display_name(platform: &str) -> &str {
  match platform {
    "tiktok" => "TikTok",
    "douyin" => "抖音",
    "xiaohongshu" => "小红书",
    _ => platform,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn source_matrix_matches_the_verified_platform_contract() {
    let tiktok = get_account_collection_capabilities("tiktok").unwrap();
    let douyin = get_account_collection_capabilities("douyin").unwrap();
    let xiaohongshu = get_account_collection_capabilities("xiaohongshu").unwrap();

    assert_eq!(tiktok.account_sources.len(), 8);
    assert_eq!(douyin.account_sources.len(), 7);
    assert_eq!(xiaohongshu.account_sources.len(), 5);
    assert!(tiktok
      .account_sources
      .iter()
      .any(|source| source.key == "similar_accounts"));
    assert!(!xiaohongshu
      .account_sources
      .iter()
      .any(|source| source.key == "followers"));
    for capability in [&tiktok, &douyin, &xiaohongshu] {
      for source_key in ["user_search", "content_search_authors", "comment_authors"] {
        let source = capability
          .account_sources
          .iter()
          .find(|source| source.key == source_key)
          .unwrap();
        assert_eq!(source.max_page_size, 20, "{}.{}", capability.platform, source_key);
      }
    }
  }

  #[test]
  fn serialized_capabilities_expose_labels_reasons_and_supported_platforms() {
    let capability = get_account_collection_capabilities("tiktok").unwrap();
    let serialized = serde_json::to_value(capability).unwrap();
    assert_eq!(serialized["account_sources"][0]["label"], "搜索用户");
    let gender = serialized["fields"]
      .as_array()
      .unwrap()
      .iter()
      .find(|field| field["key"] == "gender")
      .unwrap();
    assert_eq!(gender["label"], "性别");
    assert!(gender["missing_reason"].as_str().is_some());
    assert_eq!(gender["supported_platforms"], serde_json::json!(["douyin"]));
    let avatar = serialized["fields"]
      .as_array()
      .unwrap()
      .iter()
      .find(|field| field["key"] == "avatar_url")
      .unwrap();
    assert_eq!(
      avatar["covered_by_source_keys"],
      serde_json::json!(["user_search", "direct_account"])
    );
  }

  #[test]
  fn demographics_are_only_selectable_for_douyin() {
    for platform in ["tiktok", "xiaohongshu"] {
      let capability = get_account_collection_capabilities(platform).unwrap();
      for key in ["gender", "age"] {
        let field = capability
          .fields
          .iter()
          .find(|field| field.key == key)
          .unwrap();
        assert_eq!(field.availability, AccountFieldAvailability::Unsupported);
      }
    }
    let douyin = get_account_collection_capabilities("douyin").unwrap();
    assert!(douyin
      .fields
      .iter()
      .filter(|field| matches!(field.key.as_str(), "gender" | "age"))
      .all(|field| field.availability == AccountFieldAvailability::Enrichment));
  }

  #[test]
  fn defaults_never_include_unsupported_fields_and_keys_are_unique() {
    for platform in ["tiktok", "douyin", "xiaohongshu"] {
      let capability = get_account_collection_capabilities(platform).unwrap();
      let keys = capability
        .fields
        .iter()
        .map(|field| &field.key)
        .collect::<BTreeSet<_>>();
      assert_eq!(keys.len(), capability.fields.len());
      assert!(capability
        .fields
        .iter()
        .filter(|field| field.default_selected)
        .all(|field| field.availability != AccountFieldAvailability::Unsupported));
    }
  }

  #[test]
  fn douyin_demographics_and_live_fields_share_one_enrichment_operation() {
    let capability = get_account_collection_capabilities("douyin").unwrap();
    for key in [
      "gender",
      "age",
      "live_status",
      "live_room_id",
      "live_level",
      "live_badge",
    ] {
      let field = capability
        .fields
        .iter()
        .find(|field| field.key == key)
        .unwrap();
      assert_eq!(
        field.required_operation_keys,
        vec!["enrich.extended_demographics"]
      );
    }
    assert!(capability.fields.iter().all(|field| !field
      .required_operation_keys
      .iter()
      .any(|key| key == "enrich.live_status")));
  }
}
