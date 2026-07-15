use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use rust_xlsxwriter::Workbook;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportView {
  pub id: String,
  pub task_id: String,
  pub report_type: String,
  pub title: String,
  pub report_model_json: Value,
  pub status: String,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportJobView {
  pub id: String,
  pub report_id: String,
  pub export_type: String,
  pub status: String,
  pub file_path: Option<PathBuf>,
  pub file_hash: Option<String>,
  pub file_size: Option<i64>,
  pub error_code: Option<String>,
  pub error_message: Option<String>,
  pub created_at: String,
  pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportIntegrityResult {
  pub valid: bool,
  pub errors: Vec<String>,
}

pub fn build_report_model(
  root_path: impl AsRef<Path>,
  task_id: &str,
  report_type: &str,
) -> AppResult<ReportView> {
  let connection = open_workspace_connection(&root_path)?;
  let task = task_summary(&connection, task_id)?;
  let ai_runs = ai_run_summaries(&connection, task_id)?;
  let logs = task_log_summaries(&connection, task_id)?;
  let now = Utc::now().to_rfc3339();
  let report_id = Uuid::new_v4().to_string();
  let report_type = normalize_export_type(report_type, &["summary", "analysis"])?;
  let title = format!("{} 报告", task["name"].as_str().unwrap_or("任务"));
  let report_model = serde_json::json!({
    "title": title,
    "generated_at": now,
    "task": task,
    "ai_runs": ai_runs,
    "logs": logs,
    "data_source_statement": "仅包含本地工作区内已记录的任务、AI 运行和日志摘要。",
    "ai_disclaimer": "AI 生成内容可能存在遗漏、误判或表达偏差，使用前应人工确认。"
  });

  connection
    .execute(
      "INSERT INTO report (
        id, task_id, report_type, title, report_model_json, status, created_at, updated_at
      ) VALUES (?1, ?2, ?3, ?4, ?5, 'ready', ?6, ?7)",
      params![
        report_id,
        task_id,
        report_type,
        title,
        report_model.to_string(),
        now,
        now
      ],
    )
    .map_err(database_error)?;

  let report = get_report(&connection, &report_id)?;
  write_report_snapshot(root_path, &report)?;
  Ok(report)
}

pub fn validate_export_integrity(
  root_path: impl AsRef<Path>,
  report_id: &str,
  export_type: &str,
) -> AppResult<ExportIntegrityResult> {
  let connection = open_workspace_connection(root_path)?;
  let export_type = normalize_export_type(export_type, &["xlsx", "pdf"])?;
  let report = get_report(&connection, report_id)?;
  let serialized = report.report_model_json.to_string();
  let mut errors = Vec::new();

  if report.status != "ready" {
    errors.push("报告模型还未 ready".to_string());
  }
  if serialized.to_ascii_lowercase().contains("authorization")
    || serialized.to_ascii_lowercase().contains("api_key")
    || serialized.contains("sk-")
  {
    errors.push("报告模型疑似包含敏感密钥信息".to_string());
  }
  if export_type == "pdf" && report.title.trim().is_empty() {
    errors.push("PDF 报告缺少标题".to_string());
  }

  Ok(ExportIntegrityResult {
    valid: errors.is_empty(),
    errors,
  })
}

pub fn create_export_job(
  root_path: impl AsRef<Path>,
  report_id: &str,
  export_type: &str,
  target_path: Option<String>,
) -> AppResult<ExportJobView> {
  let root_path = root_path.as_ref();
  let connection = open_workspace_connection(root_path)?;
  let export_type = normalize_export_type(export_type, &["xlsx", "pdf"])?;
  let integrity = validate_export_integrity(root_path, report_id, &export_type)?;

  if !integrity.valid {
    return Err(export_error(format!(
      "导出完整性检查失败：{}",
      integrity.errors.join("；")
    )));
  }

  let report = get_report(&connection, report_id)?;
  let job_id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  let file_path = resolve_export_path(root_path, report_id, &job_id, &export_type, target_path)?;

  connection
    .execute(
      "INSERT INTO export_job (id, report_id, export_type, status, file_path, created_at)
       VALUES (?1, ?2, ?3, 'running', ?4, ?5)",
      params![
        job_id,
        report_id,
        export_type,
        file_path.to_string_lossy(),
        now
      ],
    )
    .map_err(database_error)?;

  let write_result = if export_type == "xlsx" {
    write_excel(&file_path, &report)
  } else {
    write_pdf(&file_path, &report)
  };

  if let Err(error) = write_result {
    connection
      .execute(
        "UPDATE export_job SET status = 'failed', error_code = 'EXPORT_WRITE_ERROR',
         error_message = ?1, completed_at = ?2 WHERE id = ?3",
        params![error.message, Utc::now().to_rfc3339(), job_id],
      )
      .map_err(database_error)?;
    return Err(error);
  }

  let file_size = fs::metadata(&file_path)
    .map_err(export_error)?
    .len()
    .try_into()
    .unwrap_or(i64::MAX);
  let file_hash = hash_file(&file_path)?;
  let completed_at = Utc::now().to_rfc3339();
  connection
    .execute(
      "UPDATE export_job
       SET status = 'success', file_hash = ?1, file_size = ?2, completed_at = ?3
       WHERE id = ?4",
      params![file_hash, file_size, completed_at, job_id],
    )
    .map_err(database_error)?;

  get_export_job(root_path, &job_id)
}

pub fn get_export_job(
  root_path: impl AsRef<Path>,
  export_job_id: &str,
) -> AppResult<ExportJobView> {
  let connection = open_workspace_connection(root_path)?;
  connection
    .query_row(
      "SELECT id, report_id, export_type, status, file_path, file_hash, file_size,
              error_code, error_message, created_at, completed_at
       FROM export_job
       WHERE id = ?1",
      params![export_job_id],
      map_export_job,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| export_error("导出任务不存在"))
}

pub fn list_export_jobs(
  root_path: impl AsRef<Path>,
  report_id: Option<String>,
) -> AppResult<Vec<ExportJobView>> {
  let connection = open_workspace_connection(root_path)?;

  if let Some(report_id) = report_id {
    let mut statement = connection
      .prepare(
        "SELECT id, report_id, export_type, status, file_path, file_hash, file_size,
                error_code, error_message, created_at, completed_at
         FROM export_job
         WHERE report_id = ?1
         ORDER BY created_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map(params![report_id], map_export_job)
      .map_err(database_error)?;
    collect_rows(rows)
  } else {
    let mut statement = connection
      .prepare(
        "SELECT id, report_id, export_type, status, file_path, file_hash, file_size,
                error_code, error_message, created_at, completed_at
         FROM export_job
         ORDER BY created_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map([], map_export_job)
      .map_err(database_error)?;
    collect_rows(rows)
  }
}

pub fn get_report(connection: &Connection, report_id: &str) -> AppResult<ReportView> {
  connection
    .query_row(
      "SELECT id, task_id, report_type, title, report_model_json, status, created_at, updated_at
       FROM report
       WHERE id = ?1",
      params![report_id],
      map_report,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| export_error("报告不存在"))
}

fn write_excel(path: &Path, report: &ReportView) -> AppResult<()> {
  let mut workbook = Workbook::new();
  write_key_value_sheet(
    workbook
      .add_worksheet()
      .set_name("任务概览")
      .map_err(export_error)?,
    &[
      ("报告标题", report.title.as_str()),
      ("报告类型", report.report_type.as_str()),
      ("生成时间", report.created_at.as_str()),
      (
        "数据来源",
        report.report_model_json["data_source_statement"]
          .as_str()
          .unwrap_or(""),
      ),
      (
        "AI 免责声明",
        report.report_model_json["ai_disclaimer"]
          .as_str()
          .unwrap_or(""),
      ),
    ],
  )?;
  write_json_sheet(
    workbook
      .add_worksheet()
      .set_name("AI结构化结果")
      .map_err(export_error)?,
    &report.report_model_json["ai_runs"],
  )?;
  write_json_sheet(
    workbook
      .add_worksheet()
      .set_name("运行日志")
      .map_err(export_error)?,
    &report.report_model_json["logs"],
  )?;
  write_key_value_sheet(
    workbook
      .add_worksheet()
      .set_name("成本明细")
      .map_err(export_error)?,
    &[(
      "成本说明",
      "MVP 当前记录本地估算成本，外部 API 实际成本由后续适配层写入。",
    )],
  )?;

  let bytes = workbook.save_to_buffer().map_err(export_error)?;
  write_new_export_file(path, &bytes)
}

fn write_key_value_sheet(
  worksheet: &mut rust_xlsxwriter::Worksheet,
  rows: &[(&str, &str)],
) -> AppResult<()> {
  worksheet.write(0, 0, "字段").map_err(export_error)?;
  worksheet.write(0, 1, "值").map_err(export_error)?;
  worksheet.set_column_width(0, 20).map_err(export_error)?;
  worksheet.set_column_width(1, 80).map_err(export_error)?;

  for (index, (key, value)) in rows.iter().enumerate() {
    let row = (index + 1) as u32;
    worksheet.write(row, 0, *key).map_err(export_error)?;
    worksheet.write(row, 1, *value).map_err(export_error)?;
  }

  Ok(())
}

fn write_json_sheet(worksheet: &mut rust_xlsxwriter::Worksheet, value: &Value) -> AppResult<()> {
  worksheet.write(0, 0, "序号").map_err(export_error)?;
  worksheet.write(0, 1, "JSON").map_err(export_error)?;
  worksheet.set_column_width(0, 10).map_err(export_error)?;
  worksheet.set_column_width(1, 100).map_err(export_error)?;

  if let Some(items) = value.as_array() {
    for (index, item) in items.iter().enumerate() {
      let row = (index + 1) as u32;
      worksheet
        .write(row, 0, (index + 1) as i64)
        .map_err(export_error)?;
      worksheet
        .write(row, 1, item.to_string())
        .map_err(export_error)?;
    }
  } else {
    worksheet.write(1, 0, 1).map_err(export_error)?;
    worksheet
      .write(1, 1, value.to_string())
      .map_err(export_error)?;
  }

  Ok(())
}

fn write_pdf(path: &Path, report: &ReportView) -> AppResult<()> {
  let title = pdf_escape(&report.title);
  let body = pdf_escape("Sortlytic report. See XLSX export for full structured data.");
  let content = format!("BT /F1 18 Tf 72 740 Td ({title}) Tj /F1 11 Tf 0 -32 Td ({body}) Tj ET");
  let objects = [
    "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    format!(
      "<< /Length {} >>\nstream\n{}\nendstream",
      content.len(),
      content
    ),
  ];
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
  write_new_export_file(path, pdf.as_bytes())
}

fn write_report_snapshot(root_path: impl AsRef<Path>, report: &ReportView) -> AppResult<()> {
  let reports_dir = root_path.as_ref().join("reports");
  ensure_real_directory(&reports_dir)?;
  let report_dir = reports_dir.join(&report.id);
  let mut builder = fs::DirBuilder::new();
  #[cfg(unix)]
  builder.mode(0o700);
  builder.create(&report_dir).map_err(export_error)?;
  let result = (|| -> AppResult<()> {
    #[cfg(unix)]
    fs::set_permissions(&report_dir, fs::Permissions::from_mode(0o700)).map_err(export_error)?;
    #[cfg(unix)]
    if fs::symlink_metadata(&report_dir)
      .map_err(export_error)?
      .permissions()
      .mode()
      & 0o7777
      != 0o700
    {
      return Err(export_error("导出文件系统无法保证 0700 私有目录权限"));
    }
    write_new_export_file(
      &report_dir.join("report_model.json"),
      &serde_json::to_vec_pretty(&report.report_model_json).map_err(export_error)?,
    )
  })();
  if result.is_err() {
    fs::remove_dir(&report_dir).ok();
  }
  result
}

fn resolve_export_path(
  root_path: &Path,
  report_id: &str,
  job_id: &str,
  export_type: &str,
  target_path: Option<String>,
) -> AppResult<PathBuf> {
  if let Some(target_path) = target_path {
    let target_path = PathBuf::from(target_path);
    validate_new_export_path(&target_path, export_type, true)?;
    return Ok(target_path);
  }

  let directory = if export_type == "xlsx" {
    root_path.join("exports/excel")
  } else {
    root_path.join("exports/pdf")
  };
  fs::create_dir_all(&directory).map_err(export_error)?;
  ensure_real_directory(&directory)?;
  let path = directory.join(format!("{report_id}-{job_id}.{export_type}"));
  validate_new_export_path(&path, export_type, false)?;
  Ok(path)
}

fn validate_new_export_path(
  path: &Path,
  export_type: &str,
  require_absolute: bool,
) -> AppResult<()> {
  if require_absolute && !path.is_absolute() {
    return Err(export_error("自定义导出路径必须是绝对路径"));
  }
  let extension_matches = path
    .extension()
    .and_then(|value| value.to_str())
    .is_some_and(|value| value.eq_ignore_ascii_case(export_type));
  if !extension_matches {
    return Err(export_error("导出目标扩展名与导出类型不一致"));
  }
  let parent = path
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .ok_or_else(|| export_error("导出目标缺少父目录"))?;
  ensure_real_directory(parent)?;
  match fs::symlink_metadata(path) {
    Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
    Ok(_) => Err(export_error("导出目标已存在，拒绝覆盖")),
    Err(error) => Err(export_error(error)),
  }
}

fn ensure_real_directory(path: &Path) -> AppResult<()> {
  let metadata = fs::symlink_metadata(path).map_err(export_error)?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    return Err(export_error("导出父目录必须是非符号链接目录"));
  }
  Ok(())
}

fn write_new_export_file(path: &Path, bytes: &[u8]) -> AppResult<()> {
  let mut options = OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)]
  options.mode(0o600);
  let mut file = options.open(path).map_err(export_error)?;
  let result = (|| -> AppResult<()> {
    #[cfg(unix)]
    {
      file
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(export_error)?;
      if file.metadata().map_err(export_error)?.permissions().mode() & 0o7777 != 0o600 {
        return Err(export_error("导出文件系统无法保证 0600 私有权限"));
      }
    }
    file.write_all(bytes).map_err(export_error)?;
    file.sync_all().map_err(export_error)?;
    let parent = path
      .parent()
      .ok_or_else(|| export_error("导出目标缺少父目录"))?;
    fs::File::open(parent)
      .and_then(|directory| directory.sync_all())
      .map_err(export_error)
  })();
  if result.is_err() {
    drop(file);
    fs::remove_file(path).ok();
  }
  result
}

fn task_summary(connection: &Connection, task_id: &str) -> AppResult<Value> {
  connection
    .query_row(
      "SELECT id, name, source_type, status, platforms_json, data_types_json, cost_estimate_json
       FROM collection_task WHERE id = ?1",
      params![task_id],
      |row| {
        Ok(serde_json::json!({
          "id": row.get::<_, String>(0)?,
          "name": row.get::<_, String>(1)?,
          "source_type": row.get::<_, String>(2)?,
          "status": row.get::<_, String>(3)?,
          "platforms": string_to_json(row.get(4)?),
          "data_types": string_to_json(row.get(5)?),
          "cost_estimate": string_to_json(row.get(6)?)
        }))
      },
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| export_error("任务不存在，无法生成报告"))
}

fn ai_run_summaries(connection: &Connection, task_id: &str) -> AppResult<Value> {
  let mut statement = connection
    .prepare(
      "SELECT id, run_type, validation_status, output_json, created_at
       FROM ai_run WHERE task_id = ?1 ORDER BY created_at DESC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_id], |row| {
      Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "run_type": row.get::<_, String>(1)?,
        "validation_status": row.get::<_, String>(2)?,
        "output": row.get::<_, Option<String>>(3)?.map(string_to_json),
        "created_at": row.get::<_, String>(4)?
      }))
    })
    .map_err(database_error)?;

  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map(Value::Array)
    .map_err(database_error)
}

fn task_log_summaries(connection: &Connection, task_id: &str) -> AppResult<Value> {
  let mut statement = connection
    .prepare(
      "SELECT tl.stage, tl.level, tl.message, tl.created_at
       FROM task_log tl
       JOIN task_run tr ON tr.id = tl.task_run_id
       WHERE tr.task_id = ?1
       ORDER BY tl.created_at ASC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_id], |row| {
      Ok(serde_json::json!({
        "stage": row.get::<_, String>(0)?,
        "level": row.get::<_, String>(1)?,
        "message": row.get::<_, String>(2)?,
        "created_at": row.get::<_, String>(3)?
      }))
    })
    .map_err(database_error)?;

  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map(Value::Array)
    .map_err(database_error)
}

fn open_workspace_connection(root_path: impl AsRef<Path>) -> AppResult<Connection> {
  open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))
}

fn map_report(row: &Row<'_>) -> rusqlite::Result<ReportView> {
  Ok(ReportView {
    id: row.get(0)?,
    task_id: row.get(1)?,
    report_type: row.get(2)?,
    title: row.get(3)?,
    report_model_json: string_to_json(row.get(4)?),
    status: row.get(5)?,
    created_at: row.get(6)?,
    updated_at: row.get(7)?,
  })
}

fn map_export_job(row: &Row<'_>) -> rusqlite::Result<ExportJobView> {
  Ok(ExportJobView {
    id: row.get(0)?,
    report_id: row.get(1)?,
    export_type: row.get(2)?,
    status: row.get(3)?,
    file_path: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
    file_hash: row.get(5)?,
    file_size: row.get(6)?,
    error_code: row.get(7)?,
    error_message: row.get(8)?,
    created_at: row.get(9)?,
    completed_at: row.get(10)?,
  })
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> AppResult<Vec<T>> {
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn hash_file(path: &Path) -> AppResult<String> {
  let bytes = fs::read(path).map_err(export_error)?;
  let mut hasher = Sha256::new();
  hasher.update(bytes);
  Ok(format!("{:x}", hasher.finalize()))
}

fn normalize_export_type(value: &str, allowed: &[&str]) -> AppResult<String> {
  let value = value.trim();
  if allowed.contains(&value) {
    Ok(value.to_string())
  } else {
    Err(export_error("导出或报告类型不受支持"))
  }
}

fn string_to_json(value: String) -> Value {
  serde_json::from_str(&value).unwrap_or_else(|_| serde_json::json!({}))
}

fn pdf_escape(value: &str) -> String {
  value
    .replace('\\', "\\\\")
    .replace('(', "\\(")
    .replace(')', "\\)")
}

fn export_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::ExportIntegrityError,
    error.to_string(),
    AppErrorStage::Export,
    false,
  )
}

fn database_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    error.to_string(),
    AppErrorStage::Database,
    false,
  )
}

#[cfg(test)]
#[path = "exports_tests.rs"]
mod tests;
