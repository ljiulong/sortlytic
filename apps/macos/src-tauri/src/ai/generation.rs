use std::path::Path;

use chrono::Utc;
use rusqlite::params;
use serde_json::Value;
use uuid::Uuid;

use super::*;
use crate::collection::validate_collection_plan_v4;
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
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'collection_plan_v4', ?8, 'active_api_profile', ?9)",
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

  let request = collection_plan_request(&prompt.content, intent_text);
  let call_started_at = std::time::Instant::now();
  let response = match call_model(&profile.config, &request) {
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
  let schema_errors = validate_collection_plan_schema(&response.output_json);
  let generated = normalize_model_plan(response.output_json);
  let mut plan_validation = validate_collection_plan_v4(&generated);
  plan_validation.errors.extend(schema_errors);
  plan_validation.errors.sort();
  plan_validation.errors.dedup();
  plan_validation.valid = plan_validation.errors.is_empty();
  let schema_valid = plan_validation.valid;
  let validation_status = if schema_valid {
    "valid"
  } else {
    "needs_review"
  };

  preserve_attempt_error(
    connection
      .execute(
      "INSERT INTO ai_run (
        id, task_id, runtime_snapshot_id, run_type, input_summary, output_json, schema_valid,
        validation_status, input_tokens, output_tokens, latency_ms, retry_count,
        cost_estimate_json, created_at
      ) VALUES (?1, ?2, ?3, 'collection_plan_generation', ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12)",
      params![
        ai_run_id,
        input.task_id,
        runtime_snapshot_id,
        intent_text,
        generated.to_string(),
        bool_to_i64(schema_valid),
        validation_status,
        response.input_tokens,
        response.output_tokens,
        response.latency_ms,
        generated
          .get("cost_estimate")
          .cloned()
          .unwrap_or_else(|| serde_json::json!({}))
          .to_string(),
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
  let generated_platforms = json_string_array(generated.get("platforms"));
  if schema_valid
    && !generated_platforms.is_empty()
    && generated.get("entity").and_then(Value::as_str) == Some("account")
  {
    preserve_attempt_error(
      update_collection_task(
        &root_path,
        &input.task_id,
        UpdateCollectionTaskInput {
          name: None,
          platforms: Some(generated_platforms),
          data_types: Some(vec!["account".to_string()]),
        },
      )
      .map(|_| ()),
      &connection,
      &attempt_id,
      "building_plan",
    )?;
  }

  let collection_plan = preserve_attempt_error(
    save_collection_plan(
      &root_path,
      SaveCollectionPlanInput {
        task_id: input.task_id.clone(),
        source: "ai_generated".to_string(),
        plan_json: generated.clone(),
        validation_status: validation_status.to_string(),
        validation_errors_json: Some(serde_json::json!(plan_validation.errors)),
        cost_estimate_json: generated.get("cost_estimate").cloned(),
      },
    ),
    &connection,
    &attempt_id,
    "building_plan",
  )?;
  update_task_intent_success(&connection, &attempt_id, validation_status, &ai_run_id)?;
  let ai_run = get_ai_run(&root_path, &ai_run_id)?;
  let runtime_snapshot = get_runtime_snapshot(&connection, &runtime_snapshot_id)?;

  Ok(GeneratedCollectionPlanView {
    ai_run,
    runtime_snapshot,
    collection_plan,
  })
}

fn json_string_array(value: Option<&Value>) -> Vec<String> {
  value
    .and_then(Value::as_array)
    .map(|values| {
      values
        .iter()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
    })
    .unwrap_or_default()
}
