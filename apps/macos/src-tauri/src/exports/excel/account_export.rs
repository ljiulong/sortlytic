use std::collections::BTreeSet;

use rusqlite::{params, types::Type, Connection, OptionalExtension, Row};
use rust_xlsxwriter::{Format, FormatAlign, FormatBorder, Workbook, Worksheet};
use serde_json::Value;

use crate::collection::{
  get_account_collection_capabilities, AccountFieldAvailability, AccountFieldValueType,
};
use crate::domain::AppResult;

use super::super::{database_error, export_error};

#[derive(Debug)]
pub(super) struct AccountExport {
  pub(super) is_v4: bool,
  pub(super) selected_fields: Vec<ExportField>,
  pub(super) catalog_fields: Vec<ExportField>,
  pub(super) accounts: Vec<ExportAccount>,
}

#[derive(Debug, Clone)]
pub(super) struct ExportField {
  pub(super) key: String,
  pub(super) label: String,
  pub(super) description: String,
  pub(super) group: String,
  pub(super) value_type: AccountFieldValueType,
  pub(super) platforms: String,
  pub(super) sources: String,
  pub(super) selected: bool,
}

#[derive(Debug)]
pub(super) struct ExportAccount {
  pub(super) username: Option<String>,
  pub(super) account: Option<String>,
  pub(super) platform_user_id: Option<String>,
  pub(super) profile_text: Option<String>,
  pub(super) country_region: Option<String>,
  pub(super) region_source: Option<String>,
  pub(super) region_confidence: Option<String>,
  pub(super) platform: String,
  pub(super) gender: Option<String>,
  pub(super) age: Option<i64>,
  pub(super) followers_count: Option<i64>,
  pub(super) posts_count: Option<i64>,
  pub(super) last_posted_at: Option<String>,
  pub(super) profile_url: Option<String>,
  pub(super) data_source: String,
  pub(super) collected_at: String,
  pub(super) notes: Option<String>,
  pub(super) account_fields_json: Value,
  pub(super) field_evidence_json: Value,
}

pub(super) fn load_account_export(
  connection: &Connection,
  task_id: &str,
) -> AppResult<AccountExport> {
  let scope = connection
    .query_row(
      "SELECT account_source, selected_fields_json, platforms_json
       FROM collection_task WHERE id = ?1",
      params![task_id],
      |row| {
        Ok((
          row.get::<_, Option<String>>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
        ))
      },
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| export_error("任务不存在，无法导出账号数据"))?;
  let selected_keys = parse_string_array(&scope.1, "selected_fields_json")?;
  let platforms = parse_string_array(&scope.2, "platforms_json")?;
  let accounts = load_accounts(connection, task_id)?;
  let catalog_fields = if scope.0.is_some() {
    build_field_catalog(&platforms, &selected_keys, &accounts)?
  } else {
    Vec::new()
  };
  let selected_set = selected_keys
    .iter()
    .map(String::as_str)
    .collect::<BTreeSet<_>>();
  if selected_set.len() != selected_keys.len() {
    return Err(export_error("所选账号字段不得重复"));
  }
  for key in &selected_keys {
    if !catalog_fields.iter().any(|field| field.key == *key) {
      return Err(export_error(format!("所选账号字段不在能力目录中：{key}")));
    }
  }
  let selected_fields = catalog_fields
    .iter()
    .filter(|field| selected_set.contains(field.key.as_str()))
    .cloned()
    .collect();

  Ok(AccountExport {
    is_v4: scope.0.is_some(),
    selected_fields,
    catalog_fields,
    accounts,
  })
}

pub(super) fn write_field_guide_sheet(
  workbook: &mut Workbook,
  export: &AccountExport,
) -> AppResult<()> {
  let worksheet = workbook
    .add_worksheet()
    .set_name("字段说明")
    .map_err(export_error)?;
  worksheet.set_screen_gridlines(false);
  let widths = [24.0, 18.0, 16.0, 14.0, 38.0, 18.0, 52.0, 18.0, 52.0];
  for (column, width) in widths.iter().enumerate() {
    worksheet
      .set_column_width(column as u16, *width)
      .map_err(export_error)?;
  }
  let headers = [
    "字段代码",
    "中文名",
    "分类",
    "值类型",
    "支持平台",
    "任务选择状态",
    "来源 / 补全操作 / 原始路径",
    "缺失语义",
    "说明与限制",
  ];
  let header = Format::new()
    .set_bold()
    .set_font_color("#FFFFFF")
    .set_background_color("#4472C4")
    .set_border(FormatBorder::Thin)
    .set_align(FormatAlign::Center)
    .set_align(FormatAlign::VerticalCenter)
    .set_text_wrap();
  let body = Format::new()
    .set_text_wrap()
    .set_align(FormatAlign::VerticalCenter)
    .set_border(FormatBorder::Thin)
    .set_border_color("#D9E2F3");
  for (column, value) in headers.iter().enumerate() {
    worksheet
      .write_with_format(0, column as u16, *value, &header)
      .map_err(export_error)?;
  }
  let base_fields = [
    ("platform", "平台", "平台代码"),
    ("display_name", "显示名称", "账号公开显示名称"),
    ("account_handle", "账号", "用户名、抖音号或小红书号"),
    ("platform_user_id", "平台用户 ID", "平台返回的用户标识"),
    ("data_source", "数据来源", "TikHub 来源端点或来源说明"),
    ("collected_at", "采集时间", "本地记录的采集时间"),
  ];
  for (index, (key, label, description)) in base_fields.iter().enumerate() {
    write_guide_row(
      worksheet,
      (index + 1) as u32,
      [
        key,
        label,
        "基础身份",
        "文本 / 时间",
        "TikTok；抖音；小红书",
        "基础字段（始终）",
        "账号发现步骤",
        "未采集到",
        description,
      ],
      &body,
    )?;
  }
  for (index, field) in export.catalog_fields.iter().enumerate() {
    let selected = if field.selected {
      "已选择"
    } else {
      "未选择"
    };
    let missing = if field.selected {
      "未采集到"
    } else {
      "任务未设置"
    };
    write_guide_row(
      worksheet,
      (index + base_fields.len() + 1) as u32,
      [
        &field.key,
        &field.label,
        group_label(&field.group),
        value_type_label(field.value_type),
        &field.platforms,
        selected,
        &field.sources,
        missing,
        &field.description,
      ],
      &body,
    )?;
  }
  Ok(())
}

fn write_guide_row(
  worksheet: &mut Worksheet,
  row: u32,
  values: [&str; 9],
  format: &Format,
) -> AppResult<()> {
  for (column, value) in values.iter().enumerate() {
    worksheet
      .write_with_format(row, column as u16, *value, format)
      .map_err(export_error)?;
  }
  Ok(())
}

fn value_type_label(value: AccountFieldValueType) -> &'static str {
  match value {
    AccountFieldValueType::Text => "文本",
    AccountFieldValueType::Integer => "整数",
    AccountFieldValueType::Boolean => "布尔值",
    AccountFieldValueType::TextList => "文本列表",
    AccountFieldValueType::Timestamp => "时间",
  }
}

fn group_label(value: &str) -> &str {
  match value {
    "profile" => "账号资料",
    "demographics" => "人口属性",
    "statistics" => "账号统计",
    "activity" => "账号活跃",
    "platform_specific" => "平台特有",
    _ => value,
  }
}

fn build_field_catalog(
  platforms: &[String],
  selected_keys: &[String],
  accounts: &[ExportAccount],
) -> AppResult<Vec<ExportField>> {
  let task_capabilities = platforms
    .iter()
    .map(|platform| get_account_collection_capabilities(platform))
    .collect::<AppResult<Vec<_>>>()?;
  let catalog_capabilities = ["tiktok", "douyin", "xiaohongshu"]
    .into_iter()
    .map(get_account_collection_capabilities)
    .collect::<AppResult<Vec<_>>>()?;
  let definitions = task_capabilities
    .first()
    .ok_or_else(|| export_error("账号任务缺少平台能力"))?;
  let selected = selected_keys.iter().collect::<BTreeSet<_>>();

  definitions
    .fields
    .iter()
    .map(|definition| {
      let mut supported_platforms = Vec::new();
      let mut declared_sources = BTreeSet::new();
      for platform in &definition.supported_platforms {
        let Some(capability) = catalog_capabilities
          .iter()
          .find(|capability| capability.platform == *platform)
        else {
          continue;
        };
        let Some(field) = capability
          .fields
          .iter()
          .find(|field| field.key == definition.key)
        else {
          continue;
        };
        if field.availability == AccountFieldAvailability::Unsupported {
          continue;
        }
        supported_platforms.push(format!(
          "{}（{}）",
          capability.display_name,
          availability_label(field.availability)
        ));
        declared_sources.extend(field.required_operation_keys.iter().cloned());
      }
      for account in accounts {
        let Some(evidence) = account
          .field_evidence_json
          .get(&definition.key)
          .and_then(Value::as_object)
        else {
          continue;
        };
        for key in ["endpoint_key", "raw_path"] {
          if let Some(value) = evidence
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
          {
            declared_sources.insert(value.to_string());
          }
        }
      }
      if declared_sources.is_empty() {
        declared_sources.insert("账号来源响应（条件返回）".to_string());
      }
      Ok(ExportField {
        key: definition.key.clone(),
        label: definition.display_name.clone(),
        description: definition.description.clone(),
        group: definition.group.clone(),
        value_type: definition.value_type,
        platforms: supported_platforms.join("；"),
        sources: declared_sources.into_iter().collect::<Vec<_>>().join("；"),
        selected: selected.contains(&definition.key),
      })
    })
    .collect()
}

fn availability_label(value: AccountFieldAvailability) -> &'static str {
  match value {
    AccountFieldAvailability::Direct => "直接提供",
    AccountFieldAvailability::Enrichment => "需补全",
    AccountFieldAvailability::Conditional => "条件返回",
    AccountFieldAvailability::Unsupported => "不支持",
  }
}

fn load_accounts(connection: &Connection, task_id: &str) -> AppResult<Vec<ExportAccount>> {
  let mut statement = connection
    .prepare(
      "SELECT account.username, account.account, account.platform_user_id,
              account.profile_text, account.country_region, account.region_source,
              account.region_confidence, account.platform, account.gender, account.age,
              account.followers_count, account.posts_count, account.last_posted_at,
              account.profile_url, account.data_source, account.collected_at, account.notes,
              account.account_fields_json, account.field_evidence_json
       FROM collected_account AS account
       JOIN task_run AS run ON run.id = account.task_run_id
       WHERE run.task_id = ?1 AND account.output_included = 1
         AND run.id = (
           SELECT latest.id FROM task_run AS latest
           WHERE latest.task_id = ?1 AND latest.status IN ('success', 'partial_success')
           ORDER BY latest.run_sequence DESC LIMIT 1
         )
       ORDER BY account.created_at, account.id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_id], |row| {
      Ok(ExportAccount {
        username: row.get(0)?,
        account: row.get(1)?,
        platform_user_id: row.get(2)?,
        profile_text: row.get(3)?,
        country_region: row.get(4)?,
        region_source: row.get(5)?,
        region_confidence: row.get(6)?,
        platform: row.get(7)?,
        gender: row.get(8)?,
        age: row.get(9)?,
        followers_count: row.get(10)?,
        posts_count: row.get(11)?,
        last_posted_at: row.get(12)?,
        profile_url: row.get(13)?,
        data_source: row.get(14)?,
        collected_at: row.get(15)?,
        notes: row.get(16)?,
        account_fields_json: json_column(row, 17)?,
        field_evidence_json: json_column(row, 18)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn parse_string_array(value: &str, field: &str) -> AppResult<Vec<String>> {
  serde_json::from_str(value).map_err(|_| export_error(format!("{field} JSON 损坏")))
}

fn json_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Value> {
  let text = row.get::<_, String>(index)?;
  serde_json::from_str(&text)
    .map_err(|error| rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error)))
}
