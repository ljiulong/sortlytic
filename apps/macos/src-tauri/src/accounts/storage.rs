use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use uuid::Uuid;

use super::{
  normalize_account_with_evidence, normalize_country_region, AccountRecord, AgeRange, SourceKind,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

#[derive(Debug, Clone)]
pub struct AccountObservationInput {
  pub task_run_id: String,
  pub platform: String,
  pub data_type: String,
  pub records: Vec<Value>,
  pub output_selected: bool,
  pub age_range: Option<AgeRange>,
  pub record_limit: usize,
  pub collected_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountPersistenceResult {
  pub observed_count: usize,
  pub skipped_count: usize,
  pub output_count: usize,
}

struct StoredAccount {
  id: String,
  account: AccountRecord,
  output_candidate: bool,
  output_included: bool,
  data_types: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EvidenceFilter<T> {
  Disabled,
  Required(T),
  Invalid,
}

#[derive(Debug, Clone)]
struct AccountFilters {
  gender: Option<BTreeSet<String>>,
  region: EvidenceFilter<String>,
  time_range_days: EvidenceFilter<i64>,
  evaluated_at: Option<DateTime<Utc>>,
}

pub fn persist_account_observations(
  connection: &Connection,
  input: AccountObservationInput,
) -> AppResult<AccountPersistenceResult> {
  if input.task_run_id.trim().is_empty() {
    return Err(validation_error("task_run_id 不能为空"));
  }
  if input.record_limit == 0 {
    return Err(validation_error("账号输出上限必须大于 0"));
  }
  if input.age_range.is_some_and(|range| {
    !(0..=130).contains(&range.min) || range.max > 130 || range.min > range.max
  }) {
    return Err(validation_error("年龄范围必须是 0–130 内的有效闭区间"));
  }
  i64::try_from(input.record_limit).map_err(|_| validation_error("账号输出上限超出范围"))?;
  let source_kind = source_kind(&input.data_type)?;
  let filters = active_account_filters(connection, &input.task_run_id, &input.collected_at)?;
  let transaction = connection.unchecked_transaction().map_err(database_error)?;
  let mut observed_count = 0;
  let mut skipped_count = 0;

  for value in &input.records {
    let endpoint_key = format!("{}.{}", input.platform, input.data_type);
    let incoming = match normalize_account_with_evidence(
      &input.platform,
      source_kind,
      &endpoint_key,
      &input.collected_at,
      value,
    ) {
      Ok(account) => account,
      Err(error) if error.message.contains("缺少平台用户 ID") => {
        skipped_count += 1;
        continue;
      }
      Err(error) => return Err(error),
    };
    persist_account(&transaction, &input, incoming, &filters)?;
    observed_count += 1;
  }

  let result = AccountPersistenceResult {
    observed_count,
    skipped_count,
    output_count: output_count(&transaction, &input.task_run_id)?,
  };
  transaction.commit().map_err(database_error)?;
  Ok(result)
}

fn source_kind(data_type: &str) -> AppResult<SourceKind> {
  match data_type {
    "comments" => Ok(SourceKind::CommentAuthor),
    "account_profile" => Ok(SourceKind::AccountProfile),
    "keyword_search" | "item_detail" => Ok(SourceKind::ContentAuthor),
    "user_search" => Ok(SourceKind::UserSearch),
    "followers" | "followings" | "similar_accounts" => Ok(SourceKind::Relationship),
    "account_posts" | "extended_demographics" | "account_country" => {
      Ok(SourceKind::FieldEnrichment)
    }
    _ => Err(validation_error("账号观测数据类型不受支持")),
  }
}

fn persist_account(
  connection: &Connection,
  input: &AccountObservationInput,
  incoming: AccountRecord,
  filters: &AccountFilters,
) -> AppResult<()> {
  let matches = matching_accounts(connection, input, &incoming)?;
  let was_included = matches.iter().any(|stored| stored.output_included);
  let mut output_candidate = input.output_selected;
  let mut data_types = BTreeSet::from([input.data_type.clone()]);
  let survivor_id = matches
    .iter()
    .find(|stored| stored.account.identity_key == incoming.identity_key)
    .or_else(|| matches.first())
    .map(|stored| stored.id.clone())
    .unwrap_or_else(|| Uuid::new_v4().to_string());
  let mut merged = matches
    .first()
    .map(|stored| stored.account.clone())
    .unwrap_or_else(|| incoming.clone());
  let initial_id = matches.first().map(|stored| stored.id.clone());

  for stored in matches {
    output_candidate |= stored.output_candidate;
    data_types.extend(stored.data_types);
    if initial_id.as_deref() != Some(stored.id.as_str()) {
      merged.merge(stored.account.clone());
    }
    if stored.id != survivor_id {
      connection
        .execute(
          "DELETE FROM collected_account WHERE id = ?1",
          params![stored.id],
        )
        .map_err(database_error)?;
    }
  }
  merged.merge(incoming.clone());
  if incoming.identity_key.starts_with("id:") {
    merged.identity_key = incoming.identity_key;
  }

  let qualifies = input
    .age_range
    .is_none_or(|range| range.includes(merged.age))
    && filters.gender.as_ref().is_none_or(|filter| {
      merged
        .gender
        .as_ref()
        .is_some_and(|gender| filter.contains(gender))
    })
    && region_matches(&filters.region, merged.country_region.as_deref())
    && time_range_matches(
      &filters.time_range_days,
      filters.evaluated_at,
      merged.last_posted_at.as_deref(),
    );
  let other_output_count = connection
    .query_row(
      "SELECT COUNT(*) FROM collected_account
       WHERE task_run_id = ?1 AND output_included = 1 AND id <> ?2",
      params![input.task_run_id, survivor_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  let output_included = output_candidate
    && qualifies
    && (was_included || other_output_count < input.record_limit as i64);
  let now = Utc::now().to_rfc3339();
  let priority_json = serde_json::json!({
    "fields": merged.field_priorities(),
    "output_candidate": output_candidate,
    "data_types": data_types
  });
  let merged_json = serde_json::to_string(&merged).map_err(json_error)?;
  let account_fields_json = serde_json::to_string(&merged.account_fields).map_err(json_error)?;
  let field_evidence_json = serde_json::to_string(&merged.field_evidence).map_err(json_error)?;
  let endpoint_sources = data_types
    .iter()
    .map(|data_type| format!("{}.{}", merged.platform, data_type))
    .collect::<BTreeSet<_>>();
  let data_source = if endpoint_sources.is_empty() {
    format!(
      "TikHub API ({})",
      data_types.into_iter().collect::<Vec<_>>().join(", ")
    )
  } else {
    endpoint_sources.into_iter().collect::<Vec<_>>().join(", ")
  };
  let region_source = merged.country_region.as_ref().map(|_| "TikHub API");
  let region_confidence = merged.country_region.as_ref().map(|_| "高");

  connection
    .execute(
      "INSERT INTO collected_account (
         id, task_run_id, platform, identity_key, username, account, platform_user_id,
         profile_text, country_region, region_source, region_confidence, gender, age,
         followers_count, posts_count, last_posted_at, profile_url, data_source,
         collected_at, notes, merged_record_json, source_priority_json, account_fields_json,
         field_evidence_json, output_included, created_at, updated_at
       ) VALUES (
         ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
         ?14, ?15, ?16, ?17, ?18, ?19, NULL, ?20, ?21, ?22, ?23, ?24, ?25, ?25
       )
       ON CONFLICT(id) DO UPDATE SET
         identity_key = excluded.identity_key, username = excluded.username,
         account = excluded.account, platform_user_id = excluded.platform_user_id,
         profile_text = excluded.profile_text, country_region = excluded.country_region,
         region_source = excluded.region_source, region_confidence = excluded.region_confidence,
         gender = excluded.gender, age = excluded.age,
         followers_count = excluded.followers_count, posts_count = excluded.posts_count,
         last_posted_at = excluded.last_posted_at, profile_url = excluded.profile_url,
         data_source = excluded.data_source, collected_at = excluded.collected_at,
         merged_record_json = excluded.merged_record_json,
         source_priority_json = excluded.source_priority_json,
         account_fields_json = excluded.account_fields_json,
         field_evidence_json = excluded.field_evidence_json,
         output_included = excluded.output_included, updated_at = excluded.updated_at",
      params![
        survivor_id,
        input.task_run_id,
        merged.platform,
        merged.identity_key,
        merged.username,
        merged.account,
        merged.platform_user_id,
        merged.profile_text,
        merged.country_region,
        region_source,
        region_confidence,
        merged.gender,
        merged.age,
        merged.followers_count,
        merged.posts_count,
        merged.last_posted_at,
        merged.profile_url,
        data_source,
        input.collected_at,
        merged_json,
        priority_json.to_string(),
        account_fields_json,
        field_evidence_json,
        i64::from(output_included),
        now
      ],
    )
    .map_err(database_error)?;
  Ok(())
}

fn matching_accounts(
  connection: &Connection,
  input: &AccountObservationInput,
  incoming: &AccountRecord,
) -> AppResult<Vec<StoredAccount>> {
  let mut statement = connection
    .prepare(
      "SELECT id, merged_record_json, source_priority_json, output_included
       FROM collected_account WHERE task_run_id = ?1 AND platform = ?2 ORDER BY created_at, id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![input.task_run_id, input.platform], |row| {
      Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, i64>(3)? != 0,
      ))
    })
    .map_err(database_error)?;
  let incoming_account = incoming.normalized_account();
  let mut matches = Vec::new();
  for row in rows {
    let (id, merged_json, priority_json, output_included) = row.map_err(database_error)?;
    let mut account: AccountRecord = serde_json::from_str(&merged_json).map_err(json_error)?;
    let priority: Value = serde_json::from_str(&priority_json).map_err(json_error)?;
    account.restore_field_priorities(priority_fields(&priority));
    let same_identity = account.identity_key == incoming.identity_key;
    let same_fallback = account.normalized_account() == incoming_account
      && incoming_account.is_some()
      && (account.identity_key.starts_with("account:")
        || incoming.identity_key.starts_with("account:"));
    if same_identity || same_fallback {
      matches.push(StoredAccount {
        id,
        account,
        output_candidate: priority
          .get("output_candidate")
          .and_then(Value::as_bool)
          .unwrap_or(output_included),
        output_included,
        data_types: priority
          .get("data_types")
          .and_then(Value::as_array)
          .into_iter()
          .flatten()
          .filter_map(Value::as_str)
          .map(ToString::to_string)
          .collect(),
      });
    }
  }
  Ok(matches)
}

fn priority_fields(value: &Value) -> BTreeMap<String, u8> {
  value
    .get("fields")
    .and_then(Value::as_object)
    .into_iter()
    .flatten()
    .filter_map(|(key, value)| {
      value
        .as_u64()
        .and_then(|value| u8::try_from(value).ok())
        .map(|value| (key.clone(), value))
    })
    .collect()
}

fn output_count(connection: &Connection, task_run_id: &str) -> AppResult<usize> {
  let count = connection
    .query_row(
      "SELECT COUNT(*) FROM collected_account
       WHERE task_run_id = ?1 AND output_included = 1",
      params![task_run_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  usize::try_from(count).map_err(|_| database_error("账号输出数量超出范围"))
}

fn active_account_filters(
  connection: &Connection,
  task_run_id: &str,
  collected_at: &str,
) -> AppResult<AccountFilters> {
  let plan_json = connection
    .query_row(
      "SELECT plan.plan_json
       FROM task_run AS run
       JOIN collection_plan AS plan ON plan.id = run.plan_id
       WHERE run.id = ?1",
      params![task_run_id],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?;
  let Some(plan_json) = plan_json else {
    return Ok(AccountFilters {
      gender: None,
      region: EvidenceFilter::Disabled,
      time_range_days: EvidenceFilter::Disabled,
      evaluated_at: timestamp_utc(collected_at),
    });
  };
  let plan_json: Value = serde_json::from_str(&plan_json).map_err(json_error)?;
  let genders = plan_json
    .get("gender_filter")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .map(ToString::to_string)
    .collect::<BTreeSet<_>>();
  let evidence_filters_enabled = plan_json.get("schema_version").and_then(Value::as_i64) == Some(4);
  Ok(AccountFilters {
    gender: (!genders.is_empty()).then_some(genders),
    region: evidence_filters_enabled
      .then(|| plan_region_filter(&plan_json))
      .unwrap_or(EvidenceFilter::Disabled),
    time_range_days: evidence_filters_enabled
      .then(|| plan_time_range_filter(&plan_json))
      .unwrap_or(EvidenceFilter::Disabled),
    evaluated_at: timestamp_utc(collected_at),
  })
}

fn plan_region_filter(plan_json: &Value) -> EvidenceFilter<String> {
  let Some(value) = plan_json.get("region").filter(|value| !value.is_null()) else {
    return EvidenceFilter::Disabled;
  };
  let raw = value
    .as_str()
    .or_else(|| value.get("value").and_then(Value::as_str));
  normalize_country_region(raw).map_or(EvidenceFilter::Invalid, EvidenceFilter::Required)
}

fn plan_time_range_filter(plan_json: &Value) -> EvidenceFilter<i64> {
  let Some(value) = plan_json.get("time_range").filter(|value| !value.is_null()) else {
    return EvidenceFilter::Disabled;
  };
  let days = value
    .as_i64()
    .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()));
  days
    .filter(|days| matches!(days, 1 | 7 | 30 | 180))
    .map_or(EvidenceFilter::Invalid, EvidenceFilter::Required)
}

fn region_matches(filter: &EvidenceFilter<String>, actual: Option<&str>) -> bool {
  match filter {
    EvidenceFilter::Disabled => true,
    EvidenceFilter::Required(expected) => {
      normalize_country_region(actual).as_ref() == Some(expected)
    }
    EvidenceFilter::Invalid => false,
  }
}

fn time_range_matches(
  filter: &EvidenceFilter<i64>,
  evaluated_at: Option<DateTime<Utc>>,
  actual: Option<&str>,
) -> bool {
  match filter {
    EvidenceFilter::Disabled => true,
    EvidenceFilter::Required(days) => evaluated_at
      .zip(actual.and_then(timestamp_utc))
      .is_some_and(|(evaluated_at, posted_at)| {
        posted_at <= evaluated_at && posted_at >= evaluated_at - Duration::days(*days)
      }),
    EvidenceFilter::Invalid => false,
  }
}

fn timestamp_utc(value: &str) -> Option<DateTime<Utc>> {
  let value = value.trim();
  DateTime::parse_from_rfc3339(value)
    .ok()
    .map(|value| value.with_timezone(&Utc))
    .or_else(|| {
      let raw = value.parse::<i64>().ok()?;
      let (seconds, nanos) = if raw.unsigned_abs() >= 1_000_000_000_000 {
        (
          raw.div_euclid(1_000),
          raw.rem_euclid(1_000) as u32 * 1_000_000,
        )
      } else {
        (raw, 0)
      };
      DateTime::from_timestamp(seconds, nanos)
    })
}

fn validation_error(message: impl Into<String>) -> AppError {
  AppError::validation(message, AppErrorStage::Collection)
}

fn database_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    error.to_string(),
    AppErrorStage::Database,
    false,
  )
}

fn json_error(error: impl ToString) -> AppError {
  AppError::validation(error.to_string(), AppErrorStage::Collection)
}
