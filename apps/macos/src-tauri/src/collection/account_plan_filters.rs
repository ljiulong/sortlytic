use std::collections::BTreeSet;

use serde_json::Value;

use super::{
  AccountCollectionCapabilityView, AccountSourceCapabilityView, AccountSourceInputKind,
  FilterExecution,
};
use crate::accounts::normalize_country_region;
use crate::ai::collection_intent_schema::{primary_query_locale, valid_query_locale};
use crate::domain::{AppError, AppErrorStage, AppResult};

pub(super) fn validate_plan_filters(
  plan_json: &Value,
  selected_fields: Option<&[String]>,
  capability: Option<&AccountCollectionCapabilityView>,
  account_source: Option<&str>,
  errors: &mut Vec<String>,
) {
  let source = capability.and_then(|capability| {
    account_source.and_then(|account_source| {
      capability
        .account_sources
        .iter()
        .find(|source| source.key == account_source)
    })
  });
  validate_plan_query_locale(plan_json, source.map(|source| source.input_kind), errors);
  match parse_region_filter(plan_json.get("region")) {
    Ok(Some(region)) => {
      if source.is_some_and(|source| source.region_filter == FilterExecution::Unsupported) {
        errors.push("当前平台或账号来源无法可靠筛选地区，请移除地区条件或更换来源".to_string());
      }
      if selected_fields.is_none_or(|fields| !fields.iter().any(|field| field == "country_region"))
      {
        errors.push("启用 region 时必须选择 country_region 证据字段".to_string());
      }
      if source.is_some_and(|source| source.region_filter == FilterExecution::Provider)
        && discovery_param(plan_json, "region") != Some(region.as_str())
      {
        errors.push("发现步骤 params.region 必须与顶层 region 一致".to_string());
      }
    }
    Ok(None) => {}
    Err(error) => errors.push(error.to_string()),
  }
  match parse_time_range_filter(plan_json.get("time_range")) {
    Ok(Some(time_range)) => {
      if let Some(source) = source {
        if source.time_range_filter == FilterExecution::Unsupported {
          errors.push("当前平台或账号来源无法可靠筛选时间，请移除时间条件或更换来源".to_string());
        } else if !source.time_ranges.contains(&time_range) {
          errors.push(format!("当前账号来源不支持 {time_range} 天时间范围"));
        }
        if source.time_range_filter == FilterExecution::Provider
          && discovery_param(plan_json, "time_range") != Some(time_range.as_str())
        {
          errors.push("发现步骤 params.time_range 必须与顶层 time_range 一致".to_string());
        }
      }
      if selected_fields.is_none_or(|fields| !fields.iter().any(|field| field == "last_posted_at"))
      {
        errors.push("启用 time_range 时必须选择 last_posted_at 证据字段".to_string());
      }
    }
    Ok(None) => {}
    Err(error) => errors.push(error.to_string()),
  }
  if let Some(age_range) = plan_json.get("age_range").filter(|value| !value.is_null()) {
    let bounds = age_range
      .get("min")
      .and_then(Value::as_i64)
      .zip(age_range.get("max").and_then(Value::as_i64));
    if !bounds.is_some_and(|(min, max)| (0..=130).contains(&min) && min <= max && max <= 130) {
      errors.push("age_range 必须是 0–130 内且 min <= max 的整数闭区间".to_string());
    }
    if selected_fields.is_none_or(|fields| !fields.iter().any(|field| field == "age")) {
      errors.push("启用 age_range 时必须选择 age 字段".to_string());
    }
  }
  if let Some(filter) = plan_json
    .get("gender_filter")
    .filter(|value| !value.is_null())
  {
    let mut seen = BTreeSet::new();
    let valid = filter.as_array().is_some_and(|values| {
      !values.is_empty()
        && values.iter().all(|value| {
          value
            .as_str()
            .is_some_and(|value| matches!(value, "male" | "female" | "other") && seen.insert(value))
        })
    });
    if !valid {
      errors.push("gender_filter 只能包含不重复的 male、female、other".to_string());
    }
    if selected_fields.is_none_or(|fields| !fields.iter().any(|field| field == "gender")) {
      errors.push("启用 gender_filter 时必须选择 gender 字段".to_string());
    }
  }
}

fn validate_plan_query_locale(
  plan_json: &Value,
  source_input_kind: Option<AccountSourceInputKind>,
  errors: &mut Vec<String>,
) {
  let Some(query_locale) = plan_json
    .get("query_locale")
    .filter(|value| !value.is_null())
  else {
    return;
  };
  if source_input_kind.is_some_and(|kind| kind != AccountSourceInputKind::Keyword) {
    errors.push("直接账号、作品或关系列表来源不得设置 query_locale 或翻译标识".to_string());
    return;
  }
  let Some(query_locale) = query_locale
    .as_str()
    .filter(|value| valid_query_locale(value))
  else {
    errors.push("query_locale 必须使用 language-REGION 格式，例如 en-GB".to_string());
    return;
  };
  let Some(region) = plan_json
    .get("region")
    .filter(|value| !value.is_null())
    .and_then(Value::as_str)
  else {
    errors.push("设置 query_locale 前必须提供明确地区".to_string());
    return;
  };
  if let Some(expected) = primary_query_locale(region) {
    if query_locale != expected {
      errors.push(format!("目标地区 {region} 的主检索语言必须为 {expected}"));
    }
  } else if !query_locale.ends_with(region) {
    errors.push(format!(
      "query_locale {query_locale} 必须与目标地区 {region} 一致"
    ));
  }
}

pub(super) fn validate_plan_limits(plan_json: &Value, errors: &mut Vec<String>) {
  for field in ["record_limit", "request_limit"] {
    if plan_json
      .get(field)
      .and_then(Value::as_i64)
      .is_none_or(|value| value <= 0)
    {
      errors.push(format!("{field} 必须是大于 0 的整数"));
    }
  }
  let valid_budget = plan_json
    .get("budget_limit")
    .and_then(Value::as_object)
    .is_some_and(|budget| {
      budget.get("currency").and_then(Value::as_str) == Some("USD")
        && budget
          .get("amount_micros")
          .and_then(Value::as_i64)
          .is_some_and(|value| value > 0)
    });
  if !valid_budget {
    errors.push("budget_limit 必须包含 USD 正整数微美元上限".to_string());
  }
  if plan_json
    .get("cost_estimate")
    .and_then(Value::as_object)
    .is_none()
  {
    errors.push("cost_estimate 必须是对象".to_string());
  }
}

pub(super) fn parse_region_filter(value: Option<&Value>) -> Result<Option<String>, &'static str> {
  let Some(value) = value.filter(|value| !value.is_null()) else {
    return Ok(None);
  };
  let Some(region) = value.as_str() else {
    return Err("region 必须是大写 ISO 两位代码或 null");
  };
  let region = region.trim();
  if normalize_country_region(Some(region)).as_deref() != Some(region) {
    return Err("region 必须是大写 ISO 两位代码或 null");
  }
  Ok(Some(region.to_string()))
}

pub(super) fn parse_time_range_filter(
  value: Option<&Value>,
) -> Result<Option<String>, &'static str> {
  let Some(value) = value.filter(|value| !value.is_null()) else {
    return Ok(None);
  };
  let days = value
    .as_i64()
    .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()));
  match days {
    Some(days @ (1 | 7 | 30 | 180)) => Ok(Some(days.to_string())),
    _ => Err("time_range 只能是 1、7、30 或 180 天或 null"),
  }
}

pub(super) fn validate_source_filter_support(
  source: &AccountSourceCapabilityView,
  region: Option<&str>,
  time_range: Option<&str>,
) -> AppResult<()> {
  if region.is_some() && source.region_filter == FilterExecution::Unsupported {
    return Err(validation_error(
      "当前平台或账号来源无法可靠筛选地区，请移除地区条件或更换来源",
    ));
  }
  if let Some(time_range) = time_range {
    if source.time_range_filter == FilterExecution::Unsupported {
      return Err(validation_error(
        "当前平台或账号来源无法可靠筛选时间，请移除时间条件或更换来源",
      ));
    }
    if !source.time_ranges.iter().any(|value| value == time_range) {
      return Err(validation_error(format!(
        "当前账号来源不支持 {time_range} 天时间范围"
      )));
    }
  }
  Ok(())
}

fn discovery_param<'a>(plan_json: &'a Value, key: &str) -> Option<&'a str> {
  plan_json
    .get("steps")
    .and_then(Value::as_array)
    .and_then(|steps| {
      steps
        .iter()
        .find(|step| step.get("role").and_then(Value::as_str) == Some("discovery"))
    })
    .and_then(|step| step.get("params"))
    .and_then(|params| params.get(key))
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty())
}

fn validation_error(message: impl Into<String>) -> AppError {
  AppError::validation(message, AppErrorStage::Collection)
}
