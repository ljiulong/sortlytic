use super::*;
use crate::tasks::{create_collection_task, CreateCollectionTaskInput};
use crate::workspace::create_workspace;
use std::process::Command;

#[test]
fn report_exports_xlsx_file() {
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

  assert_eq!(xlsx.status, "success");
  let xlsx_path = xlsx.file_path.expect("xlsx path");
  assert!(xlsx_path.is_file());
  assert_template_workbook_structure(&xlsx_path, 204);
  assert!(xlsx.file_hash.is_some());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn pdf_export_embeds_its_font_and_does_not_copy_raw_accounts() {
  let (root_path, report) = test_report("pdf-results");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at, ended_at, current_stage)
       VALUES ('43ffcb9a-a0bd-4f1e-b5a8-42039682db67', ?1, 'success', ?2, ?2, '已完成')",
      params![report.task_id, "2026-07-19T14:32:17+00:00"],
    )
    .expect("run should insert");
  connection
    .execute(
      "INSERT INTO collected_account (
         id, task_run_id, platform, identity_key, username, account, profile_text,
         country_region, followers_count, posts_count, data_source, collected_at,
         output_included, created_at, updated_at
       ) VALUES ('pdf-account', '43ffcb9a-a0bd-4f1e-b5a8-42039682db67', 'tiktok', 'id:verified-user', '测试账号',
         'verified_user', '公开简介', 'US', NULL, 0, 'TikHub API (keyword_search)', ?1, 1, ?1, ?1)",
      params!["2026-07-19T14:32:17+00:00"],
    )
    .expect("account should insert");
  connection
    .execute(
      "INSERT INTO collected_account (
         id, task_run_id, platform, identity_key, username, account, profile_text,
         country_region, followers_count, posts_count, data_source, collected_at,
         output_included, created_at, updated_at
       ) VALUES ('pdf-account-2', '43ffcb9a-a0bd-4f1e-b5a8-42039682db67', 'tiktok', 'id:second-user', '第二个测试账号',
         'second_user', '第二条公开简介', 'US', NULL, 0, 'TikHub API (keyword_search)', ?1, 1, ?1, ?1)",
      params!["2026-07-19T14:32:18+00:00"],
    )
    .expect("second account should insert");

  let analysis = build_report_model(&root_path, &report.task_id, "analysis")
    .expect("analysis report should build");
  let job = create_export_job(&root_path, &analysis.id, "pdf", None).expect("pdf should export");
  let bytes = fs::read(job.file_path.expect("pdf path")).expect("pdf should be readable");
  let text = String::from_utf8_lossy(&bytes);

  assert!(bytes
    .windows(b"/FontFile2".len())
    .any(|value| value == b"/FontFile2"));
  assert!(!text.contains(&pdf_hex_text("测试账号")));
  assert!(!text.contains(&pdf_hex_text("verified_user")));
  assert!(!text.contains(&pdf_hex_text("公开简介")));
  assert!(!text.contains("STSong-Light"));
  assert!(!text.contains("See XLSX export for full structured data."));
  assert!(bytes.starts_with(b"%PDF-"));
  assert!(bytes.windows(b"%%EOF".len()).any(|value| value == b"%%EOF"));
  assert_eq!(
    bytes
      .windows(b"/Type/Page".len())
      .filter(|value| *value == b"/Type/Page")
      .count(),
    2,
    "短分析报告应只有一个 Page 对象和一个 Pages 对象"
  );

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn analysis_report_model_aggregates_latest_results_without_copying_raw_profiles() {
  let (root_path, summary_report) = test_report("analysis-model");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  for (run_id, ended_at) in [
    ("analysis-old-run", "2026-07-18T14:32:17+00:00"),
    ("analysis-latest-run", "2026-07-19T14:32:17+00:00"),
  ] {
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at, ended_at, current_stage)
         VALUES (?1, ?2, 'success', ?3, ?3, '已完成')",
        params![run_id, summary_report.task_id, ended_at],
      )
      .expect("run should insert");
  }
  connection
    .execute(
      "INSERT INTO collected_account (
         id, task_run_id, platform, identity_key, username, profile_text, country_region,
         followers_count, posts_count, data_source, collected_at, output_included,
         created_at, updated_at
       ) VALUES ('analysis-old-account', 'analysis-old-run', 'douyin', 'id:old', '旧运行账号',
         '旧运行不应进入统计', 'CN', 999999, 999, 'TikHub API', ?1, 1, ?1, ?1)",
      params!["2026-07-18T14:32:17+00:00"],
    )
    .expect("old account should insert");
  for (id, region, followers, posts) in [
    ("analysis-account-1", Some("US"), Some(120_i64), Some(0_i64)),
    ("analysis-account-2", None, None, Some(8_i64)),
  ] {
    connection
      .execute(
        "INSERT INTO collected_account (
           id, task_run_id, platform, identity_key, username, profile_text, country_region,
           followers_count, posts_count, data_source, collected_at, output_included,
           created_at, updated_at
         ) VALUES (?1, 'analysis-latest-run', 'tiktok', ?2, ?1,
           '不应进入分析报告模型的原始长简介', ?3, ?4, ?5, 'TikHub API', ?6, 1, ?6, ?6)",
        params![
          id,
          format!("id:{id}"),
          region,
          followers,
          posts,
          "2026-07-19T14:32:17+00:00"
        ],
      )
      .expect("latest account should insert");
  }

  let report = build_report_model(&root_path, &summary_report.task_id, "analysis")
    .expect("analysis report should build");
  let analysis = report
    .report_model_json
    .get("analysis")
    .expect("analysis model should exist");

  assert_eq!(report.title, "导出任务 数据分析报告");
  assert_eq!(analysis["task_run_id"], "analysis-latest-run");
  assert_eq!(analysis["sample_size"], 2);
  assert_eq!(analysis["platform_breakdown"]["tiktok"], 2);
  assert_eq!(analysis["region_breakdown"]["US"], 1);
  assert_eq!(analysis["region_breakdown"]["未采集到"], 1);
  assert_eq!(analysis["field_completeness"]["followers_count"], 1);
  assert_eq!(analysis["field_completeness"]["posts_count"], 2);
  assert_eq!(analysis["followers_summary"]["minimum"], 120);
  assert_eq!(analysis["followers_summary"]["maximum"], 120);
  assert!(analysis["findings"]
    .as_array()
    .is_some_and(|items| !items.is_empty()));
  assert!(analysis["limitations"]
    .as_array()
    .is_some_and(|items| !items.is_empty()));
  assert!(!report
    .report_model_json
    .to_string()
    .contains("不应进入分析报告模型的原始长简介"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn pdf_export_rejects_summary_models_and_empty_analysis() {
  let (root_path, summary_report) = test_report("pdf-analysis-gate");

  let summary_result = create_export_job(&root_path, &summary_report.id, "pdf", None);
  let empty_analysis = build_report_model(&root_path, &summary_report.task_id, "analysis")
    .expect("empty analysis model should build for review");
  let empty_result = create_export_job(&root_path, &empty_analysis.id, "pdf", None);

  assert!(summary_result
    .expect_err("summary PDF should be rejected")
    .message
    .contains("PDF 只能导出数据分析报告"));
  assert!(empty_result
    .expect_err("empty analysis PDF should be rejected")
    .message
    .contains("没有可分析的结果数据"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn account_template_export_preserves_200_rows_and_expands_at_201_and_1200() {
  for (count, expected_last_row) in [(1, 204), (201, 205), (1_200, 1_204)] {
    let (root_path, xlsx_path) = account_export_fixture(count);
    assert_template_workbook_structure(&xlsx_path, expected_last_row);
    let sheet = unzip_entry(&xlsx_path, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains(&format!("<dimension ref=\"A1:R{expected_last_row}\"")));
    assert!(sheet.contains(&format!("sqref=\"G5:G{expected_last_row}\"")));
    assert!(sheet.contains(&format!("sqref=\"K5:K{expected_last_row}\"")));
    assert!(sheet.contains("type=\"whole\""));
    assert!(sheet.contains("<formula1>0</formula1><formula2>130</formula2>"));
    assert!(sheet.contains(&format!("<c r=\"A{expected_last_row}\"")));
    assert!(sheet.contains("IF(B"));
    let age_cell = xml_cell(&sheet, "K5");
    assert!(age_cell.contains("<v>28</v>"));
    assert!(!age_cell.contains("t=\"s\""));
    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn account_v4_export_uses_selected_fields_and_adds_field_guide() {
  let (root_path, xlsx_path) = account_v4_export_fixture();
  let workbook = unzip_entry(&xlsx_path, "xl/workbook.xml");
  let strings = unzip_entry(&xlsx_path, "xl/sharedStrings.xml");
  let accounts = unzip_entry(&xlsx_path, "xl/worksheets/sheet1.xml");

  assert_eq!(workbook.matches("<sheet ").count(), 5);
  assert!(workbook.contains("name=\"用户数据收集表\""));
  assert!(workbook.contains("name=\"字段说明\""));
  assert!(accounts.contains("<dimension ref=\"A1:J5\""));
  for header in [
    "平台",
    "显示名称",
    "账号",
    "平台用户 ID",
    "数据来源",
    "采集时间",
    "个人简介",
    "粉丝数",
    "认证状态",
    "年龄",
  ] {
    assert!(
      strings.contains(&format!(">{header}</t>")),
      "missing {header}"
    );
  }
  let zero = xml_cell(&accounts, "H5");
  assert!(zero.contains("<v>0</v>"));
  assert!(!zero.contains("t=\"s\""));
  assert!(strings.contains(">未采集到</t>"));
  assert!(strings.contains(">任务未设置</t>"));
  assert!(strings.contains("tiktok.account_profile"));
  assert!(strings.contains("user.signature"));
  assert!(!strings.contains("SECRET_CURSOR_TOKEN"));

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
fn failed_export_returns_an_error_instead_of_a_successful_job_view() {
  let (root_path, report) = test_analysis_report("failed-export");
  let target_dir = root_path.join("read-only-export");
  fs::create_dir(&target_dir).expect("target directory should exist");
  fs::set_permissions(&target_dir, fs::Permissions::from_mode(0o500))
    .expect("target directory should become read-only");

  let result = create_export_job(
    &root_path,
    &report.id,
    "pdf",
    Some(target_dir.join("report.pdf").to_string_lossy().to_string()),
  );

  fs::set_permissions(&target_dir, fs::Permissions::from_mode(0o700))
    .expect("target directory should become removable");
  assert!(result.is_err());
  let jobs =
    list_export_jobs(&root_path, Some(report.id.clone())).expect("export jobs should list");
  assert_eq!(jobs.len(), 1);
  assert_eq!(jobs[0].status, "failed");

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

fn test_analysis_report(label: &str) -> (std::path::PathBuf, ReportView) {
  let (root_path, summary_report) = test_report(label);
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at, ended_at, current_stage)
       VALUES (?1, ?2, 'success', ?3, ?3, '已完成')",
      params![
        format!("{label}-run"),
        summary_report.task_id,
        "2026-07-19T14:32:17+00:00"
      ],
    )
    .expect("run should insert");
  connection
    .execute(
      "INSERT INTO collected_account (
         id, task_run_id, platform, identity_key, username, data_source, collected_at,
         output_included, created_at, updated_at
       ) VALUES (?1, ?2, 'tiktok', ?3, '分析样本', 'TikHub API', ?4, 1, ?4, ?4)",
      params![
        format!("{label}-account"),
        format!("{label}-run"),
        format!("id:{label}"),
        "2026-07-19T14:32:17+00:00"
      ],
    )
    .expect("account should insert");
  let report = build_report_model(&root_path, &summary_report.task_id, "analysis")
    .expect("analysis report should build");
  (root_path, report)
}

fn account_export_fixture(count: usize) -> (std::path::PathBuf, std::path::PathBuf) {
  let (root_path, report) = test_report(&format!("template-{count}"));
  let connection = open_workspace_connection(&root_path).expect("database should open");
  let run_id = format!("run-{count}");
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at, ended_at, current_stage)
       VALUES (?1, ?2, 'success', ?3, ?3, '已完成')",
      params![run_id, report.task_id, "2026-07-16T08:00:00+00:00"],
    )
    .expect("run should insert");
  for index in 0..count {
    connection
      .execute(
        "INSERT INTO collected_account (
           id, task_run_id, platform, identity_key, username, account, platform_user_id,
           profile_text, country_region, region_source, region_confidence, gender, age,
           followers_count, posts_count, last_posted_at, profile_url, data_source,
           collected_at, notes, output_included, created_at, updated_at
         ) VALUES (?1, ?2, 'tiktok', ?3, ?4, ?5, ?6, '公开简介', 'US',
           'TikHub API', '高', 'female', 28, 1200, 36, ?7, ?8, 'TikHub API',
           ?9, '仅使用公开资料', 1, ?9, ?9)",
        params![
          format!("account-{index}"),
          run_id,
          format!("id:user-{index}"),
          format!("用户 {index}"),
          format!("user_{index}"),
          format!("user-id-{index}"),
          "2026-07-15T08:00:00+00:00",
          format!("https://www.tiktok.com/@user_{index}"),
          "2026-07-16T08:00:00+00:00"
        ],
      )
      .expect("account should insert");
  }
  let job = create_export_job(&root_path, &report.id, "xlsx", None).expect("xlsx should export");
  (root_path, job.file_path.expect("xlsx path should exist"))
}

fn account_v4_export_fixture() -> (std::path::PathBuf, std::path::PathBuf) {
  let (root_path, report) = test_report("account-v4-fields");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_task
       SET account_source = 'user_search', data_types_json = '[\"account\"]',
           selected_fields_json = '[\"bio\",\"followers_count\",\"verification_status\",\"age\"]'
       WHERE id = ?1",
      params![report.task_id],
    )
    .expect("v4 task scope should update");
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at, ended_at, current_stage)
       VALUES ('account-v4-run', ?1, 'success', ?2, ?2, '已完成')",
      params![report.task_id, "2026-07-20T08:00:00+00:00"],
    )
    .expect("v4 run should insert");
  connection
    .execute(
      "INSERT INTO collected_account (
         id, task_run_id, platform, identity_key, username, account, platform_user_id,
         data_source, collected_at, account_fields_json, field_evidence_json,
         output_included, created_at, updated_at
       ) VALUES ('account-v4-result', 'account-v4-run', 'tiktok', 'id:v4-user',
         '字段账号', 'field_account', 'v4-user', 'TikHub API', ?1,
         '{\"bio\":\"公开简介\",\"followers_count\":0,\"verification_status\":false,\"cursor_token\":\"SECRET_CURSOR_TOKEN\"}',
         '{\"bio\":{\"endpoint_key\":\"tiktok.account_profile\",\"raw_path\":\"user.signature\",\"collected_at\":\"2026-07-20T08:00:00+00:00\"}}',
         1, ?1, ?1)",
      params!["2026-07-20T08:00:00+00:00"],
    )
    .expect("v4 result should insert");
  drop(connection);

  let job = create_export_job(&root_path, &report.id, "xlsx", None).expect("xlsx should export");
  (root_path, job.file_path.expect("xlsx path should exist"))
}

fn assert_template_workbook_structure(path: &Path, expected_last_row: u32) {
  let workbook = unzip_entry(path, "xl/workbook.xml");
  let expected_sheets = ["用户数据收集表", "填写说明", "字段枚举", "资料依据"];
  assert_eq!(workbook.matches("<sheet ").count(), 4);
  for name in expected_sheets {
    assert!(workbook.contains(&format!("name=\"{name}\"")));
  }
  for forbidden in ["原始数据", "运行日志", "任务概览", "AI结构化结果"] {
    assert!(!workbook.contains(forbidden));
  }
  let strings = unzip_entry(path, "xl/sharedStrings.xml");
  let headers = [
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
  let mut previous = 0;
  for header in headers {
    let position = strings[previous..]
      .find(&format!(">{header}</t>"))
      .map(|offset| previous + offset)
      .unwrap_or_else(|| panic!("missing header {header}"));
    previous = position;
  }
  let sheet = unzip_entry(path, "xl/worksheets/sheet1.xml");
  assert!(sheet.contains(&format!("<dimension ref=\"A1:R{expected_last_row}\"")));
  assert!(sheet.contains("<dataValidations count=\"6\">"));
  for column in ["G", "H", "I", "J", "P"] {
    assert!(sheet.contains(&format!("sqref=\"{column}5:{column}{expected_last_row}\"")));
  }
  assert!(sheet.contains(&format!("sqref=\"K5:K{expected_last_row}\"")));
}

fn unzip_entry(path: &Path, entry: &str) -> String {
  let output = Command::new("unzip")
    .args([
      "-p",
      path.to_str().expect("xlsx path should be utf-8"),
      entry,
    ])
    .output()
    .expect("unzip should run");
  assert!(output.status.success(), "unzip failed for {entry}");
  String::from_utf8(output.stdout).expect("xlsx XML should be UTF-8")
}

fn xml_cell<'a>(sheet: &'a str, reference: &str) -> &'a str {
  let start = sheet
    .find(&format!("<c r=\"{reference}\""))
    .unwrap_or_else(|| panic!("missing cell {reference}"));
  let tail = &sheet[start..];
  let end = tail
    .find("</c>")
    .map(|index| index + 4)
    .or_else(|| tail.find("/>").map(|index| index + 2))
    .expect("cell should terminate");
  &tail[..end]
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}

fn pdf_hex_text(value: &str) -> String {
  value
    .encode_utf16()
    .map(|unit| format!("{unit:04X}"))
    .collect()
}
