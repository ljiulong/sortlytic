use serde::Deserialize;
use serde_json::{json, Value};

pub(super) fn collection_plan_schema() -> Value {
  let age_range = age_range_schema();
  let gender_filter = gender_filter_schema();
  let step = step_schema();
  let budget_limit = budget_limit_schema();
  let output_rules = output_rules_schema();
  let cost_estimate = cost_estimate_schema();
  let definitions = definitions_schema();
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "schema_version": { "type": "integer", "const": 4 },
      "entity": { "type": "string", "const": "account" },
      "platforms": {
        "type": "array",
        "minItems": 1,
        "maxItems": 1,
        "items": { "type": "string", "enum": ["tiktok", "douyin", "xiaohongshu"] }
      },
      "account_source": { "$ref": "#/$defs/account_source" },
      "selected_fields": {
        "type": "array",
        "uniqueItems": true,
        "items": { "$ref": "#/$defs/account_field" }
      },
      "enrichment_policy": { "type": "string", "const": "auto_costed" },
      "region": { "type": ["string", "null"] },
      "time_range": { "type": ["string", "null"] },
      "age_range": age_range,
      "gender_filter": gender_filter,
      "steps": {
        "type": "array",
        "minItems": 1,
        "items": step
      },
      "record_limit": { "type": "integer", "minimum": 1 },
      "request_limit": { "type": "integer", "minimum": 1 },
      "budget_limit": budget_limit,
      "output_rules": output_rules,
      "cost_estimate": cost_estimate,
      "missing_fields": { "type": "array", "items": { "type": "string" } },
      "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
      "requires_user_confirmation": { "type": "boolean", "const": true }
    },
    "required": [
      "schema_version", "entity", "platforms", "account_source", "selected_fields",
      "enrichment_policy", "region", "time_range", "age_range", "gender_filter", "steps",
      "record_limit", "request_limit", "budget_limit", "output_rules", "cost_estimate",
      "missing_fields", "confidence", "requires_user_confirmation"
    ],
    "$defs": definitions
  })
}

fn age_range_schema() -> Value {
  json!({
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
  })
}

fn gender_filter_schema() -> Value {
  json!({
    "anyOf": [
      { "type": "null" },
      {
        "type": "array",
        "uniqueItems": true,
        "items": { "type": "string", "enum": ["male", "female", "other"] }
      }
    ]
  })
}

fn step_schema() -> Value {
  let input_binding = input_binding_schema();
  let params = params_schema();
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "step_key": { "type": "string" },
      "operation_key": { "type": "string" },
      "role": { "type": "string", "enum": ["discovery", "enrichment"] },
      "depends_on_step_key": { "type": ["string", "null"] },
      "input_binding": input_binding,
      "endpoint_key": { "type": "string" },
      "platform": { "type": "string", "enum": ["tiktok", "douyin", "xiaohongshu"] },
      "data_type": { "$ref": "#/$defs/data_type" },
      "params": params,
      "request_limit": { "type": "integer", "minimum": 1 },
      "output_selected": { "type": "boolean" }
    },
    "required": [
      "step_key", "operation_key", "role", "depends_on_step_key", "input_binding",
      "endpoint_key", "platform", "data_type", "params", "request_limit", "output_selected"
    ]
  })
}

fn input_binding_schema() -> Value {
  json!({
    "anyOf": [
      { "type": "null" },
      {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "account_id": {
            "type": "string",
            "enum": ["account_handle", "secure_user_id", "platform_user_id"]
          }
        },
        "required": ["account_id"]
      }
    ]
  })
}

fn params_schema() -> Value {
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "keyword": { "type": ["string", "null"] },
      "item_id": { "type": ["string", "null"] },
      "account_id": { "type": ["string", "null"] },
      "region": { "type": ["string", "null"] },
      "time_range": { "type": ["string", "null"] },
      "page_size": {
        "anyOf": [
          { "type": "integer", "minimum": 1 },
          { "type": "null" }
        ]
      }
    },
    "required": ["keyword", "item_id", "account_id", "region", "time_range", "page_size"]
  })
}

fn budget_limit_schema() -> Value {
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "currency": { "type": "string", "const": "USD" },
      "amount_micros": { "type": "integer", "minimum": 1 }
    },
    "required": ["currency", "amount_micros"]
  })
}

fn output_rules_schema() -> Value {
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "entity": { "type": "string", "const": "account" },
      "required_fields": { "type": "array", "items": { "type": "string" } },
      "selected_fields": { "type": "array", "items": { "$ref": "#/$defs/account_field" } },
      "dedupe_key": { "type": "array", "items": { "type": "string" } },
      "fallback_dedupe_key": { "type": "array", "items": { "type": "string" } },
      "unselected_value_label": { "type": "string", "const": "任务未设置" },
      "missing_value_label": { "type": "string", "const": "未采集到" },
      "evidence_required": { "type": "boolean", "const": true }
    },
    "required": [
      "entity", "required_fields", "selected_fields", "dedupe_key", "fallback_dedupe_key",
      "unselected_value_label", "missing_value_label", "evidence_required"
    ]
  })
}

fn cost_estimate_schema() -> Value {
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "request_count_estimate": { "type": "integer", "minimum": 1 },
      "discovery_request_count": { "type": "integer", "minimum": 1 },
      "enrichment_request_count": { "type": "integer", "minimum": 0 },
      "enrichment_operation_count": { "type": "integer", "minimum": 0 },
      "requires_confirmation": { "type": "boolean", "const": true }
    },
    "required": [
      "request_count_estimate", "discovery_request_count", "enrichment_request_count",
      "enrichment_operation_count", "requires_confirmation"
    ]
  })
}

fn definitions_schema() -> Value {
  json!({
    "account_source": {
      "type": "string",
      "enum": [
        "user_search", "content_search_authors", "direct_account", "item_author",
        "comment_authors", "followers", "followings", "similar_accounts"
      ]
    },
    "account_field": {
      "type": "string",
      "enum": [
        "secure_user_id", "avatar_url", "profile_url", "bio", "website_url",
        "verification_status", "verification_reason", "account_type", "private_account",
        "language", "country_region", "profile_tags", "gender", "age", "followers_count",
        "following_count", "friends_count", "posts_count", "likes_received_count",
        "liked_content_count", "account_created_at", "last_posted_at", "live_status",
        "live_room_id", "username_modified_at", "nickname_modified_at", "commerce_status",
        "commerce_category", "seller_status", "organization_status", "comments_permission",
        "duet_permission", "stitch_permission", "download_permission", "favorites_visibility",
        "following_visibility", "playlist_visibility", "live_level", "live_badge"
      ]
    },
    "data_type": {
      "type": "string",
      "enum": [
        "keyword_search", "comments", "account_profile", "account_posts", "item_detail",
        "user_search", "followers", "followings", "similar_accounts",
        "extended_demographics", "account_country"
      ]
    }
  })
}

pub(crate) fn validate_collection_plan_schema(plan: &Value) -> Vec<String> {
  let parsed = match serde_json::from_value::<strict_contract::CollectionPlanV4>(plan.clone()) {
    Ok(parsed) => parsed,
    Err(error) => return vec![format!("模型输出不符合 collection_plan_v4 Schema：{error}")],
  };
  let mut errors = Vec::new();
  if parsed.schema_version != 4 {
    errors.push("collection_plan_v4.schema_version 必须为 4".to_string());
  }
  if parsed.platforms.len() != 1 {
    errors.push("collection_plan_v4.platforms 必须只包含一个平台".to_string());
  }
  if parsed.steps.is_empty() {
    errors.push("collection_plan_v4.steps 至少需要一个步骤".to_string());
  }
  if parsed.record_limit == 0 || parsed.request_limit == 0 {
    errors.push("collection_plan_v4 的记录数与请求数上限必须大于 0".to_string());
  }
  if parsed.budget_limit.amount_micros == 0 {
    errors.push("collection_plan_v4 的预算上限必须大于 0".to_string());
  }
  if !(0.0..=1.0).contains(&parsed.confidence) {
    errors.push("collection_plan_v4.confidence 必须位于 0 到 1 之间".to_string());
  }
  if !parsed.requires_user_confirmation {
    errors.push("collection_plan_v4.requires_user_confirmation 必须为 true".to_string());
  }
  if let Some(age_range) = parsed.age_range.0 {
    if age_range.min > age_range.max || age_range.max > 130 {
      errors.push("collection_plan_v4.age_range 必须是 0 到 130 的有效闭区间".to_string());
    }
  }
  for (index, step) in parsed.steps.iter().enumerate() {
    if step.request_limit == 0 {
      errors.push(format!(
        "collection_plan_v4.steps[{index}].request_limit 必须大于 0"
      ));
    }
    if step.params.page_size.0 == Some(0) {
      errors.push(format!(
        "collection_plan_v4.steps[{index}].params.page_size 必须大于 0"
      ));
    }
  }
  errors
}

#[allow(dead_code)]
mod strict_contract {
  use super::Deserialize;

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct CollectionPlanV4 {
    pub schema_version: i64,
    pub entity: AccountEntity,
    pub platforms: Vec<Platform>,
    pub account_source: AccountSource,
    pub selected_fields: Vec<AccountField>,
    pub enrichment_policy: EnrichmentPolicy,
    pub region: Nullable<String>,
    pub time_range: Nullable<String>,
    pub age_range: Nullable<AgeRange>,
    pub gender_filter: Nullable<Vec<Gender>>,
    pub steps: Vec<Step>,
    pub record_limit: u64,
    pub request_limit: u64,
    pub budget_limit: BudgetLimit,
    pub output_rules: OutputRules,
    pub cost_estimate: CostEstimate,
    pub missing_fields: Vec<String>,
    pub confidence: f64,
    pub requires_user_confirmation: bool,
  }

  #[derive(Deserialize)]
  pub(super) struct Nullable<T>(pub Option<T>);

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum Platform {
    Tiktok,
    Douyin,
    Xiaohongshu,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum AccountSource {
    UserSearch,
    ContentSearchAuthors,
    DirectAccount,
    ItemAuthor,
    CommentAuthors,
    Followers,
    Followings,
    SimilarAccounts,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum AccountField {
    SecureUserId,
    AvatarUrl,
    ProfileUrl,
    Bio,
    WebsiteUrl,
    VerificationStatus,
    VerificationReason,
    AccountType,
    PrivateAccount,
    Language,
    CountryRegion,
    ProfileTags,
    Gender,
    Age,
    FollowersCount,
    FollowingCount,
    FriendsCount,
    PostsCount,
    LikesReceivedCount,
    LikedContentCount,
    AccountCreatedAt,
    LastPostedAt,
    LiveStatus,
    LiveRoomId,
    UsernameModifiedAt,
    NicknameModifiedAt,
    CommerceStatus,
    CommerceCategory,
    SellerStatus,
    OrganizationStatus,
    CommentsPermission,
    DuetPermission,
    StitchPermission,
    DownloadPermission,
    FavoritesVisibility,
    FollowingVisibility,
    PlaylistVisibility,
    LiveLevel,
    LiveBadge,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum DataType {
    KeywordSearch,
    Comments,
    AccountProfile,
    AccountPosts,
    ItemDetail,
    UserSearch,
    Followers,
    Followings,
    SimilarAccounts,
    ExtendedDemographics,
    AccountCountry,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum Gender {
    Male,
    Female,
    Other,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct AgeRange {
    pub min: u64,
    pub max: u64,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct Step {
    pub step_key: String,
    pub operation_key: String,
    pub role: StepRole,
    pub depends_on_step_key: Nullable<String>,
    pub input_binding: Nullable<AccountBinding>,
    pub endpoint_key: String,
    pub platform: Platform,
    pub data_type: DataType,
    pub params: Params,
    pub request_limit: u64,
    pub output_selected: bool,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum StepRole {
    Discovery,
    Enrichment,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct AccountBinding {
    pub account_id: AccountBindingValue,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum AccountBindingValue {
    AccountHandle,
    SecureUserId,
    PlatformUserId,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct Params {
    pub keyword: Nullable<String>,
    pub item_id: Nullable<String>,
    pub account_id: Nullable<String>,
    pub region: Nullable<String>,
    pub time_range: Nullable<String>,
    pub page_size: Nullable<u64>,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct BudgetLimit {
    pub currency: Currency,
    pub amount_micros: u64,
  }

  #[derive(Deserialize)]
  pub(super) enum Currency {
    #[serde(rename = "USD")]
    Usd,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct OutputRules {
    pub entity: AccountEntity,
    pub required_fields: Vec<String>,
    pub selected_fields: Vec<AccountField>,
    pub dedupe_key: Vec<String>,
    pub fallback_dedupe_key: Vec<String>,
    pub unselected_value_label: UnselectedLabel,
    pub missing_value_label: MissingLabel,
    pub evidence_required: bool,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct CostEstimate {
    pub request_count_estimate: u64,
    pub discovery_request_count: u64,
    pub enrichment_request_count: u64,
    pub enrichment_operation_count: u64,
    pub requires_confirmation: bool,
  }

  #[derive(Deserialize)]
  pub(super) enum AccountEntity {
    #[serde(rename = "account")]
    Account,
  }

  #[derive(Deserialize)]
  pub(super) enum EnrichmentPolicy {
    #[serde(rename = "auto_costed")]
    AutoCosted,
  }

  #[derive(Deserialize)]
  pub(super) enum UnselectedLabel {
    #[serde(rename = "任务未设置")]
    Unselected,
  }

  #[derive(Deserialize)]
  pub(super) enum MissingLabel {
    #[serde(rename = "未采集到")]
    Missing,
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeSet;

  use super::*;

  #[test]
  fn every_object_is_closed_and_requires_all_declared_properties() {
    assert_strict_object_schemas(&collection_plan_schema(), "$schema");
  }

  fn assert_strict_object_schemas(schema: &Value, path: &str) {
    let is_object = schema.get("type").is_some_and(|value| {
      value.as_str() == Some("object")
        || value
          .as_array()
          .is_some_and(|types| types.iter().any(|value| value.as_str() == Some("object")))
    });
    if is_object {
      assert_eq!(
        schema.get("additionalProperties").and_then(Value::as_bool),
        Some(false),
        "{path} must reject additional properties"
      );
      let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{path} must define properties"));
      let required = schema
        .get("required")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{path} must require every property"));
      let property_names = properties
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
      let required_names = required
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
      assert_eq!(
        required_names, property_names,
        "{path} required fields must be exhaustive"
      );
    }
    match schema {
      Value::Object(object) => {
        for (key, value) in object {
          assert_strict_object_schemas(value, &format!("{path}/{key}"));
        }
      }
      Value::Array(values) => {
        for (index, value) in values.iter().enumerate() {
          assert_strict_object_schemas(value, &format!("{path}/{index}"));
        }
      }
      _ => {}
    }
  }
}
