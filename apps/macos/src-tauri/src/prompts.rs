use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

mod regression;

use regression::evaluate_prompt_case;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromptTemplateView {
  pub id: String,
  pub template_key: String,
  pub name: String,
  pub task_type: String,
  pub description: Option<String>,
  pub output_schema_id: Option<String>,
  pub is_builtin: bool,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromptVersionView {
  pub id: String,
  pub template_id: String,
  pub version: i64,
  pub content: String,
  pub change_note: String,
  pub status: String,
  pub created_at: String,
  pub activated_at: Option<String>,
  pub rollback_from_version: Option<i64>,
  pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromptRegressionCaseView {
  pub id: String,
  pub template_id: String,
  pub name: String,
  pub input_json: Value,
  pub expected_schema_id: String,
  pub expected_rules_json: Value,
  pub enabled: bool,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromptRegressionRunView {
  pub id: String,
  pub template_id: String,
  pub prompt_version_id: String,
  pub provider_id: Option<String>,
  pub model_id: Option<String>,
  pub case_id: String,
  pub status: String,
  pub schema_valid: bool,
  pub rules_valid: bool,
  pub error_summary: Option<String>,
  pub raw_output_path: Option<String>,
  pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatePromptVersionInput {
  pub template_id: String,
  pub content: String,
  pub change_note: String,
}

#[derive(Debug, Clone, Copy)]
struct BuiltinPromptTemplate {
  key: &'static str,
  name: &'static str,
  task_type: &'static str,
  description: &'static str,
  output_schema_id: &'static str,
  content: &'static str,
}

const BUILTIN_PROMPTS: &[BuiltinPromptTemplate] = &[
  BuiltinPromptTemplate {
    key: "collection_plan_from_text",
    name: "自然语言采集意图解析",
    task_type: "collection_plan",
    description: "把自然语言需求转为 collection_intent_v1；后端再按能力目录生成并校验 v4 计划。",
    output_schema_id: "collection_intent_v1",
    content: r#"读取 input_json.text，把它作为本次意图的唯一需求证据，只输出 collection_intent_v1 JSON，不得输出 Markdown。
必须完整包含 schema_version、platform、account_source、source_input、query_locale、region_code、selected_fields、time_range_days、age_range、gender_filter、record_limit、budget_limit_micros、missing_fields 和 confidence；业务缺失必须显式写 null。
schema_version 必须为 1。platform 只允许 tiktok、douyin、xiaohongshu；account_source 只能选择当前平台支持的账号来源。selected_fields 只包含用户明确要求的公开账号业务字段。
account_source 只允许 user_search、content_search_authors、direct_account、item_author、comment_authors、followers、followings、similar_accounts 之一，绝不能填写平台值。按主题或关键词“查找/搜索账号”使用 user_search；按内容关键词发现作者才使用 content_search_authors；指定主页、用户名或账号 ID 使用 direct_account；指定作品作者使用 item_author。
关键词、用户和内容搜索必须把 source_input 翻译为目标地区适合平台检索的一个主语言，并把 query_locale 写为 language-REGION，例如英国使用 GB、en-GB 和英文检索词。检索词语言不能作为账号地区证据。
用户名、账号 ID、作品 ID、URL、分享链接必须原样保留，禁止翻译。品牌名和专有名词只有存在明确通用本地写法时才转换；不确定时保留原文并进入 missing_fields。
预算按输入原值换算为正整数 USD 微美元；年龄和性别只能表达用户明确要求的过滤条件，禁止根据头像、姓名或简介推断。
不得输出 endpoint_key、端点、步骤、步骤依赖、请求参数白名单、分页、补全或成本估算；这些安全信息由后端能力目录生成并保留证据。
无法确认平台、地区、来源、目标语言、记录数、预算或其他必需字段时写入 missing_fields，不得猜测，也不得绕过 Schema、能力、白名单、预算或用户确认。"#,
  },
  BuiltinPromptTemplate {
    key: "general_summary",
    name: "通用摘要",
    task_type: "analysis",
    description: "对标准化记录生成带证据引用的摘要。",
    output_schema_id: "analysis_summary_v1",
    content: "读取 input_json.records，只输出 JSON 摘要，每条核心结论必须包含 source_record_ids。records 为空时返回空结果，不得编造结论。",
  },
  BuiltinPromptTemplate {
    key: "comment_sentiment",
    name: "评论情绪分析",
    task_type: "analysis",
    description: "分析评论情绪并保存字段级证据。",
    output_schema_id: "sentiment_v1",
    content: "读取 input_json.records，只输出 JSON 情绪分类，每个生成字段必须包含 source_record_ids。records 为空时返回空结果，不得编造结论。",
  },
];

pub fn seed_builtin_prompts(root_path: impl AsRef<Path>) -> AppResult<Vec<PromptTemplateView>> {
  let connection = open_workspace_connection(root_path)?;
  let now = Utc::now().to_rfc3339();

  for builtin in BUILTIN_PROMPTS {
    let template_id = get_or_create_template(&connection, builtin, &now)?;
    ensure_builtin_version(&connection, &template_id, builtin, &now)?;
    ensure_builtin_regression_cases(&connection, &template_id, builtin, &now)?;
  }

  list_prompt_templates_from_connection(&connection)
}

pub fn list_prompt_templates(root_path: impl AsRef<Path>) -> AppResult<Vec<PromptTemplateView>> {
  let connection = open_workspace_connection(root_path)?;
  list_prompt_templates_from_connection(&connection)
}

pub fn list_prompt_versions(
  root_path: impl AsRef<Path>,
  template_id: &str,
) -> AppResult<Vec<PromptVersionView>> {
  let connection = open_workspace_connection(root_path)?;
  let mut statement = connection
    .prepare(
      "SELECT id, template_id, version, content, change_note, status, created_at, activated_at,
              rollback_from_version, content_hash
       FROM prompt_version
       WHERE template_id = ?1
       ORDER BY version DESC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![template_id], map_prompt_version)
    .map_err(database_error)?;
  collect_rows(rows)
}

pub fn create_prompt_version(
  root_path: impl AsRef<Path>,
  input: CreatePromptVersionInput,
) -> AppResult<PromptVersionView> {
  let connection = open_workspace_connection(root_path)?;
  let template_id = normalize_required("template_id", &input.template_id)?;
  let content = normalize_required("content", &input.content)?;
  let change_note = normalize_required("change_note", &input.change_note)?;
  let next_version = next_prompt_version(&connection, &template_id)?;
  let id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();

  connection
    .execute(
      "INSERT INTO prompt_version (
        id, template_id, version, content, change_note, status, created_at, content_hash
       ) VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, ?7)",
      params![
        id,
        template_id,
        next_version,
        content,
        change_note,
        now,
        content_hash(&content)
      ],
    )
    .map_err(database_error)?;

  get_prompt_version(&connection, &id)
}

pub fn activate_prompt_version(
  root_path: impl AsRef<Path>,
  prompt_version_id: &str,
) -> AppResult<PromptVersionView> {
  let root_path = root_path.as_ref();
  let connection = open_workspace_connection(root_path)?;
  let version = get_prompt_version(&connection, prompt_version_id)?;
  let (failures, first_error) =
    run_prompt_regressions_for_version(root_path, &connection, &version)?;

  if failures > 0 {
    connection
      .execute(
        "UPDATE prompt_version SET status = 'failed_regression' WHERE id = ?1",
        params![prompt_version_id],
      )
      .map_err(database_error)?;
    let detail = first_error
      .map(|error| format!("：{error}"))
      .unwrap_or_default();
    return Err(prompt_error(format!(
      "提示词回归样例未通过，不能激活{detail}"
    )));
  }

  let now = Utc::now().to_rfc3339();
  connection
    .execute(
      "UPDATE prompt_version SET status = 'archived' WHERE template_id = ?1 AND status = 'active'",
      params![version.template_id],
    )
    .map_err(database_error)?;
  connection
    .execute(
      "UPDATE prompt_version SET status = 'active', activated_at = ?1 WHERE id = ?2",
      params![now, prompt_version_id],
    )
    .map_err(database_error)?;

  get_prompt_version(&connection, prompt_version_id)
}

pub fn list_prompt_regression_cases(
  root_path: impl AsRef<Path>,
  template_id: &str,
) -> AppResult<Vec<PromptRegressionCaseView>> {
  let connection = open_workspace_connection(root_path)?;
  let mut statement = connection
    .prepare(
      "SELECT id, template_id, name, input_json, expected_schema_id, expected_rules_json,
              enabled, created_at, updated_at
       FROM prompt_regression_case
       WHERE template_id = ?1
       ORDER BY created_at ASC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![template_id], map_regression_case)
    .map_err(database_error)?;
  collect_rows(rows)
}

pub fn list_prompt_regression_runs(
  root_path: impl AsRef<Path>,
  prompt_version_id: &str,
) -> AppResult<Vec<PromptRegressionRunView>> {
  let connection = open_workspace_connection(root_path)?;
  let mut statement = connection
    .prepare(
      "SELECT id, template_id, prompt_version_id, provider_id, model_id, case_id, status,
              schema_valid, rules_valid, error_summary, raw_output_path, created_at
       FROM prompt_regression_run
       WHERE prompt_version_id = ?1
       ORDER BY created_at DESC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![prompt_version_id], map_regression_run)
    .map_err(database_error)?;
  collect_rows(rows)
}

fn open_workspace_connection(root_path: impl AsRef<Path>) -> AppResult<Connection> {
  open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))
}

fn get_or_create_template(
  connection: &Connection,
  builtin: &BuiltinPromptTemplate,
  now: &str,
) -> AppResult<String> {
  if let Some(id) = connection
    .query_row(
      "SELECT id FROM prompt_template WHERE template_key = ?1",
      params![builtin.key],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?
  {
    connection
      .execute(
        "UPDATE prompt_template
         SET name = ?1, task_type = ?2, description = ?3, output_schema_id = ?4,
             updated_at = ?5
         WHERE id = ?6",
        params![
          builtin.name,
          builtin.task_type,
          builtin.description,
          builtin.output_schema_id,
          now,
          id
        ],
      )
      .map_err(database_error)?;
    return Ok(id);
  }

  let id = Uuid::new_v4().to_string();
  connection
    .execute(
      "INSERT INTO prompt_template (
        id, template_key, name, task_type, description, output_schema_id,
        is_builtin, created_at, updated_at
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)",
      params![
        id,
        builtin.key,
        builtin.name,
        builtin.task_type,
        builtin.description,
        builtin.output_schema_id,
        now,
        now
      ],
    )
    .map_err(database_error)?;

  Ok(id)
}

fn ensure_builtin_version(
  connection: &Connection,
  template_id: &str,
  builtin: &BuiltinPromptTemplate,
  now: &str,
) -> AppResult<()> {
  let hash = content_hash(builtin.content);
  let already_seeded = connection
    .query_row(
      "SELECT EXISTS(
         SELECT 1 FROM prompt_version WHERE template_id = ?1 AND content_hash = ?2
       )",
      params![template_id, hash],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?
    != 0;
  if already_seeded {
    return Ok(());
  }

  let version = next_prompt_version(connection, template_id)?;
  let active_change_note = connection
    .query_row(
      "SELECT change_note FROM prompt_version
       WHERE template_id = ?1 AND status = 'active'
       ORDER BY version DESC LIMIT 1",
      params![template_id],
      |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map_err(database_error)?;
  let replace_active_builtin = match active_change_note {
    None => true,
    Some(Some(note)) => matches!(note.as_str(), "内置初始版本" | "内置 Schema 升级"),
    Some(None) => false,
  };
  let status = if replace_active_builtin {
    "active"
  } else {
    "draft"
  };
  let activated_at = replace_active_builtin.then_some(now);
  let transaction = connection.unchecked_transaction().map_err(database_error)?;
  if replace_active_builtin {
    transaction
      .execute(
        "UPDATE prompt_version SET status = 'archived'
         WHERE template_id = ?1 AND status = 'active'",
        params![template_id],
      )
      .map_err(database_error)?;
  }
  transaction
    .execute(
      "INSERT INTO prompt_version (
        id, template_id, version, content, change_note, status, created_at, activated_at,
        content_hash
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
      params![
        Uuid::new_v4().to_string(),
        template_id,
        version,
        builtin.content,
        if version == 1 {
          "内置初始版本"
        } else {
          "内置 Schema 升级"
        },
        status,
        now,
        activated_at,
        hash
      ],
    )
    .map_err(database_error)?;
  transaction.commit().map_err(database_error)
}

fn ensure_builtin_regression_cases(
  connection: &Connection,
  template_id: &str,
  builtin: &BuiltinPromptTemplate,
  now: &str,
) -> AppResult<()> {
  if builtin.key == "collection_plan_from_text" {
    connection
      .execute(
        "UPDATE prompt_regression_case SET enabled = 0, updated_at = ?1
         WHERE template_id = ?2
           AND name IN (
             '正常自然语言需求', '缺少平台', '缺少国家地区',
             'TikTok 账号搜索完整计划', '抖音人口属性补全计划', '小红书账号搜索完整计划'
           )",
        params![now, template_id],
      )
      .map_err(database_error)?;
  }
  let cases = match builtin.key {
    "collection_plan_from_text" => vec![
      (
        "英国 TikTok 中文关键词翻译",
        serde_json::json!({ "text": "用中文查找英国 TikTok 宠物用品账号，最多 10 个，预算 0.1 美元。" }),
        serde_json::json!({
          "expected_platform": "tiktok",
          "expected_account_source": "user_search",
          "expected_region_code": "GB",
          "expected_query_locale": "en-GB",
          "source_input_ascii_letters": true,
          "expected_selected_fields": [],
          "expected_missing_fields": [],
          "expected_plan_valid": true
        }),
      ),
      (
        "TikTok 主页 URL 原样保留",
        serde_json::json!({ "text": "采集英国 TikTok 账号 https://www.tiktok.com/@PetBrandUK，最多 1 个，预算 0.1 美元。" }),
        serde_json::json!({
          "expected_platform": "tiktok",
          "expected_account_source": "direct_account",
          "expected_source_input": "https://www.tiktok.com/@PetBrandUK",
          "expected_region_code": "GB",
          "expected_query_locale": null,
          "expected_selected_fields": [],
          "expected_missing_fields": [],
          "expected_plan_valid": true
        }),
      ),
      (
        "TikTok 作品 ID 原样保留",
        serde_json::json!({ "text": "采集美国 TikTok 作品 ID 7123456789012345678 的作者，最多 1 个，预算 0.1 美元。" }),
        serde_json::json!({
          "expected_platform": "tiktok",
          "expected_account_source": "item_author",
          "expected_source_input": "7123456789012345678",
          "expected_region_code": "US",
          "expected_query_locale": null,
          "expected_selected_fields": [],
          "expected_missing_fields": [],
          "expected_plan_valid": true
        }),
      ),
      (
        "缺少执行必需信息",
        serde_json::json!({ "text": "搜索宠物用品账号" }),
        serde_json::json!({
          "expected_account_source": "user_search",
          "expected_missing_contains": [
            "platform", "source_input", "query_locale", "region_code", "record_limit",
            "budget_limit_micros"
          ],
          "expected_plan_valid": false
        }),
      ),
    ],
    _ => vec![
      (
        "正常输入",
        serde_json::json!({ "records": [{ "id": "r1", "text": "好评" }] }),
        serde_json::json!({ "records_empty": false, "requires_evidence": true }),
      ),
      (
        "证据不足",
        serde_json::json!({ "records": [] }),
        serde_json::json!({ "records_empty": true, "requires_evidence": true }),
      ),
    ],
  };

  for (name, input_json, expected_rules_json) in cases {
    connection
      .execute(
        "INSERT INTO prompt_regression_case (
          id, template_id, name, input_json, expected_schema_id, expected_rules_json,
          enabled, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)
        ON CONFLICT(template_id, name) DO UPDATE SET
          input_json = excluded.input_json,
          expected_schema_id = excluded.expected_schema_id,
          expected_rules_json = excluded.expected_rules_json,
          enabled = 1,
          updated_at = excluded.updated_at",
        params![
          Uuid::new_v4().to_string(),
          template_id,
          name,
          input_json.to_string(),
          builtin.output_schema_id,
          expected_rules_json.to_string(),
          now,
          now
        ],
      )
      .map_err(database_error)?;
  }

  Ok(())
}

fn run_prompt_regressions_for_version(
  root_path: &Path,
  connection: &Connection,
  version: &PromptVersionView,
) -> AppResult<(i64, Option<String>)> {
  let cases = regression_cases_for_template(connection, &version.template_id)?;
  let mut failures = 0;
  let mut first_error = None;

  for case in cases {
    let evaluation = evaluate_prompt_case(root_path, version, &case);
    if !evaluation.schema_valid || !evaluation.rules_valid {
      failures += 1;
      if first_error.is_none() {
        first_error.clone_from(&evaluation.error_summary);
      }
    }
    connection
      .execute(
        "INSERT INTO prompt_regression_run (
          id, template_id, prompt_version_id, provider_id, model_id, case_id, status,
          schema_valid, rules_valid, error_summary, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
          Uuid::new_v4().to_string(),
          version.template_id,
          version.id,
          evaluation.provider_id,
          evaluation.model_id,
          case.id,
          if evaluation.schema_valid && evaluation.rules_valid {
            "passed"
          } else {
            "failed"
          },
          bool_to_i64(evaluation.schema_valid),
          bool_to_i64(evaluation.rules_valid),
          evaluation.error_summary,
          Utc::now().to_rfc3339()
        ],
      )
      .map_err(database_error)?;
  }

  Ok((failures, first_error))
}

fn regression_cases_for_template(
  connection: &Connection,
  template_id: &str,
) -> AppResult<Vec<PromptRegressionCaseView>> {
  let mut statement = connection
    .prepare(
      "SELECT id, template_id, name, input_json, expected_schema_id, expected_rules_json,
              enabled, created_at, updated_at
       FROM prompt_regression_case
       WHERE template_id = ?1 AND enabled = 1",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![template_id], map_regression_case)
    .map_err(database_error)?;
  collect_rows(rows)
}

fn list_prompt_templates_from_connection(
  connection: &Connection,
) -> AppResult<Vec<PromptTemplateView>> {
  let mut statement = connection
    .prepare(
      "SELECT id, template_key, name, task_type, description, output_schema_id,
              is_builtin, created_at, updated_at
       FROM prompt_template
       ORDER BY template_key",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map([], map_prompt_template)
    .map_err(database_error)?;
  collect_rows(rows)
}

fn next_prompt_version(connection: &Connection, template_id: &str) -> AppResult<i64> {
  connection
    .query_row(
      "SELECT COALESCE(MAX(version), 0) + 1 FROM prompt_version WHERE template_id = ?1",
      params![template_id],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn get_prompt_version(connection: &Connection, id: &str) -> AppResult<PromptVersionView> {
  connection
    .query_row(
      "SELECT id, template_id, version, content, change_note, status, created_at, activated_at,
              rollback_from_version, content_hash
       FROM prompt_version
       WHERE id = ?1",
      params![id],
      map_prompt_version,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| prompt_error("提示词版本不存在"))
}

fn map_prompt_template(row: &Row<'_>) -> rusqlite::Result<PromptTemplateView> {
  Ok(PromptTemplateView {
    id: row.get(0)?,
    template_key: row.get(1)?,
    name: row.get(2)?,
    task_type: row.get(3)?,
    description: row.get(4)?,
    output_schema_id: row.get(5)?,
    is_builtin: i64_to_bool(row.get(6)?),
    created_at: row.get(7)?,
    updated_at: row.get(8)?,
  })
}

fn map_prompt_version(row: &Row<'_>) -> rusqlite::Result<PromptVersionView> {
  Ok(PromptVersionView {
    id: row.get(0)?,
    template_id: row.get(1)?,
    version: row.get(2)?,
    content: row.get(3)?,
    change_note: row.get(4)?,
    status: row.get(5)?,
    created_at: row.get(6)?,
    activated_at: row.get(7)?,
    rollback_from_version: row.get(8)?,
    content_hash: row.get(9)?,
  })
}

fn map_regression_case(row: &Row<'_>) -> rusqlite::Result<PromptRegressionCaseView> {
  Ok(PromptRegressionCaseView {
    id: row.get(0)?,
    template_id: row.get(1)?,
    name: row.get(2)?,
    input_json: string_to_json(row.get(3)?),
    expected_schema_id: row.get(4)?,
    expected_rules_json: string_to_json(row.get(5)?),
    enabled: i64_to_bool(row.get(6)?),
    created_at: row.get(7)?,
    updated_at: row.get(8)?,
  })
}

fn map_regression_run(row: &Row<'_>) -> rusqlite::Result<PromptRegressionRunView> {
  Ok(PromptRegressionRunView {
    id: row.get(0)?,
    template_id: row.get(1)?,
    prompt_version_id: row.get(2)?,
    provider_id: row.get(3)?,
    model_id: row.get(4)?,
    case_id: row.get(5)?,
    status: row.get(6)?,
    schema_valid: i64_to_bool(row.get(7)?),
    rules_valid: i64_to_bool(row.get(8)?),
    error_summary: row.get(9)?,
    raw_output_path: row.get(10)?,
    created_at: row.get(11)?,
  })
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> AppResult<Vec<T>> {
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn normalize_required(field: &str, value: &str) -> AppResult<String> {
  let value = value.trim();

  if value.is_empty() {
    return Err(prompt_error(format!("{field} 不能为空")));
  }

  Ok(value.to_string())
}

fn content_hash(content: &str) -> String {
  let mut hasher = Sha256::new();
  hasher.update(content.as_bytes());
  format!("{:x}", hasher.finalize())
}

fn bool_to_i64(value: bool) -> i64 {
  if value {
    1
  } else {
    0
  }
}

fn i64_to_bool(value: i64) -> bool {
  value != 0
}

fn string_to_json(value: String) -> Value {
  serde_json::from_str(&value).unwrap_or_else(|_| serde_json::json!({}))
}

fn prompt_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    message,
    AppErrorStage::Ai,
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
#[path = "prompts/tests.rs"]
mod tests;
