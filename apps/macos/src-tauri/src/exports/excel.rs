use std::path::Path;

use rusqlite::{params, Connection};
use rust_xlsxwriter::{
  DataValidation, DataValidationRule, ExcelDateTime, Format, FormatAlign, FormatBorder, Formula,
  Table, TableStyle, Workbook, Worksheet,
};

use super::{export_error, open_workspace_connection, write_new_export_file, ReportView};
use crate::domain::AppResult;

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

#[derive(Debug)]
struct ExportAccount {
  username: Option<String>,
  account: Option<String>,
  platform_user_id: Option<String>,
  profile_text: Option<String>,
  country_region: Option<String>,
  region_source: Option<String>,
  region_confidence: Option<String>,
  platform: String,
  gender: Option<String>,
  age: Option<i64>,
  followers_count: Option<i64>,
  posts_count: Option<i64>,
  last_posted_at: Option<String>,
  profile_url: Option<String>,
  data_source: String,
  collected_at: String,
  notes: Option<String>,
}

pub(super) fn write_excel(root_path: &Path, path: &Path, report: &ReportView) -> AppResult<()> {
  let connection = open_workspace_connection(root_path)?;
  let accounts = load_accounts(&connection, &report.task_id)?;
  let mut workbook = Workbook::new();
  write_accounts_sheet(&mut workbook, &accounts)?;
  write_instructions_sheet(&mut workbook)?;
  write_enums_sheet(&mut workbook)?;
  write_sources_sheet(&mut workbook)?;
  let bytes = workbook.save_to_buffer().map_err(export_error)?;
  write_new_export_file(path, &bytes)
}

fn load_accounts(connection: &Connection, task_id: &str) -> AppResult<Vec<ExportAccount>> {
  let mut statement = connection
    .prepare(
      "SELECT account.username, account.account, account.platform_user_id,
              account.profile_text, account.country_region, account.region_source,
              account.region_confidence, account.platform, account.gender, account.age,
              account.followers_count, account.posts_count, account.last_posted_at,
              account.profile_url, account.data_source, account.collected_at, account.notes
       FROM collected_account AS account
       JOIN task_run AS run ON run.id = account.task_run_id
       WHERE run.task_id = ?1 AND account.output_included = 1
         AND run.id = (
           SELECT latest.id FROM task_run AS latest
           WHERE latest.task_id = ?1 AND latest.status IN ('success', 'partial_success')
           ORDER BY COALESCE(latest.ended_at, latest.started_at) DESC, latest.id DESC LIMIT 1
         )
       ORDER BY account.created_at, account.id",
    )
    .map_err(super::database_error)?;
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
      })
    })
    .map_err(super::database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(super::database_error)
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
