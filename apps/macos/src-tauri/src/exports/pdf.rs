use std::path::Path;

use printpdf::{
  Color, Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt, Rgb,
  TextItem,
};
use serde_json::Value;

use super::{export_error, write_new_export_file, ReportView};
use crate::domain::AppResult;

const REPORT_FONT: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-VF.ttf");

#[derive(Clone, Copy)]
enum TextTone {
  Primary,
  Accent,
  Muted,
}

struct FlowLine {
  text: String,
  size: f32,
  tone: TextTone,
  indent: f32,
  before: f32,
  after: f32,
  wrap_units: usize,
}

struct PlacedLine {
  text: String,
  x: f32,
  y: f32,
  size: f32,
  tone: TextTone,
}

pub(super) fn write_pdf(_root_path: &Path, path: &Path, report: &ReportView) -> AppResult<()> {
  let mut font_warnings = Vec::new();
  let font = ParsedFont::from_bytes(REPORT_FONT, 0, &mut font_warnings)
    .ok_or_else(|| export_error("PDF 中文字体解析失败"))?;
  let mut document = PdfDocument::new(&report.title);
  let font_id = document.add_font(&font);
  let flow = report_flow(report);
  let page_lines = paginate(&report.title, &flow);
  let pages = page_lines
    .into_iter()
    .map(|lines| PdfPage::new(Mm(210.0), Mm(297.0), page_operations(lines, &font_id)))
    .collect::<Vec<_>>();
  let mut save_warnings = Vec::new();
  let bytes = document.with_pages(pages).save(
    &PdfSaveOptions {
      subset_fonts: true,
      ..Default::default()
    },
    &mut save_warnings,
  );
  if bytes.is_empty() {
    return Err(export_error("PDF 数据分析报告生成了空文件"));
  }
  write_new_export_file(path, &bytes)
}

fn report_flow(report: &ReportView) -> Vec<FlowLine> {
  let analysis = &report.report_model_json["analysis"];
  let sample_size = analysis["sample_size"].as_i64().unwrap_or(0);
  let generated_at = report.report_model_json["generated_at"]
    .as_str()
    .unwrap_or(&report.created_at);
  let run_status = analysis["run_status"].as_str().unwrap_or("不可用");
  let mut lines = vec![
    flow(
      &report.title,
      20.0,
      TextTone::Primary,
      0.0,
      0.0,
      2.0,
      48,
    ),
    flow(
      &format!(
        "生成时间：{}  运行状态：{}",
        compact_time(generated_at),
        run_status_label(run_status)
      ),
      9.0,
      TextTone::Muted,
      0.0,
      0.0,
      7.0,
      92,
    ),
    heading("报告摘要"),
    flow(
      &format!(
        "本报告基于任务最新一次成功或部分成功运行中明确进入输出集合的 {sample_size} 条落库记录生成。原始账号明细不在 PDF 中重复导出；如需逐行数据，请使用 Excel 工作簿。"
      ),
      10.0,
      TextTone::Primary,
      0.0,
      1.0,
      4.0,
      92,
    ),
    heading("核心结论"),
  ];
  for finding in analysis["findings"].as_array().into_iter().flatten() {
    if let Some(text) = finding["text"].as_str() {
      lines.push(bullet(text));
    }
  }
  lines.extend([
    heading("样本分布"),
    body(&format!(
      "平台分布：{}",
      format_breakdown(&analysis["platform_breakdown"], sample_size, true)
    )),
    body(&format!(
      "国家/地区分布：{}",
      format_breakdown(&analysis["region_breakdown"], sample_size, false)
    )),
    body(&format!(
      "数据来源：{}",
      format_breakdown(&analysis["data_source_breakdown"], sample_size, false)
    )),
    heading("字段完整度"),
    body(&format_completeness(
      &analysis["field_completeness"],
      sample_size,
    )),
    heading("数值概况"),
    body(&format!(
      "粉丝数：{}",
      format_numeric_summary(&analysis["followers_summary"])
    )),
    body(&format!(
      "作品数：{}",
      format_numeric_summary(&analysis["posts_summary"])
    )),
    heading("证据与统计口径"),
    body(&format!(
      "任务运行 ID：{}；开始时间：{}；结束时间：{}。",
      analysis["task_run_id"].as_str().unwrap_or("不可用"),
      compact_time(analysis["started_at"].as_str().unwrap_or("不可用")),
      compact_time(analysis["ended_at"].as_str().unwrap_or("不可用"))
    )),
    body("所有指标均由本地 SQLite 中 output_included = 1 的真实落库记录确定性计算；缺失字段不按 0 处理，也不根据姓名、头像或简介推断年龄、性别和地区。"),
    heading("限制说明"),
  ]);
  for limitation in analysis["limitations"].as_array().into_iter().flatten() {
    if let Some(text) = limitation.as_str() {
      lines.push(bullet(text));
    }
  }
  lines
}

fn paginate(title: &str, flow: &[FlowLine]) -> Vec<Vec<PlacedLine>> {
  let mut pages = vec![Vec::new()];
  let mut y = 278.0_f32;
  for line in flow {
    y -= line.before;
    for wrapped in wrap_text(&line.text, line.wrap_units) {
      let height = (line.size * 0.52).max(4.2);
      if y - height < 24.0 {
        let next_page = pages.len() + 1;
        pages.push(vec![PlacedLine {
          text: format!("{}（续）", compact_text(title, 34)),
          x: 20.0,
          y: 282.0,
          size: 11.0,
          tone: TextTone::Accent,
        }]);
        y = 270.0;
        pages
          .last_mut()
          .expect("a continuation page should exist")
          .push(PlacedLine {
            text: format!("第 {next_page} 页"),
            x: 170.0,
            y: 282.0,
            size: 8.0,
            tone: TextTone::Muted,
          });
      }
      pages
        .last_mut()
        .expect("at least one report page should exist")
        .push(PlacedLine {
          text: wrapped,
          x: 20.0 + line.indent,
          y,
          size: line.size,
          tone: line.tone,
        });
      y -= height;
    }
    y -= line.after;
  }
  let page_count = pages.len();
  for (index, page) in pages.iter_mut().enumerate() {
    page.push(PlacedLine {
      text: format!(
        "Sortlytic 数据分析报告  第 {} / {} 页",
        index + 1,
        page_count
      ),
      x: 20.0,
      y: 12.0,
      size: 8.0,
      tone: TextTone::Muted,
    });
  }
  pages
}

fn page_operations(lines: Vec<PlacedLine>, font_id: &printpdf::FontId) -> Vec<Op> {
  let mut operations = Vec::with_capacity(lines.len() * 6);
  for line in lines {
    operations.extend([
      Op::StartTextSection,
      Op::SetTextCursor {
        pos: Point::new(Mm(line.x), Mm(line.y)),
      },
      Op::SetFont {
        font: PdfFontHandle::External(font_id.clone()),
        size: Pt(line.size),
      },
      Op::SetFillColor {
        col: tone_color(line.tone),
      },
      Op::ShowText {
        items: vec![TextItem::Text(line.text)],
      },
      Op::EndTextSection,
    ]);
  }
  operations
}

fn flow(
  text: &str,
  size: f32,
  tone: TextTone,
  indent: f32,
  before: f32,
  after: f32,
  wrap_units: usize,
) -> FlowLine {
  FlowLine {
    text: text.to_string(),
    size,
    tone,
    indent,
    before,
    after,
    wrap_units,
  }
}

fn heading(text: &str) -> FlowLine {
  flow(text, 13.0, TextTone::Accent, 0.0, 5.0, 2.0, 60)
}

fn body(text: &str) -> FlowLine {
  flow(text, 10.0, TextTone::Primary, 0.0, 0.8, 1.5, 92)
}

fn bullet(text: &str) -> FlowLine {
  flow(
    &format!("· {text}"),
    10.0,
    TextTone::Primary,
    3.0,
    0.8,
    1.5,
    88,
  )
}

fn tone_color(tone: TextTone) -> Color {
  let (red, green, blue) = match tone {
    TextTone::Primary => (0.10, 0.13, 0.16),
    TextTone::Accent => (0.08, 0.45, 0.52),
    TextTone::Muted => (0.40, 0.43, 0.46),
  };
  Color::Rgb(Rgb {
    r: red,
    g: green,
    b: blue,
    icc_profile: None,
  })
}

fn format_breakdown(value: &Value, total: i64, platform: bool) -> String {
  let Some(values) = value.as_object() else {
    return "不可用".to_string();
  };
  if values.is_empty() {
    return "不可用".to_string();
  }
  let mut counts = values
    .iter()
    .map(|(label, count)| (label.as_str(), count.as_i64().unwrap_or(0)))
    .collect::<Vec<_>>();
  counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
  counts
    .into_iter()
    .map(|(label, count)| {
      let label = if platform {
        platform_label(label)
      } else {
        label
      };
      format!("{label} {count} 条（{:.1}%）", percentage(count, total))
    })
    .collect::<Vec<_>>()
    .join("；")
}

fn format_completeness(value: &Value, total: i64) -> String {
  [
    ("国家/地区", "country_region"),
    ("性别", "gender"),
    ("年龄", "age"),
    ("粉丝数", "followers_count"),
    ("作品数", "posts_count"),
  ]
  .into_iter()
  .map(|(label, key)| {
    let count = value[key].as_i64().unwrap_or(0);
    format!(
      "{label} {count}/{total}（{:.1}%）",
      percentage(count, total)
    )
  })
  .collect::<Vec<_>>()
  .join("；")
}

fn format_numeric_summary(value: &Value) -> String {
  let count = value["available_count"].as_i64().unwrap_or(0);
  if count == 0 {
    return "未采集到可统计数值".to_string();
  }
  format!(
    "可用 {count} 条，最小值 {}，平均值 {}，最大值 {}",
    display_number(&value["minimum"]),
    display_number(&value["average"]),
    display_number(&value["maximum"])
  )
}

fn display_number(value: &Value) -> String {
  if let Some(value) = value.as_i64() {
    value.to_string()
  } else if let Some(value) = value.as_f64() {
    format!("{value:.1}")
  } else {
    "不可用".to_string()
  }
}

fn wrap_text(value: &str, max_units: usize) -> Vec<String> {
  let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
  if normalized.is_empty() {
    return vec![String::new()];
  }
  let mut lines = Vec::new();
  let mut current = String::new();
  let mut units = 0;
  for character in normalized.chars() {
    let character_units = if character.is_ascii() { 1 } else { 2 };
    if units + character_units > max_units && !current.is_empty() {
      lines.push(current);
      current = String::new();
      units = 0;
    }
    current.push(character);
    units += character_units;
  }
  if !current.is_empty() {
    lines.push(current);
  }
  lines
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

fn compact_time(value: &str) -> String {
  value.chars().take(19).collect()
}

fn percentage(count: i64, total: i64) -> f64 {
  if total == 0 {
    0.0
  } else {
    count as f64 * 100.0 / total as f64
  }
}

fn run_status_label(status: &str) -> &str {
  match status {
    "success" => "成功",
    "partial_success" => "部分成功",
    _ => status,
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
