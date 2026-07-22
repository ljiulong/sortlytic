use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::accounts::normalize_country_region;
use crate::collection::{account_field_keys, account_source_keys};

const REQUIRED_FIELDS: &[&str] = &[
  "schema_version",
  "platform",
  "account_source",
  "source_input",
  "query_locale",
  "region_code",
  "selected_fields",
  "time_range_days",
  "age_range",
  "gender_filter",
  "record_limit",
  "budget_limit_micros",
  "missing_fields",
  "confidence",
];
const BUSINESS_FIELDS: &[&str] = &[
  "platform",
  "account_source",
  "source_input",
  "query_locale",
  "region_code",
  "selected_fields",
  "time_range_days",
  "age_range",
  "gender_filter",
  "record_limit",
  "budget_limit_micros",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntentAgeRange {
  pub min: i64,
  pub max: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CollectionIntentV1 {
  pub schema_version: i64,
  pub platform: Option<String>,
  pub account_source: Option<String>,
  pub source_input: Option<String>,
  pub query_locale: Option<String>,
  pub region_code: Option<String>,
  pub selected_fields: Vec<String>,
  pub time_range_days: Option<i64>,
  pub age_range: Option<IntentAgeRange>,
  pub gender_filter: Option<Vec<String>>,
  pub record_limit: Option<i64>,
  pub budget_limit_micros: Option<i64>,
  pub missing_fields: Vec<String>,
  pub confidence: f64,
}

pub(crate) fn collection_intent_schema() -> Value {
  let account_sources = account_source_keys();
  let account_fields = account_field_keys();
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "schema_version": { "type": "integer", "const": 1 },
      "platform": nullable_enum(&["tiktok", "douyin", "xiaohongshu"]),
      "account_source": nullable_owned_enum(&account_sources),
      "source_input": nullable_string(),
      "query_locale": nullable_string(),
      "region_code": nullable_string(),
      "selected_fields": {
        "type": "array",
        "uniqueItems": true,
        "items": { "type": "string", "enum": account_fields }
      },
      "time_range_days": {
        "anyOf": [
          { "type": "null" },
          { "type": "integer", "enum": [1, 7, 30, 180] }
        ]
      },
      "age_range": {
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
      },
      "gender_filter": {
        "anyOf": [
          { "type": "null" },
          {
            "type": "array",
            "uniqueItems": true,
            "items": { "type": "string", "enum": ["male", "female", "other"] }
          }
        ]
      },
      "record_limit": nullable_positive_integer(),
      "budget_limit_micros": nullable_positive_integer(),
      "missing_fields": {
        "type": "array",
        "uniqueItems": true,
        "items": { "type": "string", "enum": BUSINESS_FIELDS }
      },
      "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
    },
    "required": REQUIRED_FIELDS
  })
}

pub(crate) fn parse_collection_intent(value: &Value) -> Result<CollectionIntentV1, Vec<String>> {
  let Some(object) = value.as_object() else {
    return Err(vec![
      "模型输出不符合 collection_intent_v1 Schema：顶层必须是对象".to_string(),
    ]);
  };
  let mut errors = Vec::new();
  for field in REQUIRED_FIELDS {
    if !object.contains_key(*field) {
      errors.push(format!(
        "collection_intent_v1 缺少必需字段 {field}；业务缺失时必须显式写 null"
      ));
    }
  }
  for field in object.keys() {
    if !REQUIRED_FIELDS.contains(&field.as_str()) {
      errors.push(format!(
        "collection_intent_v1 不允许字段 {field}；端点、步骤和成本必须由后端生成"
      ));
    }
  }
  if !errors.is_empty() {
    return Err(errors);
  }
  let parsed = serde_json::from_value::<CollectionIntentV1>(value.clone()).map_err(|error| {
    vec![format!(
      "模型输出不符合 collection_intent_v1 Schema：{error}"
    )]
  })?;
  validate_intent_values(&parsed)?;
  Ok(parsed)
}

fn validate_intent_values(intent: &CollectionIntentV1) -> Result<(), Vec<String>> {
  let mut errors = Vec::new();
  if intent.schema_version != 1 {
    errors.push("collection_intent_v1.schema_version 必须为 1".to_string());
  }
  if intent
    .platform
    .as_deref()
    .is_some_and(|value| !matches!(value, "tiktok" | "douyin" | "xiaohongshu"))
  {
    errors.push("collection_intent_v1.platform 不是受支持的平台".to_string());
  }
  let account_sources = account_source_keys();
  if intent
    .account_source
    .as_ref()
    .is_some_and(|value| !account_sources.contains(value))
  {
    errors.push("collection_intent_v1.account_source 不是受支持的账号来源".to_string());
  }
  if intent
    .source_input
    .as_deref()
    .is_some_and(|value| value.trim().is_empty())
  {
    errors.push("collection_intent_v1.source_input 不能是空字符串".to_string());
  }
  if let Some(region) = intent.region_code.as_deref() {
    if normalize_country_region(Some(region)).as_deref() != Some(region) {
      errors.push("collection_intent_v1.region_code 必须是大写 ISO 两位代码".to_string());
    }
  }
  if intent
    .query_locale
    .as_deref()
    .is_some_and(|value| !valid_query_locale(value))
  {
    errors.push(
      "collection_intent_v1.query_locale 必须使用 language-REGION 格式，例如 en-GB".to_string(),
    );
  }
  let keyword_search = intent
    .account_source
    .as_deref()
    .is_some_and(|source| matches!(source, "user_search" | "content_search_authors"));
  if keyword_search {
    if let (Some(region), Some(query_locale)) = (
      intent.region_code.as_deref(),
      intent.query_locale.as_deref(),
    ) {
      if let Some(expected_locale) = primary_query_locale(region) {
        if query_locale != expected_locale {
          errors.push(format!(
            "目标地区 {region} 的主检索语言必须为 {expected_locale}"
          ));
        }
        if intent
          .source_input
          .as_deref()
          .is_some_and(|value| !query_matches_locale_script(expected_locale, value))
          && !intent
            .missing_fields
            .iter()
            .any(|field| field == "source_input")
        {
          errors.push(if expected_locale.starts_with("en-") {
            format!(
              "目标地区 {region} 必须使用英文实际检索词；专有名词不确定时请把 source_input 加入 missing_fields"
            )
          } else {
            format!(
              "目标地区 {region} 的实际检索词必须使用 {expected_locale} 对应文字脚本；专有名词不确定时请把 source_input 加入 missing_fields"
            )
          });
        }
      } else if !intent
        .missing_fields
        .iter()
        .any(|field| field == "query_locale")
      {
        errors.push(format!(
          "目标地区 {region} 尚未配置确定性主检索语言；请把 query_locale 加入 missing_fields 并确认实际检索语言"
        ));
      }
    }
  } else if intent.account_source.is_some() && intent.query_locale.is_some() {
    errors.push("直接账号、作品或关系列表来源不得设置 query_locale 或翻译标识".to_string());
  }
  let account_fields = account_field_keys();
  for field in unique_invalid_values(&intent.selected_fields, &account_fields) {
    errors.push(format!(
      "collection_intent_v1.selected_fields 包含未知或重复字段 {field}"
    ));
  }
  if intent
    .time_range_days
    .is_some_and(|value| !matches!(value, 1 | 7 | 30 | 180))
  {
    errors.push("collection_intent_v1.time_range_days 只能是 1、7、30 或 180".to_string());
  }
  if intent
    .age_range
    .as_ref()
    .is_some_and(|range| range.min < 0 || range.max > 130 || range.min > range.max)
  {
    errors.push("collection_intent_v1.age_range 必须是 0 到 130 的有效闭区间".to_string());
  }
  if let Some(genders) = intent.gender_filter.as_ref() {
    for gender in unique_invalid_values(
      genders,
      &[
        "male".to_string(),
        "female".to_string(),
        "other".to_string(),
      ],
    ) {
      errors.push(format!(
        "collection_intent_v1.gender_filter 包含未知或重复值 {gender}"
      ));
    }
  }
  if intent.record_limit.is_some_and(|value| value <= 0) {
    errors.push("collection_intent_v1.record_limit 必须是正整数或 null".to_string());
  }
  if intent.budget_limit_micros.is_some_and(|value| value <= 0) {
    errors.push("collection_intent_v1.budget_limit_micros 必须是正整数或 null".to_string());
  }
  for field in unique_invalid_values(
    &intent.missing_fields,
    &BUSINESS_FIELDS
      .iter()
      .map(|value| (*value).to_string())
      .collect::<Vec<_>>(),
  ) {
    errors.push(format!(
      "collection_intent_v1.missing_fields 包含未知或重复字段 {field}"
    ));
  }
  if !(0.0..=1.0).contains(&intent.confidence) {
    errors.push("collection_intent_v1.confidence 必须位于 0 到 1 之间".to_string());
  }
  if errors.is_empty() {
    Ok(())
  } else {
    Err(errors)
  }
}

pub(crate) fn valid_query_locale(value: &str) -> bool {
  let mut segments = value.split('-');
  let Some(language) = segments.next() else {
    return false;
  };
  let Some(region) = segments.next() else {
    return false;
  };
  segments.next().is_none()
    && (2..=3).contains(&language.len())
    && language
      .chars()
      .all(|character| character.is_ascii_lowercase())
    && region.len() == 2
    && region
      .chars()
      .all(|character| character.is_ascii_uppercase())
    && normalize_country_region(Some(region)).is_some()
}

pub(crate) fn primary_query_locale(region: &str) -> Option<&'static str> {
  Some(match region {
    "GB" => "en-GB",
    "US" => "en-US",
    "CA" => "en-CA",
    "AU" => "en-AU",
    "NZ" => "en-NZ",
    "IE" => "en-IE",
    "SG" => "en-SG",
    "ZA" => "en-ZA",
    "NG" => "en-NG",
    "KE" => "en-KE",
    "CN" => "zh-CN",
    "HK" => "zh-HK",
    "MO" => "zh-MO",
    "TW" => "zh-TW",
    "JP" => "ja-JP",
    "KR" => "ko-KR",
    "FR" => "fr-FR",
    "DE" => "de-DE",
    "AT" => "de-AT",
    "CH" => "de-CH",
    "ES" => "es-ES",
    "MX" => "es-MX",
    "AR" => "es-AR",
    "CL" => "es-CL",
    "CO" => "es-CO",
    "PE" => "es-PE",
    "IT" => "it-IT",
    "PT" => "pt-PT",
    "BR" => "pt-BR",
    "NL" => "nl-NL",
    "BE" => "nl-BE",
    "SE" => "sv-SE",
    "NO" => "no-NO",
    "DK" => "da-DK",
    "FI" => "fi-FI",
    "PL" => "pl-PL",
    "CZ" => "cs-CZ",
    "GR" => "el-GR",
    "RO" => "ro-RO",
    "HU" => "hu-HU",
    "BG" => "bg-BG",
    "UA" => "uk-UA",
    "RU" => "ru-RU",
    "TR" => "tr-TR",
    "IL" => "he-IL",
    "SA" => "ar-SA",
    "AE" => "ar-AE",
    "EG" => "ar-EG",
    "MA" => "ar-MA",
    "IN" => "hi-IN",
    "PK" => "ur-PK",
    "BD" => "bn-BD",
    "TH" => "th-TH",
    "VN" => "vi-VN",
    "ID" => "id-ID",
    "MY" => "ms-MY",
    "PH" => "fil-PH",
    _ => return None,
  })
}

pub(crate) fn query_matches_locale_script(query_locale: &str, value: &str) -> bool {
  let language = query_locale.split('-').next().unwrap_or_default();
  match language {
    "zh" => value.chars().any(is_han),
    "ja" => value
      .chars()
      .any(|character| is_han(character) || is_japanese_kana(character)),
    "ko" => value.chars().any(is_hangul),
    "ru" | "uk" | "bg" => value.chars().any(is_cyrillic),
    "ar" => value.chars().any(is_arabic),
    language if is_latin_query_language(language) => {
      value.chars().any(char::is_alphabetic)
        && !value.chars().any(|character| {
          is_han(character)
            || is_japanese_kana(character)
            || is_hangul(character)
            || is_cyrillic(character)
            || is_arabic(character)
        })
    }
    _ => true,
  }
}

fn is_latin_query_language(language: &str) -> bool {
  matches!(
    language,
    "en"
      | "fr"
      | "de"
      | "es"
      | "it"
      | "pt"
      | "nl"
      | "sv"
      | "no"
      | "da"
      | "fi"
      | "pl"
      | "cs"
      | "ro"
      | "hu"
      | "tr"
      | "vi"
      | "id"
      | "ms"
      | "fil"
  )
}

fn is_han(character: char) -> bool {
  matches!(
    character as u32,
    0x3400..=0x4DBF
      | 0x4E00..=0x9FFF
      | 0xF900..=0xFAFF
      | 0x20000..=0x2A6DF
      | 0x2A700..=0x2EE5F
      | 0x30000..=0x323AF
  )
}

fn is_japanese_kana(character: char) -> bool {
  matches!(
    character as u32,
    0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9D
  )
}

fn is_hangul(character: char) -> bool {
  matches!(
    character as u32,
    0x1100..=0x11FF
      | 0x3130..=0x318F
      | 0xA960..=0xA97F
      | 0xAC00..=0xD7AF
      | 0xD7B0..=0xD7FF
  )
}

fn is_cyrillic(character: char) -> bool {
  matches!(
    character as u32,
    0x0400..=0x052F | 0x1C80..=0x1C8F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F
  )
}

fn is_arabic(character: char) -> bool {
  matches!(
    character as u32,
    0x0600..=0x06FF
      | 0x0750..=0x077F
      | 0x0870..=0x089F
      | 0x08A0..=0x08FF
      | 0xFB50..=0xFDFF
      | 0xFE70..=0xFEFF
  )
}

fn unique_invalid_values(values: &[String], allowed: &[String]) -> Vec<String> {
  let mut seen = BTreeSet::new();
  values
    .iter()
    .filter(|value| !allowed.contains(value) || !seen.insert((*value).clone()))
    .cloned()
    .collect()
}

fn nullable_string() -> Value {
  json!({ "type": ["string", "null"] })
}

fn nullable_positive_integer() -> Value {
  json!({
    "anyOf": [
      { "type": "null" },
      { "type": "integer", "minimum": 1 }
    ]
  })
}

fn nullable_enum(values: &[&str]) -> Value {
  json!({
    "anyOf": [
      { "type": "null" },
      { "type": "string", "enum": values }
    ]
  })
}

fn nullable_owned_enum(values: &[String]) -> Value {
  json!({
    "anyOf": [
      { "type": "null" },
      { "type": "string", "enum": values }
    ]
  })
}

#[cfg(test)]
mod tests {
  use serde_json::{json, Value};

  use super::{collection_intent_schema, parse_collection_intent, query_matches_locale_script};

  fn valid_intent() -> Value {
    json!({
      "schema_version": 1,
      "platform": "tiktok",
      "account_source": "user_search",
      "source_input": "pet supplies",
      "query_locale": "en-GB",
      "region_code": "GB",
      "selected_fields": ["bio", "followers_count"],
      "time_range_days": 30,
      "age_range": null,
      "gender_filter": null,
      "record_limit": 10,
      "budget_limit_micros": 100000,
      "missing_fields": [],
      "confidence": 0.94
    })
  }

  #[test]
  fn accepts_a_strict_collection_intent() {
    let parsed = parse_collection_intent(&valid_intent()).expect("有效意图应通过 Schema");

    assert_eq!(parsed.schema_version, 1);
    assert_eq!(parsed.region_code.as_deref(), Some("GB"));
    assert_eq!(parsed.query_locale.as_deref(), Some("en-GB"));
    assert_eq!(parsed.source_input.as_deref(), Some("pet supplies"));
  }

  #[test]
  fn rejects_execution_fields_and_missing_top_level_fields() {
    let mut intent = valid_intent();
    intent["endpoint_key"] = json!("tiktok.user_search");
    intent
      .as_object_mut()
      .expect("对象")
      .remove("budget_limit_micros");

    let errors = parse_collection_intent(&intent).expect_err("越权字段和缺失字段必须被拒绝");
    let message = errors.join("\n");
    assert!(message.contains("endpoint_key"));
    assert!(message.contains("budget_limit_micros"));
  }

  #[test]
  fn rejects_non_iso_regions_invalid_locales_and_unknown_catalog_values() {
    let mut intent = valid_intent();
    intent["region_code"] = json!("UK");
    intent["query_locale"] = json!("english-uk");
    intent["account_source"] = json!("model_selected_endpoint");
    intent["selected_fields"] = json!(["bio", "inferred_country"]);

    let errors = parse_collection_intent(&intent).expect_err("不规范意图必须被拒绝");
    let message = errors.join("\n");
    assert!(message.contains("region_code"));
    assert!(message.contains("query_locale"));
    assert!(message.contains("account_source"));
    assert!(message.contains("selected_fields"));
  }

  #[test]
  fn rejects_non_primary_or_untranslated_british_search_queries() {
    for invalid_locale in ["zh-GB", "zz-GB"] {
      let mut intent = valid_intent();
      intent["query_locale"] = json!(invalid_locale);

      let errors = parse_collection_intent(&intent).expect_err("英国检索必须使用 en-GB");
      assert!(errors.join("\n").contains("en-GB"));
    }

    let mut untranslated = valid_intent();
    untranslated["source_input"] = json!("宠物用品");
    let errors =
      parse_collection_intent(&untranslated).expect_err("未标记待确认的英国实际检索词不能仍为中文");
    assert!(errors.join("\n").contains("英文实际检索词"));

    untranslated["missing_fields"] = json!(["source_input"]);
    parse_collection_intent(&untranslated).expect("待确认的专有名词原文允许保留");
  }

  #[test]
  fn rejects_non_english_scripts_and_regions_without_a_primary_query_locale() {
    for source_input in [
      "товары для домашних животных",
      "مستلزمات الحيوانات الأليفة",
      "ペット用品",
    ] {
      let mut intent = valid_intent();
      intent["source_input"] = json!(source_input);

      let errors =
        parse_collection_intent(&intent).expect_err("en-GB 不能接受其他字母脚本冒充英文检索词");
      assert!(errors.join("\n").contains("英文实际检索词"));
    }

    let mut unmapped_region = valid_intent();
    unmapped_region["region_code"] = json!("AF");
    unmapped_region["query_locale"] = json!("ps-AF");
    let errors = parse_collection_intent(&unmapped_region)
      .expect_err("没有确定性主语言映射的地区必须进入待确认");
    assert!(errors.join("\n").contains("主检索语言"));
  }

  #[test]
  fn validates_search_query_scripts_for_non_english_regions() {
    for (region, locale, invalid_query, valid_query) in [
      ("JP", "ja-JP", "pet supplies", "ペット用品"),
      (
        "RU",
        "ru-RU",
        "pet supplies",
        "товары для домашних животных",
      ),
      ("CN", "zh-CN", "pet supplies", "宠物用品"),
    ] {
      let mut invalid = valid_intent();
      invalid["region_code"] = json!(region);
      invalid["query_locale"] = json!(locale);
      invalid["source_input"] = json!(invalid_query);

      let errors =
        parse_collection_intent(&invalid).expect_err("非英语地区不能接受错误文字脚本的检索词");
      assert!(
        errors.iter().any(|error| error.contains(locale)),
        "{locale} 的错误信息必须指出目标检索语言：{errors:?}"
      );

      invalid["source_input"] = json!(valid_query);
      parse_collection_intent(&invalid)
        .unwrap_or_else(|errors| panic!("{locale} 本地文字检索词应通过：{errors:?}"));
    }
  }

  #[test]
  fn recognizes_supported_query_script_families_and_latin_conflicts() {
    for (locale, valid, invalid) in [
      ("zh-CN", "宠物用品", "pet supplies"),
      ("ja-JP", "ペット用品", "pet supplies"),
      ("ko-KR", "반려동물 용품", "pet supplies"),
      ("ru-RU", "товары для животных", "pet supplies"),
      ("uk-UA", "товари для тварин", "pet supplies"),
      ("bg-BG", "стоки за домашни любимци", "pet supplies"),
      ("ar-SA", "مستلزمات الحيوانات الأليفة", "pet supplies"),
    ] {
      assert!(
        query_matches_locale_script(locale, valid),
        "{locale} 应接受对应文字脚本"
      );
      assert!(
        !query_matches_locale_script(locale, invalid),
        "{locale} 不应接受纯英文检索词"
      );
    }

    for locale in ["en-GB", "fr-FR", "de-DE", "es-ES", "vi-VN"] {
      assert!(query_matches_locale_script(locale, "café supplies"));
      for conflict in ["宠物用品", "반려동물 용품", "товары", "مستلزمات"] {
        assert!(
          !query_matches_locale_script(locale, conflict),
          "拉丁语言 {locale} 不应接受明显冲突的文字脚本"
        );
      }
    }
  }

  #[test]
  fn direct_identifiers_remain_untranslated_and_skip_query_script_validation() {
    for source_input in [
      "@PetBrandUK",
      "account_123456",
      "https://www.tiktok.com/@PetBrandUK",
      "https://xhslink.com/m/3ZSCJZAMz0a",
    ] {
      let mut intent = valid_intent();
      intent["account_source"] = json!("direct_account");
      intent["source_input"] = json!(source_input);
      intent["query_locale"] = Value::Null;

      let parsed = parse_collection_intent(&intent)
        .unwrap_or_else(|errors| panic!("直接来源 {source_input} 必须原样保留：{errors:?}"));
      assert_eq!(parsed.source_input.as_deref(), Some(source_input));
    }
  }

  #[test]
  fn allows_explicit_nulls_for_business_fields_that_need_review() {
    let intent = json!({
      "schema_version": 1,
      "platform": null,
      "account_source": null,
      "source_input": null,
      "query_locale": null,
      "region_code": null,
      "selected_fields": [],
      "time_range_days": null,
      "age_range": null,
      "gender_filter": null,
      "record_limit": null,
      "budget_limit_micros": null,
      "missing_fields": ["platform", "region_code", "budget_limit_micros"],
      "confidence": 0.2
    });

    let parsed = parse_collection_intent(&intent).expect("业务缺失必须使用显式 null 表达");
    assert!(parsed.platform.is_none());
    assert_eq!(parsed.missing_fields.len(), 3);
  }

  #[test]
  fn publishes_a_closed_json_schema_without_execution_plan_fields() {
    let schema = collection_intent_schema();
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["properties"]["schema_version"]["const"], json!(1));
    assert!(schema["properties"].get("endpoint_key").is_none());
    assert!(schema["properties"].get("steps").is_none());
    assert!(schema["properties"].get("cost_estimate").is_none());
    assert!(schema["required"]
      .as_array()
      .is_some_and(|fields| fields.len() == 14));
  }
}
