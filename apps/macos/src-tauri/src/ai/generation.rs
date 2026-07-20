use std::path::Path;

use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

use super::collection_intent_schema::parse_collection_intent;
use super::intent_plan::build_collection_plan_from_intent;
use super::provider_client::collection_intent_request;
use super::*;
use crate::prompts::seed_builtin_prompts;
use crate::tasks::{
  save_collection_plan, update_collection_task, SaveCollectionPlanInput, UpdateCollectionTaskInput,
};

pub fn generate_collection_plan_from_text(
  root_path: impl AsRef<Path>,
  input: GenerateCollectionPlanFromTextInput,
) -> AppResult<GeneratedCollectionPlanView> {
  let root_path = root_path.as_ref().to_path_buf();
  let intent_text = input.intent_text.trim();
  if intent_text.is_empty() {
    return Err(ai_error("自然语言采集需求不能为空"));
  }
  let connection = open_workspace_connection(&root_path)?;
  ensure_task_exists(&connection, &input.task_id)?;
  let now = Utc::now().to_rfc3339();
  let attempt_id = Uuid::new_v4().to_string();
  create_task_intent_attempt(&connection, &attempt_id, &input.task_id, intent_text, &now)?;
  preserve_attempt_error(
    seed_builtin_prompts(&root_path),
    &connection,
    &attempt_id,
    "preparing",
  )?;
  let prompt = preserve_attempt_error(
    active_prompt_version(&connection, "collection_plan_from_text"),
    &connection,
    &attempt_id,
    "preparing",
  )?;
  let profile = preserve_attempt_error(
    active_ai_profile(&root_path, &input),
    &connection,
    &attempt_id,
    "preparing",
  )?;
  let runtime_snapshot_id = Uuid::new_v4().to_string();
  let ai_run_id = Uuid::new_v4().to_string();

  update_task_intent_phase(&connection, &attempt_id, "requesting_ai", None)?;
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
            "provider_type": provider_type_name(profile.config.provider_type)
          })
          .to_string(),
          now
        ],
      )
      .map(|_| ())
      .map_err(database_error),
    &connection,
    &attempt_id,
    "requesting_ai",
  )?;

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
          attempt_id: &attempt_id,
          task_id: &input.task_id,
          runtime_snapshot_id: &runtime_snapshot_id,
          intent_text,
          error: &error,
          latency_ms,
          created_at: &now,
        },
      )?;
      return Err(error);
    }
  };
  update_task_intent_phase(&connection, &attempt_id, "validating_intent", None)?;
  let raw_intent = response.output_json;
  let (parsed_intent, plan_draft, issues, schema_valid, validation_status) =
    match parse_collection_intent(&raw_intent) {
      Ok(mut intent) => {
        let built = build_collection_plan_from_intent(intent.clone());
        intent.missing_fields.clone_from(&built.missing_fields);
        (
          Some(intent),
          built.collection_plan,
          built.issues,
          true,
          built.validation_status,
        )
      }
      Err(issues) => (None, None, issues, false, "needs_review".to_string()),
    };
  let persisted_intent = parsed_intent
    .as_ref()
    .and_then(|intent| serde_json::to_value(intent).ok())
    .unwrap_or_else(|| raw_intent.clone());
  let cost_estimate_json = plan_draft
    .as_ref()
    .map(|plan| plan.cost_estimate_json.clone())
    .unwrap_or_else(|| serde_json::json!({}));

  preserve_attempt_error(
    connection
      .execute(
      "INSERT INTO ai_run (
        id, task_id, runtime_snapshot_id, run_type, input_summary, output_json, schema_valid,
        validation_status, input_tokens, output_tokens, latency_ms, retry_count,
        cost_estimate_json, created_at
      ) VALUES (?1, ?2, ?3, 'collection_intent_generation', ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12)",
      params![
        ai_run_id,
        input.task_id,
        runtime_snapshot_id,
        intent_text,
        persisted_intent.to_string(),
        bool_to_i64(schema_valid),
        validation_status,
        response.input_tokens,
        response.output_tokens,
        response.latency_ms,
        cost_estimate_json.to_string(),
        now
      ],
      )
      .map(|_| ())
      .map_err(database_error),
    &connection,
    &attempt_id,
    "validating_intent",
  )?;

  update_task_intent_phase(&connection, &attempt_id, "building_plan", Some(&ai_run_id))?;
  if let Some(platform) = parsed_intent
    .as_ref()
    .and_then(|intent| intent.platform.as_ref())
  {
    preserve_attempt_error(
      update_collection_task(
        &root_path,
        &input.task_id,
        UpdateCollectionTaskInput {
          name: None,
          platforms: Some(vec![platform.clone()]),
          data_types: Some(vec!["account".to_string()]),
        },
      )
      .map(|_| ()),
      &connection,
      &attempt_id,
      "building_plan",
    )?;
  }

  let collection_plan = match plan_draft {
    Some(plan_draft) => Some(preserve_attempt_error(
      save_collection_plan(
        &root_path,
        SaveCollectionPlanInput {
          task_id: input.task_id.clone(),
          source: "ai_generated".to_string(),
          plan_json: plan_draft.plan_json,
          validation_status: plan_draft.validation_status,
          validation_errors_json: Some(plan_draft.validation_errors_json),
          cost_estimate_json: Some(plan_draft.cost_estimate_json),
        },
      ),
      &connection,
      &attempt_id,
      "building_plan",
    )?),
    None => None,
  };
  update_task_intent_success(&connection, &attempt_id, &validation_status, &ai_run_id)?;
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
