pub(super) fn schema_checksum() -> String {
  use sha2::{Digest, Sha256};

  let mut hasher = Sha256::new();
  hasher.update(SCHEMA_SQL.as_bytes());
  format!("{:x}", hasher.finalize())
}

pub(super) const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  applied_at TEXT NOT NULL,
  checksum TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  last_opened_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS secret_ref (
  id TEXT PRIMARY KEY,
  provider_type TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  alias TEXT,
  secret_store_key TEXT NOT NULL,
  masked_hint TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_tested_at TEXT,
  last_test_status TEXT
);

CREATE TABLE IF NOT EXISTS model_provider (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  auth_type TEXT NOT NULL,
  secret_ref_id TEXT,
  base_url TEXT,
  api_format TEXT NOT NULL,
  region TEXT,
  default_model_id TEXT,
  cost_policy_json TEXT NOT NULL DEFAULT '{}',
  rate_limit_policy_json TEXT NOT NULL DEFAULT '{}',
  health_check_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (secret_ref_id) REFERENCES secret_ref(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS model_profile (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  capabilities_json TEXT NOT NULL DEFAULT '{}',
  context_window INTEGER,
  supports_structured_output INTEGER NOT NULL DEFAULT 0,
  supports_streaming INTEGER NOT NULL DEFAULT 0,
  supports_tools INTEGER NOT NULL DEFAULT 0,
  supports_vision INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (provider_id, model_id),
  FOREIGN KEY (provider_id) REFERENCES model_provider(provider_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS prompt_template (
  id TEXT PRIMARY KEY,
  template_key TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  task_type TEXT NOT NULL,
  description TEXT,
  output_schema_id TEXT,
  is_builtin INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS prompt_version (
  id TEXT PRIMARY KEY,
  template_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  content TEXT NOT NULL,
  change_note TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  activated_at TEXT,
  rollback_from_version INTEGER,
  content_hash TEXT NOT NULL,
  UNIQUE (template_id, version),
  FOREIGN KEY (template_id) REFERENCES prompt_template(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS prompt_regression_case (
  id TEXT PRIMARY KEY,
  template_id TEXT NOT NULL,
  name TEXT NOT NULL,
  input_json TEXT NOT NULL,
  expected_schema_id TEXT NOT NULL,
  expected_rules_json TEXT NOT NULL DEFAULT '{}',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (template_id) REFERENCES prompt_template(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS prompt_regression_run (
  id TEXT PRIMARY KEY,
  template_id TEXT NOT NULL,
  prompt_version_id TEXT NOT NULL,
  provider_id TEXT,
  model_id TEXT,
  case_id TEXT NOT NULL,
  status TEXT NOT NULL,
  schema_valid INTEGER NOT NULL DEFAULT 0,
  rules_valid INTEGER NOT NULL DEFAULT 0,
  error_summary TEXT,
  raw_output_path TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (template_id) REFERENCES prompt_template(id) ON DELETE CASCADE,
  FOREIGN KEY (prompt_version_id) REFERENCES prompt_version(id) ON DELETE CASCADE,
  FOREIGN KEY (case_id) REFERENCES prompt_regression_case(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS collection_task (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  source_type TEXT NOT NULL,
  status TEXT NOT NULL,
  platforms_json TEXT NOT NULL DEFAULT '[]',
  data_types_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  confirmed_at TEXT,
  completed_at TEXT,
  cancelled_at TEXT,
  cost_estimate_json TEXT NOT NULL DEFAULT '{}',
  actual_cost_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS task_intent (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  intent_text TEXT NOT NULL,
  language TEXT,
  parse_status TEXT NOT NULL,
  ai_run_id TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS collection_plan (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  source TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  plan_json TEXT NOT NULL,
  validation_status TEXT NOT NULL,
  validation_errors_json TEXT NOT NULL DEFAULT '[]',
  cost_estimate_json TEXT NOT NULL DEFAULT '{}',
  confirmed_by_user INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS api_call_step (
  id TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL,
  step_order INTEGER NOT NULL,
  platform TEXT NOT NULL,
  data_type TEXT NOT NULL,
  endpoint_key TEXT NOT NULL,
  params_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL,
  request_count_estimate INTEGER NOT NULL DEFAULT 0,
  cost_estimate_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (plan_id) REFERENCES collection_plan(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS task_run (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  current_stage TEXT,
  error_code TEXT,
  error_message TEXT,
  retryable INTEGER NOT NULL DEFAULT 0,
  cost_actual_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS task_log (
  id TEXT PRIMARY KEY,
  task_run_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  level TEXT NOT NULL,
  message TEXT NOT NULL,
  safe_details_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_run_id) REFERENCES task_run(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS raw_record (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  platform_record_id TEXT NOT NULL,
  raw_url TEXT,
  raw_file_path TEXT NOT NULL,
  raw_hash TEXT NOT NULL,
  summary_json TEXT NOT NULL DEFAULT '{}',
  collected_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (platform, platform_record_id, task_id),
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS normalized_record (
  id TEXT PRIMARY KEY,
  raw_record_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  author_id TEXT,
  author_name TEXT,
  content_text TEXT,
  content_url TEXT,
  published_at TEXT,
  region TEXT,
  metrics_json TEXT NOT NULL DEFAULT '{}',
  tags_json TEXT NOT NULL DEFAULT '[]',
  normalized_schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (raw_record_id) REFERENCES raw_record(id) ON DELETE CASCADE,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS runtime_snapshot (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  agent_profile_id TEXT,
  provider_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  api_format TEXT NOT NULL,
  base_url_type TEXT NOT NULL,
  prompt_version_id TEXT NOT NULL,
  output_schema_id TEXT NOT NULL,
  capabilities_json TEXT NOT NULL DEFAULT '{}',
  config_source TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE,
  FOREIGN KEY (prompt_version_id) REFERENCES prompt_version(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS ai_run (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  runtime_snapshot_id TEXT NOT NULL,
  run_type TEXT NOT NULL,
  input_record_set_id TEXT,
  input_summary TEXT,
  output_json TEXT,
  raw_output_path TEXT,
  schema_valid INTEGER NOT NULL DEFAULT 0,
  validation_status TEXT NOT NULL,
  error_code TEXT,
  error_message TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  latency_ms INTEGER,
  first_token_latency_ms INTEGER,
  retry_count INTEGER NOT NULL DEFAULT 0,
  cost_estimate_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE,
  FOREIGN KEY (runtime_snapshot_id) REFERENCES runtime_snapshot(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS field_provenance (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  ai_run_id TEXT NOT NULL,
  target_entity_type TEXT NOT NULL,
  target_entity_id TEXT NOT NULL,
  field_name TEXT NOT NULL,
  generated_value TEXT NOT NULL,
  source_record_ids_json TEXT NOT NULL DEFAULT '[]',
  source_field_paths_json TEXT NOT NULL DEFAULT '[]',
  transform_reason TEXT,
  confidence REAL NOT NULL,
  validation_status TEXT NOT NULL,
  review_status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE,
  FOREIGN KEY (ai_run_id) REFERENCES ai_run(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS insight (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  ai_run_id TEXT,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  insight_type TEXT NOT NULL,
  source_record_ids_json TEXT NOT NULL DEFAULT '[]',
  confidence REAL NOT NULL,
  review_status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE,
  FOREIGN KEY (ai_run_id) REFERENCES ai_run(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS report (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  report_type TEXT NOT NULL,
  title TEXT NOT NULL,
  report_model_json TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS export_job (
  id TEXT PRIMARY KEY,
  report_id TEXT NOT NULL,
  export_type TEXT NOT NULL,
  status TEXT NOT NULL,
  file_path TEXT,
  file_hash TEXT,
  file_size INTEGER,
  error_code TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (report_id) REFERENCES report(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS webhook_config (
  id TEXT PRIMARY KEY,
  url TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  sensitive_header_ref_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (sensitive_header_ref_id) REFERENCES secret_ref(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS webhook_job (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  config_id TEXT,
  url TEXT NOT NULL,
  status TEXT NOT NULL,
  request_body_summary_json TEXT NOT NULL DEFAULT '{}',
  response_status INTEGER,
  error_message TEXT,
  created_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE,
  FOREIGN KEY (config_id) REFERENCES webhook_config(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS backup_record (
  id TEXT PRIMARY KEY,
  backup_version TEXT NOT NULL,
  file_path TEXT NOT NULL,
  manifest_json TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_log (
  id TEXT PRIMARY KEY,
  entity_type TEXT NOT NULL,
  entity_id TEXT,
  action TEXT NOT NULL,
  safe_details_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_collection_task_status ON collection_task(status);
CREATE INDEX IF NOT EXISTS idx_collection_task_created_at ON collection_task(created_at);
CREATE INDEX IF NOT EXISTS idx_collection_task_source_type ON collection_task(source_type);
CREATE INDEX IF NOT EXISTS idx_task_run_task_id ON task_run(task_id);
CREATE INDEX IF NOT EXISTS idx_task_run_status ON task_run(status);
CREATE INDEX IF NOT EXISTS idx_raw_record_task_id ON raw_record(task_id);
CREATE INDEX IF NOT EXISTS idx_raw_record_platform ON raw_record(platform);
CREATE INDEX IF NOT EXISTS idx_raw_record_platform_record_id ON raw_record(platform_record_id);
CREATE INDEX IF NOT EXISTS idx_normalized_record_task_id ON normalized_record(task_id);
CREATE INDEX IF NOT EXISTS idx_ai_run_task_id ON ai_run(task_id);
CREATE INDEX IF NOT EXISTS idx_ai_run_run_type ON ai_run(run_type);
CREATE INDEX IF NOT EXISTS idx_field_provenance_task_id ON field_provenance(task_id);
CREATE INDEX IF NOT EXISTS idx_field_provenance_ai_run_id ON field_provenance(ai_run_id);
CREATE INDEX IF NOT EXISTS idx_insight_task_id ON insight(task_id);
CREATE INDEX IF NOT EXISTS idx_report_task_id ON report(task_id);
CREATE INDEX IF NOT EXISTS idx_export_job_report_id ON export_job(report_id);
"#;
