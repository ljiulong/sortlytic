use std::path::Path;

use rust_xlsxwriter::{
  DataValidation, DataValidationRule, ExcelDateTime, Format, FormatAlign, FormatBorder, Formula,
  Table, TableStyle, Workbook, Worksheet,
};
use serde_json::Value;

#[path = "excel/account_export.rs"]
mod account_export;

use super::{export_error, open_workspace_connection, write_new_export_file, ReportView};
use crate::collection::AccountFieldValueType;
use crate::domain::AppResult;
use account_export::{
  load_account_export, write_field_guide_sheet, AccountExport, ExportAccount, ExportField,
};

const HEADERS: [&str; 18] = [
  "序号",
  "用户名",
  "账号",
  "平台用户ID",
  "个人信息",
  "国家/地区",
  "地区来源",
  "地区置信度",
  "社交平台信息",
  "性别",
  "年龄",
  "粉丝数",
  "作品数",
  "最近发文时间",
  "主页链接",
  "数据来源",
  "收集日期",
  "备注",
];
const COLUMN_WIDTHS: [f64; 18] = [
  8.0, 16.0, 18.0, 20.0, 34.0, 14.0, 24.0, 14.0, 18.0, 12.0, 10.0, 12.0, 12.0, 18.0, 34.0, 24.0,
  16.0, 42.0,
];

pub(super) fn write_excel(root_path: &Path, path: &Path, report: &ReportView) -> AppResult<()> {
  let connection = open_workspace_connection(root_path)?;
  let export = load_account_export(&connection, &report.task_id)?;
  let mut workbook = Workbook::new();
  if export.is_v4 {
    write_dynamic_accounts_sheet(&mut workbook, &export)?;
    write_field_guide_sheet(&mut workbook, &export)?;
  } else {
    write_accounts_sheet(&mut workbook, &export.accounts)?;
  }
  write_instructions_sheet(&mut workbook)?;
  write_enums_sheet(&mut workbook)?;
  write_sources_sheet(&mut workbook)?;
  let bytes = workbook.save_to_buffer().map_err(export_error)?;
  write_new_export_file(path, &bytes)
}

fn write_dynamic_accounts_sheet(workbook: &mut Workbook, export: &AccountExport) -> AppResult<()> {
  let worksheet = workbook
    .add_worksheet()
    .set_name("用户数据收集表")
    .map_err(export_error)?;
  let headers = [
    "平台",
    "显示名称",
    "账号",
    "平台用户 ID",
    "数据来源",
    "采集时间",
  ]
  .into_iter()
  .map(str::to_string)
  .chain(
    export
      .selected_fields
      .iter()
      .map(|field| field.label.clone()),
  )
  .collect::<Vec<_>>();
  let last_column = (headers.len() - 1) as u16;
  worksheet.set_screen_gridlines(false);
  worksheet.set_freeze_panes(4, 0).map_err(export_error)?;
  for column in 0..headers.len() {
    let width = if column == 1 || column >= 6 {
      22.0
    } else {
      18.0
    };
    worksheet
      .set_column_width(column as u16, width)
      .map_err(export_error)?;
  }
  let title = Format::new()
    .set_bold()
    .set_font_size(16)
    .set_font_color("#FFFFFF")
    .set_background_color("#1F4E78")
    .set_align(FormatAlign::Center)
    .set_align(FormatAlign::VerticalCenter);
  let note = Format::new()
    .set_font_color("#44546A")
    .set_background_color("#D9EAF7")
    .set_text_wrap()
    .set_align(FormatAlign::VerticalCenter);
  worksheet
    .merge_range(0, 0, 0, last_column, "账号公开数据导出", &title)
    .map_err(export_error)?;
  worksheet
    .merge_range(
      1,
      0,
      1,
      last_column,
      "基础身份字段始终存在，仅追加本任务选择的扩展字段；技术游标、日志 ID、临时 URL 与签名令牌不导出。",
      &note,
    )
    .map_err(export_error)?;
  let header = header_format();
  for (column, value) in headers.iter().enumerate() {
    worksheet
      .write_with_format(3, column as u16, value, &header)
      .map_err(export_error)?;
  }
  let body = Format::new()
    .set_border(FormatBorder::Thin)
    .set_border_color("#D9E2F3")
    .set_align(FormatAlign::VerticalCenter)
    .set_text_wrap();
  let centered = body.clone().set_align(FormatAlign::Center);
  let integer = centered.clone().set_num_format("#,##0");
  let date = centered.clone().set_num_format("yyyy-mm-dd");
  for (index, account) in export.accounts.iter().enumerate() {
    let row = (index + 4) as u32;
    worksheet.set_row_height(row, 36).map_err(export_error)?;
    write_dynamic_base_fields(worksheet, row, account, &body, &centered, &date)?;
    for (field_index, field) in export.selected_fields.iter().enumerate() {
      write_dynamic_field(
        worksheet,
        row,
        (field_index + 6) as u16,
        field,
        account.account_fields_json.get(&field.key),
        &body,
        &centered,
        &integer,
        &date,
      )?;
    }
  }
  if !export.accounts.is_empty() {
    let table = Table::new()
      .set_name("CollectedAccounts")
      .set_style(TableStyle::Light9)
      .set_banded_rows(true);
    worksheet
      .add_table(
        3,
        0,
        (export.accounts.len() + 3) as u32,
        last_column,
        &table,
      )
      .map_err(export_error)?;
  }
  Ok(())
}

fn write_dynamic_base_fields(
  worksheet: &mut Worksheet,
  row: u32,
  account: &ExportAccount,
  body: &Format,
  centered: &Format,
  date: &Format,
) -> AppResult<()> {
  worksheet
    .write_with_format(row, 0, platform_label(&account.platform), centered)
    .map_err(export_error)?;
  write_required_text(worksheet, row, 1, account.username.as_deref(), body)?;
  write_required_text(worksheet, row, 2, account.account.as_deref(), body)?;
  write_required_text(worksheet, row, 3, account.platform_user_id.as_deref(), body)?;
  write_required_text(worksheet, row, 4, Some(&account.data_source), body)?;
  write_date(worksheet, row, 5, Some(&account.collected_at), date)
}

#[allow(clippy::too_many_arguments)]
fn write_dynamic_field(
  worksheet: &mut Worksheet,
  row: u32,
  column: u16,
  field: &ExportField,
  value: Option<&Value>,
  body: &Format,
  centered: &Format,
  integer: &Format,
  date: &Format,
) -> AppResult<()> {
  let Some(value) = value.filter(|value| !value.is_null()) else {
    return worksheet
      .write_with_format(row, column, "未采集到", body)
      .map(|_| ())
      .map_err(export_error);
  };
  match field.value_type {
    AccountFieldValueType::Integer => match value.as_i64() {
      Some(number) => worksheet.write_with_format(row, column, number, integer),
      None => worksheet.write_with_format(row, column, value.to_string(), body),
    }
    .map(|_| ())
    .map_err(export_error),
    AccountFieldValueType::Boolean => match value.as_bool() {
      Some(flag) => worksheet.write_with_format(row, column, flag, centered),
      None => worksheet.write_with_format(row, column, value.to_string(), body),
    }
    .map(|_| ())
    .map_err(export_error),
    AccountFieldValueType::Timestamp => {
      let text = value.as_str().unwrap_or_default();
      if text.len() >= 10 && ExcelDateTime::parse_from_str(&text[..10]).is_ok() {
        write_date(worksheet, row, column, Some(text), date)
      } else {
        worksheet
          .write_with_format(row, column, text, body)
          .map(|_| ())
          .map_err(export_error)
      }
    }
    AccountFieldValueType::TextList => {
      let text = value
        .as_array()
        .map(|items| {
          items
            .iter()
            .map(|item| {
              item
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| item.to_string())
            })
            .collect::<Vec<_>>()
            .join("；")
        })
        .unwrap_or_else(|| value.to_string());
      worksheet
        .write_with_format(row, column, text, body)
        .map(|_| ())
        .map_err(export_error)
    }
    AccountFieldValueType::Text => {
      let text = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
      write_required_text(worksheet, row, column, Some(&text), body)
    }
  }
}

fn write_required_text(
  worksheet: &mut Worksheet,
  row: u32,
  column: u16,
  value: Option<&str>,
  format: &Format,
) -> AppResult<()> {
  worksheet
    .write_with_format(
      row,
      column,
      value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("未采集到"),
      format,
    )
    .map(|_| ())
    .map_err(export_error)
}

fn write_accounts_sheet(workbook: &mut Workbook, accounts: &[ExportAccount]) -> AppResult<()> {
  let worksheet = workbook
    .add_worksheet()
    .set_name("用户数据收集表")
    .map_err(export_error)?;
  worksheet.set_screen_gridlines(false);
  worksheet.set_freeze_panes(4, 0).map_err(export_error)?;
  for (column, width) in COLUMN_WIDTHS.iter().enumerate() {
    worksheet
      .set_column_width(column as u16, *width)
      .map_err(export_error)?;
  }
  let title = Format::new()
    .set_bold()
    .set_font_size(16)
    .set_font_color("#FFFFFF")
    .set_background_color("#1F4E78")
    .set_align(FormatAlign::Center)
    .set_align(FormatAlign::VerticalCenter);
  let note = Format::new()
    .set_font_color("#44546A")
    .set_background_color("#D9EAF7")
    .set_text_wrap()
    .set_align(FormatAlign::VerticalCenter);
  worksheet
    .merge_range(
      0,
      0,
      0,
      17,
      "社交平台用户数据收集模板：国家/地区合规版",
      &title,
    )
    .map_err(export_error)?;
  worksheet
    .merge_range(
      1,
      0,
      1,
      17,
      "统一整理 TikHub API 返回的公开账号字段。国家/地区、年龄和性别只记录接口或公开资料明确提供的值，不做推断。",
      &note,
    )
    .map_err(export_error)?;
  worksheet.set_row_height(0, 28).map_err(export_error)?;
  worksheet.set_row_height(1, 36).map_err(export_error)?;

  let header = Format::new()
    .set_bold()
    .set_font_color("#FFFFFF")
    .set_background_color("#4472C4")
    .set_border(FormatBorder::Thin)
    .set_align(FormatAlign::Center)
    .set_align(FormatAlign::VerticalCenter)
    .set_text_wrap();
  for (column, value) in HEADERS.iter().enumerate() {
    worksheet
      .write_with_format(3, column as u16, *value, &header)
      .map_err(export_error)?;
  }
  worksheet.set_row_height(3, 24).map_err(export_error)?;

  let body = Format::new()
    .set_border(FormatBorder::Thin)
    .set_border_color("#D9E2F3")
    .set_align(FormatAlign::VerticalCenter)
    .set_text_wrap();
  let centered = body.clone().set_align(FormatAlign::Center);
  let integer = centered.clone().set_num_format("#,##0");
  let date = centered.clone().set_num_format("yyyy-mm-dd");
  let last_excel_row = accounts.len().max(200) + 4;
  for row_index in 0..(last_excel_row - 4) {
    let row = (row_index + 4) as u32;
    worksheet.set_row_height(row, 36).map_err(export_error)?;
    for column in 0..18 {
      worksheet
        .write_blank(
          row,
          column,
          if matches!(column, 0 | 5..=12 | 16) {
            &centered
          } else {
            &body
          },
        )
        .map_err(export_error)?;
    }
    let excel_row = row + 1;
    worksheet
      .write_formula_with_format(
        row,
        0,
        Formula::new(format!("=IF(B{excel_row}<>\"\",ROW()-4,\"\")")),
        &centered,
      )
      .map_err(export_error)?;
  }
  for (index, account) in accounts.iter().enumerate() {
    write_account_row(
      worksheet,
      (index + 4) as u32,
      account,
      &body,
      &centered,
      &integer,
      &date,
    )?;
  }
  add_validations(worksheet, (last_excel_row - 1) as u32)?;
  let table = Table::new()
    .set_name("CollectedAccounts")
    .set_style(TableStyle::Light9)
    .set_banded_rows(true);
  worksheet
    .add_table(3, 0, (last_excel_row - 1) as u32, 17, &table)
    .map_err(export_error)?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_account_row(
  worksheet: &mut Worksheet,
  row: u32,
  account: &ExportAccount,
  body: &Format,
  centered: &Format,
  integer: &Format,
  date: &Format,
) -> AppResult<()> {
  write_optional(worksheet, row, 1, account.username.as_deref(), body)?;
  write_optional(worksheet, row, 2, account.account.as_deref(), body)?;
  write_optional(worksheet, row, 3, account.platform_user_id.as_deref(), body)?;
  write_optional(worksheet, row, 4, account.profile_text.as_deref(), body)?;
  write_optional(
    worksheet,
    row,
    5,
    account.country_region.as_deref(),
    centered,
  )?;
  let region_source = account.region_source.as_deref().map(|source| {
    if source == "TikHub API" {
      "platform_region_code"
    } else {
      source
    }
  });
  write_optional(worksheet, row, 6, region_source, centered)?;
  write_optional(
    worksheet,
    row,
    7,
    account.region_confidence.as_deref(),
    centered,
  )?;
  worksheet
    .write_with_format(row, 8, platform_label(&account.platform), centered)
    .map_err(export_error)?;
  write_optional(
    worksheet,
    row,
    9,
    account.gender.as_deref().map(gender_label),
    centered,
  )?;
  write_integer(worksheet, row, 10, account.age, centered)?;
  write_integer(worksheet, row, 11, account.followers_count, integer)?;
  write_integer(worksheet, row, 12, account.posts_count, integer)?;
  write_date(worksheet, row, 13, account.last_posted_at.as_deref(), date)?;
  write_optional(worksheet, row, 14, account.profile_url.as_deref(), body)?;
  let data_source = if account.data_source.starts_with("TikHub API") {
    "TikHub API"
  } else {
    account.data_source.as_str()
  };
  worksheet
    .write_with_format(row, 15, data_source, body)
    .map_err(export_error)?;
  write_date(worksheet, row, 16, Some(&account.collected_at), date)?;
  write_optional(worksheet, row, 17, account.notes.as_deref(), body)
}

fn write_optional(
  worksheet: &mut Worksheet,
  row: u32,
  column: u16,
  value: Option<&str>,
  format: &Format,
) -> AppResult<()> {
  if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
    worksheet
      .write_with_format(row, column, value, format)
      .map_err(export_error)?;
  }
  Ok(())
}

fn write_integer(
  worksheet: &mut Worksheet,
  row: u32,
  column: u16,
  value: Option<i64>,
  format: &Format,
) -> AppResult<()> {
  if let Some(value) = value {
    worksheet
      .write_with_format(row, column, value, format)
      .map_err(export_error)?;
  }
  Ok(())
}

fn write_date(
  worksheet: &mut Worksheet,
  row: u32,
  column: u16,
  value: Option<&str>,
  format: &Format,
) -> AppResult<()> {
  let Some(value) = value.and_then(|value| value.get(..10)) else {
    return Ok(());
  };
  let datetime = ExcelDateTime::parse_from_str(value).map_err(export_error)?;
  worksheet
    .write_datetime_with_format(row, column, &datetime, format)
    .map_err(export_error)?;
  Ok(())
}

fn add_validations(worksheet: &mut Worksheet, last_row: u32) -> AppResult<()> {
  for (column, formula) in [
    (6, "='字段枚举'!$A$2:$A$6"),
    (7, "='字段枚举'!$B$2:$B$6"),
    (8, "='字段枚举'!$C$2:$C$4"),
    (9, "='字段枚举'!$D$2:$D$5"),
    (15, "='字段枚举'!$E$2:$E$8"),
  ] {
    let validation = DataValidation::new().allow_list_formula(Formula::new(formula));
    worksheet
      .add_data_validation(4, column, last_row, column, &validation)
      .map_err(export_error)?;
  }
  let age = DataValidation::new().allow_whole_number(DataValidationRule::Between(0, 130));
  worksheet
    .add_data_validation(4, 10, last_row, 10, &age)
    .map_err(export_error)?;
  Ok(())
}

fn write_instructions_sheet(workbook: &mut Workbook) -> AppResult<()> {
  let worksheet = workbook
    .add_worksheet()
    .set_name("填写说明")
    .map_err(export_error)?;
  worksheet.set_screen_gridlines(false);
  for (column, width) in [24.0, 16.0, 44.0, 26.0, 46.0].iter().enumerate() {
    worksheet
      .set_column_width(column as u16, *width)
      .map_err(export_error)?;
  }
  let title = title_format();
  worksheet
    .merge_range(0, 0, 0, 4, "填写说明：国家/地区合规版", &title)
    .map_err(export_error)?;
  let headers = ["字段", "是否必填", "填写说明", "示例", "注意事项"];
  let header = header_format();
  for (column, value) in headers.iter().enumerate() {
    worksheet
      .write_with_format(2, column as u16, *value, &header)
      .map_err(export_error)?;
  }
  let rows = [
    [
      "国家/地区",
      "按需填写",
      "只记录公开或接口合法返回的国家/地区代码。",
      "US",
      "不代表真实 IP。",
    ],
    [
      "地区来源",
      "建议必填",
      "说明地区字段来自哪里。",
      "platform_region_code",
      "每条位置数据必须有来源。",
    ],
    [
      "性别",
      "可选",
      "只填写接口或公开资料明确提供的性别。",
      "未公开",
      "禁止根据头像、声音、姓名推断。",
    ],
    [
      "年龄",
      "可选",
      "只填写接口或公开资料明确提供的年龄。",
      "28",
      "未知留空，禁止推断。",
    ],
    [
      "数据来源",
      "建议必填",
      "记录字段的公开数据来源。",
      "TikHub API",
      "原始证据保存在本地证据库。",
    ],
  ];
  write_text_matrix(worksheet, 3, &rows)
}

fn write_enums_sheet(workbook: &mut Workbook) -> AppResult<()> {
  let worksheet = workbook
    .add_worksheet()
    .set_name("字段枚举")
    .map_err(export_error)?;
  let rows = [
    ["地区来源", "地区置信度", "社交平台信息", "性别", "数据来源"],
    [
      "platform_region_code",
      "高",
      "TikTok",
      "未公开",
      "TikHub API",
    ],
    ["public_profile_region", "中高", "Douyin", "男", "公开主页"],
    ["bio_declared_location", "中", "Xiaohongshu", "女", "评论区"],
    [
      "content_inferred_region",
      "低",
      "",
      "其他明确性别",
      "搜索结果",
    ],
    ["unknown", "未知", "", "", "人工补充"],
    ["", "", "", "", "MCP Agent"],
    ["", "", "", "", "用户提交"],
  ];
  write_text_matrix(worksheet, 0, &rows)?;
  worksheet
    .set_column_range_width(0, 4, 24)
    .map_err(export_error)?;
  Ok(())
}

fn write_sources_sheet(workbook: &mut Workbook) -> AppResult<()> {
  let worksheet = workbook
    .add_worksheet()
    .set_name("资料依据")
    .map_err(export_error)?;
  let rows = [
    ["资料依据", "说明", "链接", "用途"],
    [
      "TikHub API Reference",
      "TikHub 社交平台公开数据接口。",
      "https://docs.tikhub.io",
      "账号公开信息采集。",
    ],
    [
      "TikHub 用户信息接口",
      "返回充值余额和免费额度。",
      "https://docs.tikhub.io/186826050e0",
      "运行前额度门禁。",
    ],
    [
      "TikHub 实时计价接口",
      "返回端点实时报价。",
      "https://docs.tikhub.io/186826052e0",
      "预算预检与报价快照。",
    ],
  ];
  write_text_matrix(worksheet, 0, &rows)?;
  for (column, width) in [28.0, 52.0, 60.0, 42.0].iter().enumerate() {
    worksheet
      .set_column_width(column as u16, *width)
      .map_err(export_error)?;
  }
  Ok(())
}

fn write_text_matrix<const C: usize>(
  worksheet: &mut Worksheet,
  start_row: u32,
  rows: &[[&str; C]],
) -> AppResult<()> {
  let header = header_format();
  let body = Format::new()
    .set_text_wrap()
    .set_align(FormatAlign::VerticalCenter)
    .set_border(FormatBorder::Thin)
    .set_border_color("#D9E2F3");
  for (row_offset, row_values) in rows.iter().enumerate() {
    for (column, value) in row_values.iter().enumerate() {
      worksheet
        .write_with_format(
          start_row + row_offset as u32,
          column as u16,
          *value,
          if row_offset == 0 { &header } else { &body },
        )
        .map_err(export_error)?;
    }
  }
  Ok(())
}

fn title_format() -> Format {
  Format::new()
    .set_bold()
    .set_font_size(15)
    .set_font_color("#FFFFFF")
    .set_background_color("#1F4E78")
    .set_align(FormatAlign::Center)
}

fn header_format() -> Format {
  Format::new()
    .set_bold()
    .set_font_color("#FFFFFF")
    .set_background_color("#4472C4")
    .set_border(FormatBorder::Thin)
    .set_align(FormatAlign::Center)
    .set_align(FormatAlign::VerticalCenter)
    .set_text_wrap()
}

fn platform_label(value: &str) -> &str {
  match value {
    "tiktok" => "TikTok",
    "douyin" => "Douyin",
    "xiaohongshu" => "Xiaohongshu",
    _ => value,
  }
}

fn gender_label(value: &str) -> &str {
  match value {
    "male" => "男",
    "female" => "女",
    "other" => "其他明确性别",
    _ => "未公开",
  }
}
