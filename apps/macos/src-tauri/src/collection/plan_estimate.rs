use std::collections::BTreeMap;

use serde_json::Value;

use super::{endpoint_for, PaginationMode};

pub(crate) fn estimate_request_count(plan_json: &Value) -> Option<i64> {
  let steps = plan_json.get("steps")?.as_array()?;
  if steps.is_empty() {
    return None;
  }
  let default_request_limit = plan_json
    .get("request_limit")
    .and_then(Value::as_i64)
    .filter(|value| *value > 0)
    .unwrap_or(1);
  let record_limit = plan_json
    .get("record_limit")
    .and_then(Value::as_i64)
    .filter(|value| *value > 0);
  let mut output_limits = BTreeMap::new();
  let mut total = 0_i64;

  for (index, step) in steps.iter().enumerate() {
    let step = step.as_object()?;
    let platform = step.get("platform")?.as_str()?;
    let data_type = step.get("data_type")?.as_str()?;
    let endpoint = endpoint_for(platform, data_type).ok()?;
    let request_limit = step
      .get("request_limit")
      .and_then(Value::as_i64)
      .filter(|value| *value > 0)
      .unwrap_or(default_request_limit);
    let dependency = step
      .get("depends_on_step_key")
      .and_then(Value::as_str)
      .map(str::trim)
      .filter(|value| !value.is_empty());
    let request_count = dependency.map_or(Some(request_limit), |dependency| {
      output_limits
        .get(dependency)
        .map(|target_count: &i64| target_count.saturating_mul(request_limit))
    })?;
    total = total.saturating_add(request_count);

    let step_key = step
      .get("step_key")
      .and_then(Value::as_str)
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .map(ToString::to_string)
      .unwrap_or_else(|| format!("step-{index}"));
    let output_limit = match endpoint.pagination_mode {
      PaginationMode::Single => request_count,
      PaginationMode::Cursor => request_count.saturating_mul(endpoint.max_page_size),
    };
    let output_limit = record_limit.map_or(output_limit, |limit| output_limit.min(limit));
    if output_limits.insert(step_key, output_limit).is_some() {
      return None;
    }
  }

  Some(total.max(1))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn counts_dependency_targets_from_the_parent_page_capacity() {
    let plan = serde_json::json!({
      "request_limit": 20,
      "steps": [
        {
          "step_key": "keyword_search",
          "platform": "xiaohongshu",
          "data_type": "keyword_search",
          "request_limit": 20
        },
        {
          "step_key": "item_detail",
          "depends_on_step_key": "keyword_search",
          "platform": "xiaohongshu",
          "data_type": "item_detail",
          "request_limit": 1
        },
        {
          "step_key": "account_profile",
          "depends_on_step_key": "keyword_search",
          "platform": "xiaohongshu",
          "data_type": "account_profile",
          "request_limit": 1
        },
        {
          "step_key": "comments",
          "depends_on_step_key": "keyword_search",
          "platform": "xiaohongshu",
          "data_type": "comments",
          "request_limit": 20
        }
      ]
    });

    assert_eq!(estimate_request_count(&plan), Some(22_020));
  }

  #[test]
  fn falls_back_to_the_top_level_limit_for_legacy_steps() {
    let plan = serde_json::json!({
      "request_limit": 5,
      "steps": [{
        "platform": "tiktok",
        "data_type": "keyword_search"
      }]
    });

    assert_eq!(estimate_request_count(&plan), Some(5));
  }

  #[test]
  fn caps_v4_dependency_fanout_at_the_record_limit() {
    let plan = serde_json::json!({
      "record_limit": 10,
      "request_limit": 1,
      "steps": [
        {
          "step_key": "discover",
          "platform": "tiktok",
          "data_type": "user_search",
          "request_limit": 1
        },
        {
          "step_key": "enrich_country",
          "depends_on_step_key": "discover",
          "platform": "tiktok",
          "data_type": "account_country",
          "request_limit": 1
        }
      ]
    });

    assert_eq!(estimate_request_count(&plan), Some(11));
  }
}
