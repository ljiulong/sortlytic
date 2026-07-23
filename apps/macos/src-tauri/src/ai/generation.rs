use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use super::collection_intent_schema::parse_collection_intent;
use super::intent_plan::build_collection_plan_from_intent;
use super::parse_lock::NaturalParseLock;
use super::provider_client::collection_intent_request;
use super::*;
use crate::prompts::seed_builtin_prompts;
use crate::tasks::{
  get_latest_collection_plan, normalize_natural_intent_text, save_collection_plan_in_transaction,
  SaveCollectionPlanInput,
};

const NATURAL_PARSE_LEASE_SECONDS: i64 = 300;

struct AttemptClaim {
  id: String,
  base_task_updated_at: String,
  base_latest_plan_id: Option<String>,
}

pub fn generate_collection_plan_from_text(
  root_path: impl AsRef<Path>,
  input: GenerateCollectionPlanFromTextInput,
) -> AppResult<GeneratedCollectionPlanView> {
  let root_path = root_path.as_ref().to_path_buf();
  let intent_text = normalize_natural_intent_text(&input.intent_text)?;
  let intent_text = intent_text.as_str();
  let mut connection = open_workspace_connection(&root_path)?;
  natural_parse_task_status(&connection, &input.task_id)?;
  let _parse_lock = NaturalParseLock::acquire(&root_path, &input.task_id)?;
  let now = Utc::now().to_rfc3339();
  let attempt = acquire_task_intent_attempt(&mut connection, &input.task_id, intent_text, &now)?;
  let attempt_id = attempt.id.as_str();
  preserve_attempt_error(
    seed_builtin_prompts(&root_path),
    &connection,
    attempt_id,
    "preparing",
  )?;
  let prompt = preserve_attempt_error(
    active_prompt_version(&connection, "collection_plan_from_text"),
    &connection,
    attempt_id,
    "preparing",
  )?;
  let profile = preserve_attempt_error(
    active_ai_profile(&root_path, &input),
    &connection,
    attempt_id,
    "preparing",
  )?;
  let runtime_snapshot_id = Uuid::new_v4().to_string();
  let ai_run_id = Uuid::new_v4().to_string();

  update_task_intent_phase(&connection, attempt_id, "requesting_ai", None)?;
  preserve_attempt_error(
    connection
      .execute(
        "INSERT INTO runtime_snapshot (
        id, task_id, provider_id, model_id, api_format, base_url_type, prompt_version_id,
        output_schema_id, capabilities_json, config_source, created_at
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'collection_intent_v1', ?8, 'active_api_profile', ?9)",
        params![
          runtime_snapshot_id,
          input.task_id,
          profile.profile_id,
          profile.config.model_id,
          api_format_name(profile.config.api_format),
          base_url_type(profile.config.provider_type),
          prompt.id,
          serde_json::json!({
            "structured_output": true,
            "schema_enforced_locally": true,
            "prompt_content_hash": prompt.content_hash,
            "provider_type": provider_type_name(profile.config.provider_type),
            "profile_name": profile.profile_name
          })
          .to_string(),
          now
        ],
      )
      .map(|_| ())
      .map_err(database_error),
    &connection,
    attempt_id,
    "requesting_ai",
  )?;

  preserve_attempt_error(
    connection
      .execute(
        "INSERT INTO ai_run (
          id, task_id, runtime_snapshot_id, run_type, input_summary, schema_valid,
          validation_status, retry_count, cost_estimate_json, created_at
        ) VALUES (?1, ?2, ?3, 'collection_intent_generation', ?4, 0, 'running', 0, '{}', ?5)",
        params![
          ai_run_id,
          input.task_id,
          runtime_snapshot_id,
          intent_text,
          now
        ],
      )
      .map(|_| ())
      .map_err(database_error),
    &connection,
    attempt_id,
    "requesting_ai",
  )?;
  update_task_intent_phase(&connection, attempt_id, "requesting_ai", Some(&ai_run_id))?;

  let request = collection_intent_request(&prompt.content, intent_text);
  let call_started_at = std::time::Instant::now();
  let response = match call_model_for_intent(&profile.config, &request) {
    Ok(response) => response,
    Err(error) => {
      let latency_ms = i64::try_from(call_started_at.elapsed().as_millis()).unwrap_or(i64::MAX);
      persist_failed_ai_run(
        &connection,
        FailedAiRunInput {
          ai_run_id: &ai_run_id,
          attempt_id,
          error: &error,
          latency_ms,
        },
      )?;
      return Err(error);
    }
  };
  update_task_intent_phase(&connection, attempt_id, "validating_intent", None)?;
  let raw_intent = response.output_json;
  let (parsed_intent, plan_draft, issues, schema_valid, validation_status) =
    match parse_collection_intent(&raw_intent) {
      Ok(mut intent) => {
        let preservation_issue = direct_source_preservation_issue(intent_text, &mut intent);
        let built = build_collection_plan_from_intent(intent.clone());
        intent.missing_fields.clone_from(&built.missing_fields);
        let mut issues = built.issues;
        if let Some(issue) = preservation_issue {
          issues.push(issue);
        }
        (
          Some(intent),
          built.collection_plan,
          issues,
          true,
          built.validation_status,
        )
      }
      Err(issues) => (None, None, issues, false, "needs_review".to_string()),
    };
  let persisted_intent = parsed_intent
    .as_ref()
    .and_then(|intent| serde_json::to_string(intent).ok());
  let cost_estimate_json = plan_draft
    .as_ref()
    .map(|plan| plan.cost_estimate_json.clone())
    .unwrap_or_else(|| serde_json::json!({}));

  update_task_intent_phase(&connection, attempt_id, "building_plan", Some(&ai_run_id))?;
  let missing_fields = parsed_intent
    .as_ref()
    .map(|intent| intent.missing_fields.clone())
    .unwrap_or_default();
  let final_result = (|| -> AppResult<(Option<String>, String, Vec<String>)> {
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    let current_baseline = task_parse_baseline(&transaction, &input.task_id)?;
    let task_was_edited = current_baseline.0 != attempt.base_task_updated_at
      || current_baseline.1 != attempt.base_latest_plan_id;
    let mut final_issues = issues.clone();
    let final_status = if task_was_edited {
      final_issues.push(
        "AI 响应返回前任务已被用户编辑；模型结果已保留为待审核候选，不会覆盖用户计划".to_string(),
      );
      "needs_review".to_string()
    } else {
      validation_status.clone()
    };
    let updated_ai_run = transaction
      .execute(
        "UPDATE ai_run
         SET output_json = ?1, schema_valid = ?2, validation_status = ?3,
             input_tokens = ?4, output_tokens = ?5, latency_ms = ?6,
             cost_estimate_json = ?7
         WHERE id = ?8 AND validation_status = 'running'
           AND EXISTS (
             SELECT 1 FROM task_intent
             WHERE id = ?9 AND task_id = ?10 AND parse_status = 'running'
               AND ai_run_id = ?8
           )",
        params![
          persisted_intent,
          bool_to_i64(schema_valid),
          final_status,
          response.input_tokens,
          response.output_tokens,
          response.latency_ms,
          cost_estimate_json.to_string(),
          ai_run_id,
          attempt_id,
          input.task_id
        ],
      )
      .map_err(database_error)?;
    if updated_ai_run != 1 {
      return Err(ai_error("自然语言解析所有权已变化，迟到的模型响应不能保存"));
    }

    let plan_id = if task_was_edited {
      None
    } else {
      if let Some(platform) = parsed_intent
        .as_ref()
        .and_then(|intent| intent.platform.as_ref())
      {
        let changed = transaction
          .execute(
            "UPDATE collection_task
             SET platforms_json = ?1, data_types_json = '[\"account\"]', updated_at = ?2
             WHERE id = ?3 AND updated_at = ?4
               AND status IN ('draft', 'waiting_confirmation')",
            params![
              serde_json::json!([platform]).to_string(),
              Utc::now().to_rfc3339(),
              input.task_id,
              attempt.base_task_updated_at
            ],
          )
          .map_err(database_error)?;
        if changed != 1 {
          return Err(ai_error("任务状态已变化，AI 结果不能覆盖当前编辑"));
        }
      }
      plan_draft
        .map(|plan_draft| {
          save_collection_plan_in_transaction(
            &transaction,
            SaveCollectionPlanInput {
              task_id: input.task_id.clone(),
              source: "ai_generated".to_string(),
              plan_json: plan_draft.plan_json,
              validation_status: plan_draft.validation_status,
              validation_errors_json: Some(plan_draft.validation_errors_json),
              cost_estimate_json: Some(plan_draft.cost_estimate_json),
            },
          )
        })
        .transpose()?
    };
    update_task_intent_success(
      &transaction,
      TaskIntentSuccessInput {
        attempt_id,
        parse_status: &final_status,
        ai_run_id: &ai_run_id,
        issues: &final_issues,
        missing_fields: &missing_fields,
        intent: parsed_intent.as_ref(),
        superseded_by_user_edit: task_was_edited,
      },
    )?;
    transaction.commit().map_err(database_error)?;
    Ok((plan_id, final_status, final_issues))
  })();
  let (plan_id, _final_status, issues) = match final_result {
    Ok(result) => result,
    Err(error) => {
      let _ = persist_failed_ai_run(
        &connection,
        FailedAiRunInput {
          ai_run_id: &ai_run_id,
          attempt_id,
          error: &error,
          latency_ms: response.latency_ms,
        },
      );
      return Err(error);
    }
  };
  let collection_plan = plan_id
    .map(|_| get_latest_collection_plan(&root_path, &input.task_id))
    .transpose()?;
  let ai_run = get_ai_run(&root_path, &ai_run_id)?;
  let runtime_snapshot = get_runtime_snapshot(&connection, &runtime_snapshot_id)?;

  Ok(GeneratedCollectionPlanView {
    ai_run,
    runtime_snapshot,
    parsed_intent,
    issues,
    collection_plan,
  })
}

fn acquire_task_intent_attempt(
  connection: &mut Connection,
  task_id: &str,
  intent_text: &str,
  claimed_at: &str,
) -> AppResult<AttemptClaim> {
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  prepare_task_for_natural_parse(&transaction, task_id)?;
  let baseline = task_parse_baseline(&transaction, task_id)?;

  if let Some(attempt_id) =
    claim_initial_task_intent_attempt(&transaction, task_id, intent_text, claimed_at)?
  {
    reject_recent_running_attempt(&transaction, task_id, Some(&attempt_id))?;
    super::attempts::interrupt_task_intents(&transaction, task_id, Some(&attempt_id))?;
    transaction.commit().map_err(database_error)?;
    return Ok(AttemptClaim {
      id: attempt_id,
      base_task_updated_at: baseline.0,
      base_latest_plan_id: baseline.1,
    });
  }

  reject_recent_running_attempt(&transaction, task_id, None)?;
  super::attempts::interrupt_task_intents(&transaction, task_id, None)?;

  let attempt_id = Uuid::new_v4().to_string();
  create_task_intent_attempt(&transaction, &attempt_id, task_id, intent_text, claimed_at)?;
  update_task_intent_phase(&transaction, &attempt_id, "requesting_ai", None)?;
  transaction.commit().map_err(database_error)?;
  Ok(AttemptClaim {
    id: attempt_id,
    base_task_updated_at: baseline.0,
    base_latest_plan_id: baseline.1,
  })
}

fn reject_recent_running_attempt(
  connection: &Connection,
  task_id: &str,
  except_attempt_id: Option<&str>,
) -> AppResult<()> {
  let active_attempt = connection
    .query_row(
      "SELECT updated_at, ai_run_id FROM task_intent
       WHERE task_id = ?1 AND parse_status = 'running'
         AND (?2 IS NULL OR id <> ?2)
       ORDER BY updated_at DESC, created_at DESC, id DESC LIMIT 1",
      params![task_id, except_attempt_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .optional()
    .map_err(database_error)?;
  let Some((updated_at, ai_run_id)) = active_attempt else {
    return Ok(());
  };
  let recent = ai_run_id.is_some()
    || chrono::DateTime::parse_from_rfc3339(&updated_at)
      .map(|value| {
        value.with_timezone(&Utc)
          >= Utc::now() - chrono::Duration::seconds(NATURAL_PARSE_LEASE_SECONDS)
      })
      .unwrap_or(true);
  if recent {
    return Err(
      AppError::new(
        AppErrorCode::ModelRequestError,
        "该任务已有正在进行的自然语言解析，请等待完成后再重试",
        AppErrorStage::Ai,
        true,
      )
      .with_safe_detail("reason", "natural_parse_database_lease"),
    );
  }
  Ok(())
}

fn task_parse_baseline(
  connection: &Connection,
  task_id: &str,
) -> AppResult<(String, Option<String>)> {
  connection
    .query_row(
      "SELECT task.updated_at,
              (SELECT plan.id FROM collection_plan AS plan
               WHERE plan.task_id = task.id
               ORDER BY plan.created_at DESC, plan.id DESC LIMIT 1)
       FROM collection_task AS task WHERE task.id = ?1",
      params![task_id],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| ai_error("任务不存在"))
}

pub(super) fn claim_initial_task_intent_attempt(
  connection: &Connection,
  task_id: &str,
  intent_text: &str,
  claimed_at: &str,
) -> AppResult<Option<String>> {
  connection
    .query_row(
      "UPDATE task_intent
       SET parse_phase = 'requesting_ai', updated_at = ?1
       WHERE id = (
         SELECT id FROM task_intent
         WHERE task_id = ?2 AND intent_text = ?3
           AND parse_status = 'running' AND parse_phase = 'preparing'
           AND ai_run_id IS NULL
         ORDER BY created_at ASC, id ASC
         LIMIT 1
       )
         AND parse_status = 'running' AND parse_phase = 'preparing'
         AND ai_run_id IS NULL
       RETURNING id",
      params![claimed_at, task_id, intent_text],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)
}

fn direct_source_preservation_issue(
  intent_text: &str,
  intent: &mut CollectionIntentV1,
) -> Option<String> {
  let direct_source = intent.account_source.as_deref().is_some_and(|source| {
    matches!(
      source,
      "direct_account"
        | "item_author"
        | "comment_authors"
        | "followers"
        | "followings"
        | "similar_accounts"
    )
  });
  let source_input = intent.source_input.as_deref()?.trim();
  if !direct_source
    || source_input.is_empty()
    || contains_exact_direct_source(intent_text, source_input)
  {
    return None;
  }
  if !intent
    .missing_fields
    .iter()
    .any(|field| field == "source_input")
  {
    intent.missing_fields.push("source_input".to_string());
  }
  Some(
    "用户名、账号 ID、作品 ID、URL 或分享链接必须从原始需求中逐字提取并原样保留；当前模型输出无法在原始输入中确认"
      .to_string(),
  )
}

fn contains_exact_direct_source(intent_text: &str, source_input: &str) -> bool {
  intent_text.match_indices(source_input).any(|(start, _)| {
    let end = start + source_input.len();
    let before = intent_text[..start].chars().next_back();
    let after = intent_text[end..].chars().next();
    before.is_none_or(|character| !is_direct_source_continuation(character))
      && after.is_none_or(|character| !is_direct_source_continuation(character))
  })
}

fn is_direct_source_continuation(character: char) -> bool {
  character.is_ascii_alphanumeric()
    || matches!(
      character,
      '_' | '-' | '.' | '/' | ':' | '@' | '?' | '&' | '=' | '%' | '#' | '+' | '~'
    )
}
