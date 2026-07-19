use std::fmt::Write as FmtWrite;
use std::path::Path;

use rusqlite::{params, Connection};

use super::{
  database_error, export_error, open_workspace_connection, write_new_export_file, ReportView,
};
use crate::domain::AppResult;

const ACCOUNTS_PER_PAGE: usize = 10;

#[derive(Debug)]
struct PdfAccount {
  username: Option<String>,
  account: Option<String>,
  profile_text: Option<String>,
  country_region: Option<String>,
  platform: String,
  followers_count: Option<i64>,
  posts_count: Option<i64>,
  data_source: String,
  collected_at: String,
}

struct PdfLine {
  x: i32,
  y: i32,
  size: i32,
  text: String,
}

pub(super) fn write_pdf(root_path: &Path, path: &Path, report: &ReportView) -> AppResult<()> {
  let connection = open_workspace_connection(root_path)?;
  let accounts = load_accounts(&connection, &report.task_id)?;
  let pages = layout_pages(report, &accounts);
  let bytes = build_pdf(&pages)?;
  write_new_export_file(path, &bytes)
}

fn load_accounts(connection: &Connection, task_id: &str) -> AppResult<Vec<PdfAccount>> {
  let mut statement = connection
    .prepare(
      "SELECT account.username, account.account, account.profile_text,
              account.country_region, account.platform, account.followers_count,
              account.posts_count, account.data_source, account.collected_at
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
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_id], |row| {
      Ok(PdfAccount {
        username: row.get(0)?,
        account: row.get(1)?,
        profile_text: row.get(2)?,
        country_region: row.get(3)?,
        platform: row.get(4)?,
        followers_count: row.get(5)?,
        posts_count: row.get(6)?,
        data_source: row.get(7)?,
        collected_at: row.get(8)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn layout_pages(report: &ReportView, accounts: &[PdfAccount]) -> Vec<Vec<PdfLine>> {
  let page_count = accounts.len().div_ceil(ACCOUNTS_PER_PAGE).max(1);
  let generated_at = report
    .report_model_json
    .get("generated_at")
    .and_then(|value| value.as_str())
    .unwrap_or(&report.created_at);
  let mut pages = Vec::with_capacity(page_count);

  for page_index in 0..page_count {
    let mut lines = vec![
      PdfLine {
        x: 52,
        y: 790,
        size: 18,
        text: compact_text(&report.title, 34),
      },
      PdfLine {
        x: 475,
        y: 792,
        size: 9,
        text: format!("第 {} / {} 页", page_index + 1, page_count),
      },
      PdfLine {
        x: 52,
        y: 758,
        size: 9,
        text: format!(
          "结果记录：{} 条  口径：最新一次成功或部分成功运行的已入库结果",
          accounts.len()
        ),
      },
      PdfLine {
        x: 52,
        y: 32,
        size: 8,
        text: format!(
          "Sortlytic 任务结果报告  生成时间：{}",
          compact_text(generated_at, 30)
        ),
      },
    ];
    let start = page_index * ACCOUNTS_PER_PAGE;
    let end = (start + ACCOUNTS_PER_PAGE).min(accounts.len());

    if start == end {
      lines.push(PdfLine {
        x: 52,
        y: 700,
        size: 11,
        text: "当前任务没有可写入报告的结果记录。".to_string(),
      });
    }

    for (slot, account) in accounts[start..end].iter().enumerate() {
      let y = 710 - slot as i32 * 64;
      let username = display_value(account.username.as_deref());
      let account_name = account
        .account
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("  @{}", value.trim().trim_start_matches('@')))
        .unwrap_or_default();
      lines.extend([
        PdfLine {
          x: 52,
          y,
          size: 11,
          text: format!(
            "{}. {}{}",
            start + slot + 1,
            compact_text(&username, 28),
            compact_text(&account_name, 30)
          ),
        },
        PdfLine {
          x: 64,
          y: y - 15,
          size: 9,
          text: format!(
            "平台：{}  国家/地区：{}  粉丝数：{}  作品数：{}",
            platform_label(&account.platform),
            display_value(account.country_region.as_deref()),
            display_count(account.followers_count),
            display_count(account.posts_count)
          ),
        },
        PdfLine {
          x: 64,
          y: y - 30,
          size: 9,
          text: format!(
            "公开简介：{}",
            compact_text(&display_value(account.profile_text.as_deref()), 45)
          ),
        },
        PdfLine {
          x: 64,
          y: y - 45,
          size: 8,
          text: format!(
            "来源：{}  收集时间：{}",
            compact_text(&account.data_source, 24),
            compact_text(&account.collected_at, 28)
          ),
        },
      ]);
    }
    pages.push(lines);
  }
  pages
}

fn build_pdf(pages: &[Vec<PdfLine>]) -> AppResult<Vec<u8>> {
  let page_ids = (0..pages.len())
    .map(|index| format!("{} 0 R", 5 + index * 2))
    .collect::<Vec<_>>()
    .join(" ");
  let mut objects = vec![
    "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
    format!(
      "<< /Type /Pages /Kids [{}] /Count {} >>",
      page_ids,
      pages.len()
    ),
    "<< /Type /Font /Subtype /Type0 /BaseFont /STSong-Light /Encoding /UniGB-UCS2-H /DescendantFonts [4 0 R] >>".to_string(),
    "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /STSong-Light /CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 4 >> /DW 1000 >>".to_string(),
  ];

  for (index, lines) in pages.iter().enumerate() {
    let content_id = 6 + index * 2;
    let mut content = String::from("0.85 G 52 744 m 543 744 l S\n");
    for line in lines {
      writeln!(
        &mut content,
        "BT /F1 {} Tf 1 0 0 1 {} {} Tm <{}> Tj ET",
        line.size,
        line.x,
        line.y,
        pdf_hex(&line.text)
      )
      .map_err(export_error)?;
    }
    objects.push(format!(
      "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
    ));
    objects.push(format!(
      "<< /Length {} >>\nstream\n{}endstream",
      content.len(),
      content
    ));
  }

  write_pdf_objects(&objects)
}

fn write_pdf_objects(objects: &[String]) -> AppResult<Vec<u8>> {
  let mut pdf = String::from("%PDF-1.4\n");
  let mut offsets = Vec::with_capacity(objects.len() + 1);
  offsets.push(0);
  for (index, object) in objects.iter().enumerate() {
    offsets.push(pdf.len());
    writeln!(&mut pdf, "{} 0 obj\n{}\nendobj", index + 1, object).map_err(export_error)?;
  }
  let xref_offset = pdf.len();
  writeln!(&mut pdf, "xref\n0 {}", offsets.len()).map_err(export_error)?;
  writeln!(&mut pdf, "0000000000 65535 f ").map_err(export_error)?;
  for offset in offsets.iter().skip(1) {
    writeln!(&mut pdf, "{offset:010} 00000 n ").map_err(export_error)?;
  }
  write!(
    &mut pdf,
    "trailer << /Root 1 0 R /Size {} >>\nstartxref\n{}\n%%EOF\n",
    offsets.len(),
    xref_offset
  )
  .map_err(export_error)?;
  Ok(pdf.into_bytes())
}

fn pdf_hex(value: &str) -> String {
  value
    .chars()
    .flat_map(|character| {
      if character as u32 <= 0xffff {
        vec![character as u16]
      } else {
        vec!['?' as u16]
      }
    })
    .map(|unit| format!("{unit:04X}"))
    .collect()
}

fn display_value(value: Option<&str>) -> String {
  value
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .unwrap_or("未采集到")
    .to_string()
}

fn display_count(value: Option<i64>) -> String {
  value
    .map(|value| value.to_string())
    .unwrap_or_else(|| "未采集到".to_string())
}

fn compact_text(value: &str, max_chars: usize) -> String {
  let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
  let mut characters = normalized.chars();
  let compact = characters.by_ref().take(max_chars).collect::<String>();
  if characters.next().is_some() {
    format!("{compact}…")
  } else {
    compact
  }
}

fn platform_label(platform: &str) -> &str {
  match platform {
    "tiktok" => "TikTok",
    "douyin" => "抖音",
    "xiaohongshu" => "小红书",
    _ => platform,
  }
}
