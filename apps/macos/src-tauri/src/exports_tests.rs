use super::*;
use crate::tasks::{create_collection_task, CreateCollectionTaskInput};
use crate::workspace::create_workspace;

#[test]
fn report_exports_xlsx_and_pdf_files() {
  let root_path = unique_temp_workspace("exports");
  create_workspace("导出测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "导出任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["comments".to_string()],
    },
  )
  .expect("task should be created");
  let report = build_report_model(&root_path, &task.id, "summary").expect("report built");
  #[cfg(unix)]
  {
    let report_dir = root_path.join("reports").join(&report.id);
    for (path, expected) in [
      (report_dir.clone(), 0o700),
      (report_dir.join("report_model.json"), 0o600),
    ] {
      let mode = fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777;
      assert_eq!(mode, expected);
    }
  }
  let xlsx = create_export_job(&root_path, &report.id, "xlsx", None).expect("xlsx exported");
  let pdf = create_export_job(&root_path, &report.id, "pdf", None).expect("pdf exported");

  assert_eq!(xlsx.status, "success");
  assert_eq!(pdf.status, "success");
  assert!(xlsx.file_path.expect("xlsx path").is_file());
  assert!(pdf.file_path.expect("pdf path").is_file());
  assert!(xlsx.file_hash.is_some());
  assert!(pdf.file_size.unwrap_or_default() > 0);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn custom_export_target_must_be_new_and_match_the_export_type() {
  let (root_path, report) = test_report("custom-target");
  let existing = root_path.join("existing.pdf");
  fs::write(&existing, b"user content").expect("sentinel should be written");

  let existing_result = create_export_job(
    &root_path,
    &report.id,
    "pdf",
    Some(existing.to_string_lossy().to_string()),
  );
  let wrong_extension = create_export_job(
    &root_path,
    &report.id,
    "pdf",
    Some(root_path.join("wrong.txt").to_string_lossy().to_string()),
  );

  assert!(existing_result.is_err());
  assert_eq!(
    fs::read(&existing).expect("sentinel should remain"),
    b"user content"
  );
  assert!(wrong_extension.is_err());
  std::fs::remove_dir_all(root_path).ok();
}

#[cfg(unix)]
#[test]
fn custom_export_target_rejects_symbolic_links() {
  use std::os::unix::fs::symlink;

  let (root_path, report) = test_report("target-symlink");
  let sentinel = root_path.join("sentinel.pdf");
  let target = root_path.join("linked.pdf");
  fs::write(&sentinel, b"user content").expect("sentinel should be written");
  symlink(&sentinel, &target).expect("target symlink should be created");

  let result = create_export_job(
    &root_path,
    &report.id,
    "pdf",
    Some(target.to_string_lossy().to_string()),
  );

  assert!(result.is_err());
  assert_eq!(
    fs::read(&sentinel).expect("sentinel should remain"),
    b"user content"
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[cfg(unix)]
#[test]
fn report_snapshot_rejects_a_symlinked_report_directory() {
  use std::os::unix::fs::symlink;

  let (root_path, mut report) = test_report("snapshot-symlink");
  report.id = Uuid::new_v4().to_string();
  let outside = root_path.join("outside-report-target");
  fs::create_dir(&outside).expect("outside directory should exist");
  let report_dir = root_path.join("reports").join(&report.id);
  symlink(&outside, &report_dir).expect("report directory symlink should exist");

  let result = write_report_snapshot(&root_path, &report);

  assert!(result.is_err());
  assert!(!outside.join("report_model.json").exists());
  std::fs::remove_dir_all(root_path).ok();
}

fn test_report(label: &str) -> (std::path::PathBuf, ReportView) {
  let root_path = unique_temp_workspace(label);
  create_workspace("导出测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "导出任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["comments".to_string()],
    },
  )
  .expect("task should be created");
  let report = build_report_model(&root_path, &task.id, "summary").expect("report built");
  (root_path, report)
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("smart-data-workbench-{label}-{}", Uuid::new_v4()))
}
