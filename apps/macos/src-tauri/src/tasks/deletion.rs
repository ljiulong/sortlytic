use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, Path, PathBuf};

use rusqlite::{params, Transaction, TransactionBehavior};

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArtifactKind {
  File,
  ReportDirectory,
}

struct ArtifactCandidate {
  kind: ArtifactKind,
  path: PathBuf,
}

struct MovedArtifact {
  original: PathBuf,
  staged: PathBuf,
}

struct TaskArtifactQuarantine {
  directory: Option<PathBuf>,
  moved: Vec<MovedArtifact>,
  restore_on_drop: bool,
}

impl TaskArtifactQuarantine {
  fn stage(root_path: &Path, transaction: &Transaction<'_>, task_id: &str) -> AppResult<Self> {
    let candidates = artifact_candidates(root_path, transaction, task_id)?;
    validate_candidates(&candidates)?;
    if candidates.is_empty() {
      return Ok(Self {
        directory: None,
        moved: Vec::new(),
        restore_on_drop: false,
      });
    }

    let temp_directory = root_path.join("temp");
    ensure_real_directory(&temp_directory)?;
    let quarantine_directory = temp_directory.join(format!("task-delete-{}", Uuid::new_v4()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder
      .create(&quarantine_directory)
      .map_err(cleanup_error)?;
    let mut quarantine = Self {
      directory: Some(quarantine_directory.clone()),
      moved: Vec::with_capacity(candidates.len()),
      restore_on_drop: true,
    };
    for (index, candidate) in candidates.into_iter().enumerate() {
      let staged = quarantine_directory.join(index.to_string());
      fs::rename(&candidate.path, &staged).map_err(cleanup_error)?;
      quarantine.moved.push(MovedArtifact {
        original: candidate.path,
        staged,
      });
    }
    Ok(quarantine)
  }

  fn purge_after_commit(mut self) -> AppResult<()> {
    self.restore_on_drop = false;
    let Some(directory) = self.directory.take() else {
      return Ok(());
    };
    fs::remove_dir_all(directory)
      .map_err(|error| cleanup_error(format!("任务已删除，但隔离文件清理失败：{error}")))
  }

  fn restore(&mut self) {
    for artifact in self.moved.iter().rev() {
      if artifact.staged.exists() && !artifact.original.exists() {
        let _ = fs::rename(&artifact.staged, &artifact.original);
      }
    }
    if let Some(directory) = &self.directory {
      let _ = fs::remove_dir(directory);
    }
  }
}

impl Drop for TaskArtifactQuarantine {
  fn drop(&mut self) {
    if self.restore_on_drop {
      self.restore();
    }
  }
}

pub fn delete_task(root_path: impl AsRef<Path>, task_id: &str) -> AppResult<()> {
  let root_path = root_path.as_ref();
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let task = get_task_by_id(&transaction, task_id)?;
  let active_run_count = transaction
    .query_row(
      "SELECT COUNT(*) FROM task_run
       WHERE task_id = ?1 AND status IN ('queued', 'running')",
      params![task_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;

  if matches!(task.status.as_str(), "queued" | "running") || active_run_count > 0 {
    return Err(task_error("排队或运行中的任务请先取消，再执行删除"));
  }

  let plan_count = transaction
    .query_row(
      "SELECT COUNT(*) FROM collection_plan WHERE task_id = ?1",
      params![task_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  let run_count = transaction
    .query_row(
      "SELECT COUNT(*) FROM task_run WHERE task_id = ?1",
      params![task_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  let quarantine = TaskArtifactQuarantine::stage(root_path, &transaction, task_id)?;

  transaction
    .execute("DELETE FROM task_run WHERE task_id = ?1", params![task_id])
    .map_err(database_error)?;
  transaction
    .execute(
      "DELETE FROM collection_plan WHERE task_id = ?1",
      params![task_id],
    )
    .map_err(database_error)?;
  let deleted = transaction
    .execute(
      "DELETE FROM collection_task WHERE id = ?1",
      params![task_id],
    )
    .map_err(database_error)?;
  if deleted != 1 {
    return Err(task_error("任务状态已变化，无法删除"));
  }

  write_task_audit_log(
    &transaction,
    "delete_task",
    Some(task_id),
    serde_json::json!({
      "status": task.status,
      "plan_count": plan_count,
      "run_count": run_count,
    }),
  )?;
  transaction.commit().map_err(database_error)?;
  quarantine.purge_after_commit()
}

fn artifact_candidates(
  root_path: &Path,
  transaction: &Transaction<'_>,
  task_id: &str,
) -> AppResult<Vec<ArtifactCandidate>> {
  let mut candidates = BTreeMap::<PathBuf, ArtifactKind>::new();
  for relative_path in query_strings(
    transaction,
    "SELECT raw_file_path FROM raw_record WHERE task_id = ?1",
    task_id,
  )? {
    let path = managed_raw_path(root_path, &relative_path)?;
    insert_candidate(&mut candidates, path, ArtifactKind::File)?;
  }
  for report_id in query_strings(
    transaction,
    "SELECT id FROM report WHERE task_id = ?1",
    task_id,
  )? {
    Uuid::parse_str(&report_id).map_err(|_| cleanup_error("报告快照 ID 格式无效"))?;
    insert_candidate(
      &mut candidates,
      root_path.join("reports").join(report_id),
      ArtifactKind::ReportDirectory,
    )?;
  }
  for stored_path in query_strings(
    transaction,
    "SELECT job.file_path
     FROM export_job AS job
     JOIN report ON report.id = job.report_id
     WHERE report.task_id = ?1 AND job.file_path IS NOT NULL",
    task_id,
  )? {
    if let Some(path) = managed_export_path(root_path, &stored_path)? {
      insert_candidate(&mut candidates, path, ArtifactKind::File)?;
    }
  }

  let mut existing = Vec::new();
  for (path, kind) in candidates {
    match fs::symlink_metadata(&path) {
      Ok(_) => existing.push(ArtifactCandidate { kind, path }),
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
      Err(error) => return Err(cleanup_error(error)),
    }
  }
  Ok(existing)
}

fn query_strings(
  transaction: &Transaction<'_>,
  sql: &str,
  task_id: &str,
) -> AppResult<Vec<String>> {
  let mut statement = transaction.prepare(sql).map_err(database_error)?;
  let rows = statement
    .query_map(params![task_id], |row| row.get::<_, String>(0))
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn insert_candidate(
  candidates: &mut BTreeMap<PathBuf, ArtifactKind>,
  path: PathBuf,
  kind: ArtifactKind,
) -> AppResult<()> {
  if candidates
    .insert(path, kind)
    .is_some_and(|existing| existing != kind)
  {
    return Err(cleanup_error("任务关联文件类型冲突"));
  }
  Ok(())
}

fn managed_raw_path(root_path: &Path, relative_path: &str) -> AppResult<PathBuf> {
  let path = Path::new(relative_path);
  let components = path.components().collect::<Vec<_>>();
  let valid = matches!(
    components.as_slice(),
    [Component::Normal(raw), Component::Normal(provider), Component::Normal(file)]
      if *raw == "raw" && *provider == "tikhub" && valid_hashed_json_name(file)
  );
  if !valid {
    return Err(cleanup_error("原始记录文件路径不在受管目录内"));
  }
  Ok(root_path.join(path))
}

fn valid_hashed_json_name(file: &std::ffi::OsStr) -> bool {
  let Some(file) = file.to_str() else {
    return false;
  };
  let Some(hash) = file.strip_suffix(".json") else {
    return false;
  };
  hash.len() == 64
    && hash
      .bytes()
      .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
}

fn managed_export_path(root_path: &Path, stored_path: &str) -> AppResult<Option<PathBuf>> {
  let path = PathBuf::from(stored_path);
  let exports_root = root_path.join("exports");
  let Ok(relative) = path.strip_prefix(&exports_root) else {
    return Ok(None);
  };
  let components = relative.components().collect::<Vec<_>>();
  let valid = matches!(
    components.as_slice(),
    [Component::Normal(format), Component::Normal(file)]
      if matches!(
        (format.to_str(), Path::new(file).extension().and_then(|value| value.to_str())),
        (Some("excel"), Some("xlsx")) | (Some("pdf"), Some("pdf"))
      )
  );
  if !valid {
    return Err(cleanup_error("工作区导出文件路径不符合受管目录约束"));
  }
  Ok(Some(path))
}

fn validate_candidates(candidates: &[ArtifactCandidate]) -> AppResult<()> {
  for candidate in candidates {
    let metadata = fs::symlink_metadata(&candidate.path).map_err(cleanup_error)?;
    if metadata.file_type().is_symlink() {
      return Err(cleanup_error("任务关联文件不能是符号链接"));
    }
    match candidate.kind {
      ArtifactKind::File if !metadata.is_file() => {
        return Err(cleanup_error("任务关联文件必须是普通文件"));
      }
      ArtifactKind::ReportDirectory if !metadata.is_dir() => {
        return Err(cleanup_error("报告快照路径必须是目录"));
      }
      ArtifactKind::ReportDirectory => validate_report_directory(&candidate.path)?,
      ArtifactKind::File => {}
    }
  }
  Ok(())
}

fn validate_report_directory(path: &Path) -> AppResult<()> {
  for entry in fs::read_dir(path).map_err(cleanup_error)? {
    let entry = entry.map_err(cleanup_error)?;
    if entry.file_name() != "report_model.json" {
      return Err(cleanup_error("报告快照目录包含未知文件，已拒绝自动删除"));
    }
    let metadata = fs::symlink_metadata(entry.path()).map_err(cleanup_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
      return Err(cleanup_error("报告快照必须是普通文件"));
    }
  }
  Ok(())
}

fn ensure_real_directory(path: &Path) -> AppResult<()> {
  let metadata = fs::symlink_metadata(path).map_err(cleanup_error)?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    return Err(cleanup_error("任务删除隔离目录必须是真实目录"));
  }
  Ok(())
}

fn cleanup_error(error: impl std::fmt::Display) -> AppError {
  AppError::new(
    AppErrorCode::WorkspaceError,
    format!("任务关联文件清理失败：{error}"),
    AppErrorStage::Workspace,
    false,
  )
}
