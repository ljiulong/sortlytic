use serde_json::Value;

pub(crate) fn generate_plan_json(intent_text: &str) -> Value {
  let lower = intent_text.to_ascii_lowercase();
  let mut platforms = Vec::new();
  let mut data_types = Vec::new();
  let mut missing_fields = Vec::new();

  if lower.contains("tiktok") {
    platforms.push("tiktok");
  }
  if intent_text.contains("抖音") || lower.contains("douyin") {
    platforms.push("douyin");
  }
  if intent_text.contains("小红书") || lower.contains("xiaohongshu") {
    platforms.push("xiaohongshu");
  }
  if intent_text.contains("评论") || lower.contains("comment") {
    data_types.push("comments");
  }
  if intent_text.contains("关键词") || lower.contains("keyword") || lower.contains("search") {
    data_types.push("keyword_search");
  }

  let should_infer_cn = intent_text.contains("中国")
    || lower.contains("china")
    || lower.contains(" cn")
    || platforms
      .iter()
      .any(|platform| matches!(*platform, "douyin" | "xiaohongshu"));
  let region = if intent_text.contains("美国") || lower.contains(" us") || lower.contains("usa") {
    Some("US")
  } else if should_infer_cn {
    Some("CN")
  } else {
    None
  };

  if platforms.is_empty() {
    missing_fields.push("platforms");
  }
  if data_types.is_empty() {
    missing_fields.push("data_types");
  }
  if region.is_none() {
    missing_fields.push("region");
  }

  let keywords = extract_keywords(intent_text);
  let steps = platforms
    .iter()
    .flat_map(|platform| {
      let keywords = keywords.clone();
      data_types.iter().map(move |data_type| {
        let mut params = serde_json::Map::new();
        if let Some(region) = region {
          params.insert("region".to_string(), Value::String(region.to_string()));
        }
        if *data_type == "keyword_search" {
          if let Some(keyword) = keywords
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_str)
          {
            params.insert("keyword".to_string(), Value::String(keyword.to_string()));
          }
        }
        serde_json::json!({
          "endpoint_key": format!("{platform}.{data_type}"),
          "platform": platform,
          "data_type": data_type,
          "params": params
        })
      })
    })
    .collect::<Vec<_>>();
  let request_count_estimate = platforms.len().max(1) * data_types.len().max(1);

  serde_json::json!({
    "platforms": platforms,
    "data_types": data_types,
    "region": region.map(|value| serde_json::json!({
      "value": value,
      "source": "natural_language",
      "confidence": 0.8,
      "validation_status": "unverified"
    })).unwrap_or(Value::Null),
    "keywords": keywords,
    "accounts": [],
    "time_range": Value::Null,
    "steps": steps,
    "request_limit": request_count_estimate,
    "cost_estimate": {
      "request_count_estimate": request_count_estimate,
      "requires_confirmation": true
    },
    "missing_fields": missing_fields,
    "confidence": 0.65,
    "requires_user_confirmation": true
  })
}

fn extract_keywords(intent_text: &str) -> Value {
  for marker in ["关键词", "keyword"] {
    if let Some(index) = intent_text.find(marker) {
      let keyword = intent_text[index + marker.len()..]
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(['：', ':', '，', ',']);
      if !keyword.is_empty() {
        return serde_json::json!([keyword]);
      }
    }
  }

  Value::Array(Vec::new())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn builds_step_for_each_platform_and_data_type_pair() {
    let plan = generate_plan_json("同时采集美国 TikTok 和抖音的评论与关键词");

    assert_eq!(plan["platforms"], serde_json::json!(["tiktok", "douyin"]));
    assert_eq!(
      plan["data_types"],
      serde_json::json!(["comments", "keyword_search"])
    );
    assert_eq!(plan["steps"].as_array().map(Vec::len), Some(4));
  }
}
