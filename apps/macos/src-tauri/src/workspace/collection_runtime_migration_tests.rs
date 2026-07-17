use std::fs;
use std::path::PathBuf;

use rusqlite::{params, Connection};
use uuid::Uuid;

use super::*;

const T0: &str = "2026-07-13T08:00:00+00:00";
const T1: &str = "2026-07-13T09:00:00+00:00";

#[test]
fn fresh_workspace_creates_v6_collection_runtime_snapshot_contract() {
  let root_path = unique_temp_workspace("v6-runtime-snapshot-fresh");
  let summary = create_workspace("v6 运行快照", &root_path).expect("workspace should create");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");

  assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
  assert_eq!(
    migration_marker(&connection, 6).0,
    "collection_runtime_snapshot"
  );
  let checksum = migration_marker(&connection, 6).1;
  assert_eq!(
    checksum,
    "0ca16ff59e1442fd6f5361e5736ecc91052b8522bdc8c0009cd7f38c502d7f47"
  );
  assert_eq!(
    table_columns(&connection, "secret_ref")
      .last()
      .map(String::as_str),
    Some("credential_revision")
  );
  assert_eq!(
    table_columns(&connection, "collection_runtime_snapshot"),
    [
      "id",
      "task_run_id",
      "workspace_id",
      "runtime_contract_version",
      "plan_id",
      "plan_schema_version",
      "plan_json",
      "connector_type",
      "connector_id",
      "connector_config_version",
      "base_url",
      "secret_ref_id",
      "secret_revision",
      "secret_provider_type",
      "secret_provider_id",
      "connector_tested_at",
      "connector_test_status",
      "created_at",
    ]
  );

  for trigger in [
    "trg_secret_ref_credential_revision_overflow",
    "trg_secret_ref_credential_revision",
    "trg_secret_ref_credential_invalidates_connector",
    "trg_collection_runtime_snapshot_insert",
    "trg_collection_runtime_snapshot_immutable_update",
    "trg_collection_runtime_snapshot_immutable_delete",
  ] {
    assert_eq!(
      object_count(&connection, "trigger", trigger),
      1,
      "{trigger}"
    );
  }
  assert_eq!(
    scalar_i64(
      &connection,
      "SELECT COUNT(*) FROM collection_runtime_snapshot"
    ),
    0
  );
  assert_eq!(foreign_key_violation_count(&connection), 0);

  let snapshot_sql = object_sql(&connection, "table", "collection_runtime_snapshot");
  for forbidden in [
    "secret_store_key",
    "masked_hint",
    "token",
    "api_key",
    "authorization",
    "credential_value",
  ] {
    assert!(
      !snapshot_sql.contains(forbidden),
      "runtime snapshot schema must not contain {forbidden}"
    );
  }

  fs::remove_dir_all(root_path).ok();
}

#[test]
fn runtime_snapshot_enforces_source_binding_immutability_and_one_per_run() {
  let root_path = unique_temp_workspace("v6-runtime-snapshot-contract");
  create_workspace("v6 快照约束", &root_path).expect("workspace should create");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  let fixture = insert_runtime_fixture(&connection, "contract");
  let snapshot = SnapshotInput::from_fixture(&fixture);

  assert_eq!(
    insert_runtime_snapshot(&connection, &snapshot).expect("valid snapshot should insert"),
    1
  );
  let mut duplicate = snapshot.clone();
  duplicate.id = Uuid::new_v4().to_string();
  insert_runtime_snapshot(&connection, &duplicate)
    .expect_err("one run must not accept a second snapshot");
  connection
    .execute(
      "UPDATE collection_runtime_snapshot SET connector_config_version = 2 WHERE id = ?1",
      params![snapshot.id],
    )
    .expect_err("runtime snapshot must be immutable");
  connection
    .execute(
      "DELETE FROM collection_runtime_snapshot WHERE id = ?1",
      params![snapshot.id],
    )
    .expect_err("runtime snapshot must reject direct deletion");
  assert_eq!(
    connection
      .query_row(
        "SELECT connector_config_version FROM collection_runtime_snapshot WHERE id = ?1",
        params![snapshot.id],
        |row| row.get::<_, i64>(0),
      )
      .expect("snapshot should remain"),
    1
  );

  assert_eq!(
    connection
      .execute(
        "DELETE FROM task_run WHERE id = ?1",
        params![fixture.run_id]
      )
      .expect("parent run deletion should cascade snapshot"),
    1
  );
  assert_eq!(
    scalar_i64(
      &connection,
      "SELECT COUNT(*) FROM collection_runtime_snapshot"
    ),
    0
  );
  assert_eq!(foreign_key_violation_count(&connection), 0);

  fs::remove_dir_all(root_path).ok();
}

#[test]
fn runtime_snapshot_rejects_stale_cross_scope_or_post_claim_sources() {
  for case in [
    "workspace",
    "plan_id",
    "plan_json",
    "unconfirmed_plan",
    "connector_id",
    "connector_version",
    "base_url",
    "disabled_connector",
    "secret_ref",
    "secret_revision",
    "secret_provider_id",
    "connector_test_time",
    "non_queued_run",
    "missing_run_steps",
    "started_run_step",
    "checkpoint_evidence",
  ] {
    let root_path = unique_temp_workspace(&format!("v6-runtime-source-{case}"));
    create_workspace("v6 来源校验", &root_path).expect("workspace should create");
    let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
      .expect("workspace database should open");
    let fixture = insert_runtime_fixture(&connection, case);
    let mut snapshot = SnapshotInput::from_fixture(&fixture);
    match case {
      "workspace" => snapshot.workspace_id = "foreign-workspace".to_string(),
      "plan_id" => snapshot.plan_id = "foreign-plan".to_string(),
      "plan_json" => snapshot.plan_json = "{\"schema_version\":2}".to_string(),
      "unconfirmed_plan" => {
        connection
          .execute(
            "UPDATE collection_plan SET confirmed_by_user = 0 WHERE id = ?1",
            params![fixture.plan_id],
          )
          .expect("plan confirmation should change");
      }
      "connector_id" => snapshot.connector_id = "foreign-connector".to_string(),
      "connector_version" => snapshot.connector_config_version += 1,
      "base_url" => snapshot.base_url = "https://api.tikhub.dev".to_string(),
      "disabled_connector" => {
        connection
          .execute("UPDATE tikhub_connector SET enabled = 0", [])
          .expect("connector should disable");
      }
      "secret_ref" => snapshot.secret_ref_id = "foreign-secret".to_string(),
      "secret_revision" => snapshot.secret_revision += 1,
      "secret_provider_id" => snapshot.secret_provider_id = "foreign-provider".to_string(),
      "connector_test_time" => snapshot.connector_tested_at = T1.to_string(),
      "non_queued_run" => {
        connection
          .execute(
            "UPDATE task_run SET status = 'running', claimed_at = ?1 WHERE id = ?2",
            params![T0, fixture.run_id],
          )
          .expect("run should become claimed");
      }
      "missing_run_steps" => {
        connection
          .execute(
            "DELETE FROM task_run_step WHERE task_run_id = ?1",
            params![fixture.run_id],
          )
          .expect("run steps should delete");
      }
      "started_run_step" => {
        connection
          .execute(
            "UPDATE task_run_step SET started_at = ?1 WHERE id = ?2",
            params![T0, fixture.run_step_id],
          )
          .expect("run step should show execution evidence");
      }
      "checkpoint_evidence" => {
        connection
          .execute(
            "INSERT INTO collection_page_checkpoint (
               id, task_run_step_id, page_index, idempotency_key, status, created_at, updated_at
             ) VALUES (?1, ?2, 0, ?3, 'prepared', ?4, ?4)",
            params![
              Uuid::new_v4().to_string(),
              fixture.run_step_id,
              Uuid::new_v4().to_string(),
              T0
            ],
          )
          .expect("checkpoint evidence should insert");
      }
      _ => unreachable!("runtime source case should be known"),
    }

    insert_runtime_snapshot(&connection, &snapshot)
      .expect_err("stale or post-claim source must be rejected");
    assert_eq!(
      scalar_i64(
        &connection,
        "SELECT COUNT(*) FROM collection_runtime_snapshot"
      ),
      0
    );
    assert_eq!(foreign_key_violation_count(&connection), 0);
    fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn credential_revision_tracks_only_credential_writes_and_invalidates_connector_test() {
  let root_path = unique_temp_workspace("v6-credential-revision");
  create_workspace("v6 凭据修订", &root_path).expect("workspace should create");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  let fixture = insert_runtime_fixture(&connection, "credential");

  connection
    .execute(
      "UPDATE secret_ref
       SET last_tested_at = ?1, last_test_status = 'success', updated_at = ?1
       WHERE id = ?2",
      params![T1, fixture.secret_ref_id],
    )
    .expect("non-credential metadata should update");
  assert_eq!(secret_revision(&connection, &fixture.secret_ref_id), 1);
  assert_eq!(
    connector_test_state(&connection),
    (1, Some(T0.to_string()), Some("success".to_string()))
  );

  connection
    .execute(
      "UPDATE secret_ref
       SET secret_store_key = secret_store_key, masked_hint = masked_hint, updated_at = ?1
       WHERE id = ?2",
      params![T1, fixture.secret_ref_id],
    )
    .expect("credential write intent should update revision");
  assert_eq!(secret_revision(&connection, &fixture.secret_ref_id), 2);
  assert_eq!(connector_test_state(&connection), (2, None, None));

  connection
    .execute(
      "UPDATE secret_ref SET credential_revision = 9223372036854775807 WHERE id = ?1",
      params![fixture.secret_ref_id],
    )
    .expect("overflow fixture should install");
  connection
    .execute(
      "UPDATE secret_ref SET secret_store_key = secret_store_key WHERE id = ?1",
      params![fixture.secret_ref_id],
    )
    .expect_err("credential revision overflow must fail closed");
  assert_eq!(
    secret_revision(&connection, &fixture.secret_ref_id),
    9_223_372_036_854_775_807
  );
  assert_eq!(connector_test_state(&connection), (2, None, None));

  fs::remove_dir_all(root_path).ok();
}

#[test]
fn v5_workspace_migrates_without_fabricating_historical_runtime_snapshots() {
  let root_path = unique_temp_workspace("v6-from-v5");
  create_workspace("v5 升级 v6", &root_path).expect("workspace should create");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  let fixture = insert_runtime_fixture(&connection, "legacy");
  downgrade_to_v5(&connection, &root_path);
  drop(connection);

  let summary = open_workspace(&root_path).expect("v5 workspace should migrate to v6");
  let migrated = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
    .expect("migrated database should open");
  assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
  assert_eq!(
    migration_marker(&migrated, 6).0,
    "collection_runtime_snapshot"
  );
  assert_eq!(
    migrated
      .query_row(
        "SELECT status FROM task_run WHERE id = ?1",
        params![fixture.run_id],
        |row| row.get::<_, String>(0),
      )
      .expect("legacy run should remain"),
    "queued"
  );
  assert_eq!(
    scalar_i64(
      &migrated,
      "SELECT COUNT(*) FROM collection_runtime_snapshot"
    ),
    0
  );
  assert_eq!(secret_revision(&migrated, &fixture.secret_ref_id), 2);
  assert_eq!(
    connector_test_state(&migrated),
    (2, None, Some("needs_rebind".to_string()))
  );
  assert_eq!(foreign_key_violation_count(&migrated), 0);
  drop(migrated);

  open_workspace(&root_path).expect("v6 migration should be idempotent");
  let reopened = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
    .expect("reopened database should load");
  assert_eq!(
    scalar_i64(
      &reopened,
      "SELECT COUNT(*) FROM schema_migrations WHERE version = 6"
    ),
    1
  );
  assert_eq!(
    scalar_i64(
      &reopened,
      "SELECT COUNT(*) FROM collection_runtime_snapshot"
    ),
    0
  );
  assert_eq!(
    connector_test_state(&reopened),
    (2, None, Some("needs_rebind".to_string()))
  );
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn v5_migration_invalidates_an_in_flight_first_connector_test() {
  let root_path = unique_temp_workspace("v6-from-v5-in-flight-test");
  create_workspace("v5 首次测试迁移", &root_path).expect("workspace should create");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  insert_runtime_fixture(&connection, "in-flight");
  connection
    .execute(
      "UPDATE tikhub_connector SET last_tested_at = NULL, last_test_status = NULL",
      [],
    )
    .expect("legacy first-test fixture should have no stored result");
  downgrade_to_v5(&connection, &root_path);
  drop(connection);

  open_workspace(&root_path).expect("v5 workspace should migrate to v6");
  let migrated = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
    .expect("migrated database should open");
  assert_eq!(
    connector_test_state(&migrated),
    (2, None, Some("needs_rebind".to_string()))
  );
  assert_eq!(
    migrated
      .execute(
        "UPDATE tikhub_connector
         SET last_tested_at = ?1, last_test_status = 'success'
         WHERE id = 'default' AND config_version = 1",
        params![T1],
      )
      .expect("legacy test writeback should execute"),
    0
  );
  assert_eq!(
    connector_test_state(&migrated),
    (2, None, Some("needs_rebind".to_string()))
  );
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn v6_marker_or_structure_damage_is_rejected_before_repair() {
  for damage in [
    "checksum",
    "missing_marker",
    "missing_trigger",
    "weakened_index",
    "partial_unique_index",
    "disabled_update_trigger",
    "changed_trigger_literal",
    "changed_guard_literal_case",
    "weakened_secret_revision",
    "merged_not_null_token",
    "merged_primary_key_token",
    "swapped_unique_index_rootpage",
    "swapped_secret_primary_key_rootpage",
    "missing_table",
  ] {
    let root_path = unique_temp_workspace(&format!("v6-damage-{damage}"));
    let summary = create_workspace("v6 损坏拒绝", &root_path).expect("workspace should create");
    let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
      .expect("workspace database should open");
    match damage {
      "checksum" => {
        connection
          .execute(
            "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 6",
            [],
          )
          .expect("checksum should corrupt");
      }
      "missing_marker" => {
        connection
          .execute("DELETE FROM schema_migrations WHERE version = 6", [])
          .expect("marker should delete");
      }
      "missing_trigger" => connection
        .execute_batch("DROP TRIGGER trg_collection_runtime_snapshot_insert;")
        .expect("insert trigger should drop"),
      "weakened_index" => connection
        .execute_batch(
          "DROP INDEX idx_collection_runtime_snapshot_task_run_id;
           CREATE INDEX idx_collection_runtime_snapshot_task_run_id
           ON collection_runtime_snapshot(task_run_id);",
        )
        .expect("unique index should weaken"),
      "partial_unique_index" => connection
        .execute_batch(
          "DROP INDEX idx_collection_runtime_snapshot_task_run_id;
           CREATE UNIQUE INDEX idx_collection_runtime_snapshot_task_run_id
           ON collection_runtime_snapshot(task_run_id)
           WHERE task_run_id IS NULL;",
        )
        .expect("unique index should become ineffective"),
      "disabled_update_trigger" => connection
        .execute_batch(
          "DROP TRIGGER trg_collection_runtime_snapshot_immutable_update;
           CREATE TRIGGER trg_collection_runtime_snapshot_immutable_update
           BEFORE UPDATE ON collection_runtime_snapshot
           WHEN 0
           BEGIN
             SELECT RAISE(ABORT, 'collection runtime snapshot is immutable');
           END;",
        )
        .expect("update trigger should be disabled"),
      "changed_trigger_literal" => connection
        .execute_batch(
          "DROP TRIGGER trg_collection_runtime_snapshot_immutable_update;
           CREATE TRIGGER trg_collection_runtime_snapshot_immutable_update
           BEFORE UPDATE ON collection_runtime_snapshot
           BEGIN
             SELECT RAISE(ABORT, 'collection  runtime snapshot is immutable');
           END;",
        )
        .expect("trigger literal should drift"),
      "changed_guard_literal_case" => {
        let schema_version = connection
          .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))
          .expect("schema version should load");
        connection
          .execute_batch("PRAGMA writable_schema = ON;")
          .expect("writable schema should enable");
        assert_eq!(
          connection
            .execute(
              "UPDATE sqlite_schema
               SET sql = replace(sql, 'run.status = ''queued''', 'run.status = ''QUEUED''')
               WHERE type = 'trigger' AND name = 'trg_collection_runtime_snapshot_insert'",
              [],
            )
            .expect("guard literal case should change"),
          1
        );
        connection
          .execute_batch(&format!(
            "PRAGMA writable_schema = OFF; PRAGMA schema_version = {};",
            schema_version + 1
          ))
          .expect("schema cache should invalidate");
      }
      "weakened_secret_revision" => {
        let schema_version = connection
          .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))
          .expect("schema version should load");
        connection
          .execute_batch("PRAGMA writable_schema = ON;")
          .expect("writable schema should enable");
        assert_eq!(
          connection
            .execute(
              "UPDATE sqlite_schema SET sql = ?1
               WHERE type = 'table' AND name = 'secret_ref'",
              params![
                "CREATE TABLE secret_ref (
                   id TEXT PRIMARY KEY, provider_type TEXT NOT NULL,
                   provider_id TEXT NOT NULL, alias TEXT, secret_store_key TEXT NOT NULL,
                   masked_hint TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                   last_tested_at TEXT, last_test_status TEXT,
                   credential_revision INTEGER
                   /* credential_revision INTEGER NOT NULL DEFAULT 1
                      CHECK (credential_revision > 0) */
                 )"
              ],
            )
            .expect("secret revision contract should weaken"),
          1
        );
        connection
          .execute_batch(&format!(
            "PRAGMA writable_schema = OFF; PRAGMA schema_version = {};",
            schema_version + 1
          ))
          .expect("schema cache should invalidate");
      }
      "merged_not_null_token" => {
        rewrite_schema_sql(
          &connection,
          "table",
          "collection_runtime_snapshot",
          "task_run_id TEXT NOT NULL",
          "task_run_id TEXTNOTNULL",
          None,
        );
        assert_eq!(
          table_column_contract(&connection, "collection_runtime_snapshot", "task_run_id"),
          ("TEXTNOTNULL".to_string(), false, false)
        );
      }
      "merged_primary_key_token" => {
        rewrite_schema_sql(
          &connection,
          "table",
          "collection_runtime_snapshot",
          "id TEXT PRIMARY KEY",
          "id TEXTPRIMARYKEY",
          Some("sqlite_autoindex_collection_runtime_snapshot_1"),
        );
        assert_eq!(
          table_column_contract(&connection, "collection_runtime_snapshot", "id"),
          ("TEXTPRIMARYKEY".to_string(), false, false)
        );
      }
      "swapped_unique_index_rootpage" => {
        let fixture = insert_runtime_fixture(&connection, "rootpage-swap");
        let snapshot = SnapshotInput::from_fixture(&fixture);
        assert_eq!(
          insert_runtime_snapshot(&connection, &snapshot).expect("initial snapshot should insert"),
          1
        );
        connection
          .execute_batch(
            "CREATE INDEX idx_collection_runtime_snapshot_rootpage_decoy
             ON collection_runtime_snapshot(id);",
          )
          .expect("decoy index should create");
        swap_index_rootpages(
          &connection,
          "idx_collection_runtime_snapshot_task_run_id",
          "idx_collection_runtime_snapshot_rootpage_decoy",
        );
        assert_eq!(pragma_results(&connection, "PRAGMA quick_check"), ["ok"]);
        assert_ne!(
          pragma_results(
            &connection,
            "PRAGMA integrity_check('collection_runtime_snapshot')"
          ),
          ["ok"]
        );

        let duplicate = SnapshotInput::from_fixture(&fixture);
        assert_eq!(
          insert_runtime_snapshot(&connection, &duplicate)
            .expect("swapped unique index should accept the duplicate run"),
          1
        );
        assert_eq!(
          scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM collection_runtime_snapshot NOT INDEXED"
          ),
          2
        );
      }
      "swapped_secret_primary_key_rootpage" => {
        let fixture = insert_runtime_fixture(&connection, "secret-rootpage-swap");
        connection
          .execute_batch("CREATE INDEX idx_secret_ref_rootpage_decoy ON secret_ref(provider_id);")
          .expect("secret decoy index should create");
        swap_index_rootpages(
          &connection,
          "sqlite_autoindex_secret_ref_1",
          "idx_secret_ref_rootpage_decoy",
        );
        assert_eq!(pragma_results(&connection, "PRAGMA quick_check"), ["ok"]);
        assert_ne!(
          pragma_results(&connection, "PRAGMA integrity_check('secret_ref')"),
          ["ok"]
        );
        assert_eq!(
          connection
            .execute(
              "INSERT INTO secret_ref (
                 id, provider_type, provider_id, secret_store_key, masked_hint,
                 created_at, updated_at, credential_revision
               ) VALUES (?1, 'tikhub', 'duplicate-provider', 'duplicate-key',
                         'duplicate-hint', ?2, ?2, 1)",
              params![fixture.secret_ref_id, T1],
            )
            .expect("swapped primary-key index should accept the duplicate id"),
          1
        );
        assert_eq!(
          connection
            .query_row(
              "SELECT COUNT(*) FROM secret_ref NOT INDEXED WHERE id = ?1",
              params![fixture.secret_ref_id],
              |row| row.get::<_, i64>(0),
            )
            .expect("duplicate secret count should load"),
          2
        );
      }
      "missing_table" => connection
        .execute_batch("DROP TABLE collection_runtime_snapshot;")
        .expect("snapshot table should drop"),
      _ => unreachable!("damage case should be known"),
    }
    drop(connection);

    let error = open_workspace(&root_path).expect_err("damaged v6 must fail closed");
    assert!(
      error.message.contains("v6"),
      "unexpected error: {}",
      error.message
    );
    let unchanged = open_workspace_database(root_path.join(DATABASE_FILE_NAME))
      .expect("damaged database should remain readable directly");
    assert_eq!(
      unchanged
        .query_row("SELECT last_opened_at FROM workspace", [], |row| {
          row.get::<_, String>(0)
        })
        .expect("last-opened timestamp should load"),
      summary.last_opened_at
    );
    fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn v6_partial_artifacts_and_marker_failure_leave_v5_unchanged() {
  let partial_root = unique_temp_workspace("v6-partial-artifact");
  create_workspace("v6 半套结构", &partial_root).expect("workspace should create");
  let partial = open_workspace_database(partial_root.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  downgrade_to_v5(&partial, &partial_root);
  partial
    .execute_batch(
      "ALTER TABLE secret_ref ADD COLUMN credential_revision INTEGER NOT NULL DEFAULT 1
         CHECK (credential_revision > 0);",
    )
    .expect("partial credential revision should install");
  drop(partial);
  let partial_error = open_workspace(&partial_root).expect_err("partial v6 must fail closed");
  assert!(partial_error.message.contains("v6"));
  let partial_after = open_workspace_database(partial_root.join(DATABASE_FILE_NAME))
    .expect("partial database should remain readable");
  assert_eq!(workspace_schema_version(&partial_after), 5);
  assert_eq!(
    object_count(&partial_after, "table", "collection_runtime_snapshot"),
    0
  );
  assert_eq!(
    object_count(
      &partial_after,
      "trigger",
      "trg_secret_ref_credential_revision"
    ),
    0
  );
  fs::remove_dir_all(partial_root).ok();

  let rollback_root = unique_temp_workspace("v6-marker-rollback");
  create_workspace("v6 回滚", &rollback_root).expect("workspace should create");
  let rollback = open_workspace_database(rollback_root.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  let fixture = insert_runtime_fixture(&rollback, "rollback");
  downgrade_to_v5(&rollback, &rollback_root);
  rollback
    .execute_batch(
      "CREATE TRIGGER fail_v6_marker
       BEFORE INSERT ON schema_migrations
       WHEN NEW.version = 6
       BEGIN SELECT RAISE(ABORT, 'test v6 marker failure'); END;",
    )
    .expect("marker failure trigger should install");
  drop(rollback);

  open_workspace(&rollback_root).expect_err("marker failure should roll migration back");
  let rollback_after = open_workspace_database(rollback_root.join(DATABASE_FILE_NAME))
    .expect("rolled back database should open");
  assert_eq!(workspace_schema_version(&rollback_after), 5);
  assert!(!table_columns(&rollback_after, "secret_ref")
    .iter()
    .any(|column| column == "credential_revision"));
  assert_eq!(
    object_count(&rollback_after, "table", "collection_runtime_snapshot"),
    0
  );
  assert_eq!(
    scalar_i64(
      &rollback_after,
      "SELECT COUNT(*) FROM schema_migrations WHERE version = 6"
    ),
    0
  );
  assert_eq!(
    connector_test_state(&rollback_after),
    (1, Some(T0.to_string()), Some("success".to_string()))
  );
  assert_eq!(
    rollback_after
      .query_row(
        "SELECT status FROM task_run WHERE id = ?1",
        params![fixture.run_id],
        |row| row.get::<_, String>(0),
      )
      .expect("legacy run should remain"),
    "queued"
  );
  fs::remove_dir_all(rollback_root).ok();
}

#[derive(Clone)]
struct RuntimeFixture {
  workspace_id: String,
  plan_id: String,
  plan_json: String,
  run_id: String,
  run_step_id: String,
  secret_ref_id: String,
  secret_provider_id: String,
}

#[derive(Clone)]
struct SnapshotInput {
  id: String,
  task_run_id: String,
  workspace_id: String,
  plan_id: String,
  plan_schema_version: i64,
  plan_json: String,
  connector_type: String,
  connector_id: String,
  connector_config_version: i64,
  base_url: String,
  secret_ref_id: String,
  secret_revision: i64,
  secret_provider_type: String,
  secret_provider_id: String,
  connector_tested_at: String,
  connector_test_status: String,
  created_at: String,
}

impl SnapshotInput {
  fn from_fixture(fixture: &RuntimeFixture) -> Self {
    Self {
      id: Uuid::new_v4().to_string(),
      task_run_id: fixture.run_id.clone(),
      workspace_id: fixture.workspace_id.clone(),
      plan_id: fixture.plan_id.clone(),
      plan_schema_version: 2,
      plan_json: fixture.plan_json.clone(),
      connector_type: "tikhub".to_string(),
      connector_id: "default".to_string(),
      connector_config_version: 1,
      base_url: "https://api.tikhub.io".to_string(),
      secret_ref_id: fixture.secret_ref_id.clone(),
      secret_revision: 1,
      secret_provider_type: "tikhub".to_string(),
      secret_provider_id: fixture.secret_provider_id.clone(),
      connector_tested_at: T0.to_string(),
      connector_test_status: "success".to_string(),
      created_at: T0.to_string(),
    }
  }
}

fn insert_runtime_fixture(connection: &Connection, label: &str) -> RuntimeFixture {
  let workspace_id = connection
    .query_row("SELECT id FROM workspace", [], |row| {
      row.get::<_, String>(0)
    })
    .expect("workspace id should load");
  let task_id = Uuid::new_v4().to_string();
  let plan_id = Uuid::new_v4().to_string();
  let run_id = Uuid::new_v4().to_string();
  let api_step_id = Uuid::new_v4().to_string();
  let run_step_id = Uuid::new_v4().to_string();
  let secret_ref_id = Uuid::new_v4().to_string();
  let secret_provider_id = format!("provider-{label}");
  let plan_json = serde_json::json!({
    "schema_version": 2,
    "steps": [{
      "platform": "tiktok",
      "data_type": "keyword_search",
      "endpoint_key": "tiktok.keyword_search",
      "params": { "keyword": label }
    }],
    "record_limit": 10,
    "request_limit": 1,
    "budget_limit": { "currency": "USD", "amount_micros": 1_000 }
  })
  .to_string();

  connection
    .execute(
      "INSERT INTO secret_ref (
         id, provider_type, provider_id, secret_store_key, masked_hint,
         created_at, updated_at, credential_revision
       ) VALUES (?1, 'tikhub', ?2, ?3, 'safe...[REDACTED]...hint', ?4, ?4, 1)",
      params![
        secret_ref_id,
        secret_provider_id,
        format!("keychain-reference-{label}"),
        T0
      ],
    )
    .expect("secret fixture should insert");
  connection
    .execute(
      "INSERT INTO tikhub_connector (
         id, workspace_id, secret_ref_id, base_url, enabled, config_version,
         last_tested_at, last_test_status, created_at, updated_at
       ) VALUES ('default', ?1, ?2, 'https://api.tikhub.io', 1, 1,
                 ?3, 'success', ?3, ?3)",
      params![workspace_id, secret_ref_id, T0],
    )
    .expect("connector fixture should insert");
  connection
    .execute(
      "INSERT INTO collection_task (id, name, source_type, status, created_at, updated_at)
       VALUES (?1, ?2, 'form', 'queued', ?3, ?3)",
      params![task_id, format!("task-{label}"), T0],
    )
    .expect("task fixture should insert");
  connection
    .execute(
      "INSERT INTO collection_plan (
         id, task_id, source, schema_version, plan_json, validation_status,
         validation_errors_json, cost_estimate_json, confirmed_by_user, created_at, updated_at
       ) VALUES (?1, ?2, 'form', 2, ?3, 'valid', '[]', '{}', 1, ?4, ?4)",
      params![plan_id, task_id, plan_json, T0],
    )
    .expect("plan fixture should insert");
  connection
    .execute(
      "INSERT INTO api_call_step (
         id, plan_id, step_order, platform, data_type, endpoint_key, params_json,
         status, request_count_estimate, cost_estimate_json, created_at, updated_at
       ) VALUES (?1, ?2, 0, 'tiktok', 'keyword_search', 'tiktok.keyword_search',
                 ?3, 'pending', 1, '{}', ?4, ?4)",
      params![
        api_step_id,
        plan_id,
        serde_json::json!({ "keyword": label }).to_string(),
        T0
      ],
    )
    .expect("api step fixture should insert");
  connection
    .execute(
      "INSERT INTO task_run (
         id, task_id, status, started_at, retryable, cost_actual_json,
         plan_id, attempt_number, claimed_at
       ) VALUES (?1, ?2, 'queued', ?3, 0, '{}', ?4, 1, NULL)",
      params![run_id, task_id, T0, plan_id],
    )
    .expect("run fixture should insert");
  connection
    .execute(
      "INSERT INTO task_run_step (
         id, task_run_id, api_call_step_id, status, created_at, updated_at
       ) VALUES (?1, ?2, ?3, 'pending', ?4, ?4)",
      params![run_step_id, run_id, api_step_id, T0],
    )
    .expect("run-step fixture should insert");

  RuntimeFixture {
    workspace_id,
    plan_id,
    plan_json,
    run_id,
    run_step_id,
    secret_ref_id,
    secret_provider_id,
  }
}

fn insert_runtime_snapshot(
  connection: &Connection,
  snapshot: &SnapshotInput,
) -> rusqlite::Result<usize> {
  connection.execute(
    "INSERT INTO collection_runtime_snapshot (
       id, task_run_id, workspace_id, runtime_contract_version,
       plan_id, plan_schema_version, plan_json,
       connector_type, connector_id, connector_config_version, base_url,
       secret_ref_id, secret_revision, secret_provider_type, secret_provider_id,
       connector_tested_at, connector_test_status, created_at
     ) VALUES (
       ?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
       ?11, ?12, ?13, ?14, ?15, ?16, ?17
     )",
    params![
      snapshot.id,
      snapshot.task_run_id,
      snapshot.workspace_id,
      snapshot.plan_id,
      snapshot.plan_schema_version,
      snapshot.plan_json,
      snapshot.connector_type,
      snapshot.connector_id,
      snapshot.connector_config_version,
      snapshot.base_url,
      snapshot.secret_ref_id,
      snapshot.secret_revision,
      snapshot.secret_provider_type,
      snapshot.secret_provider_id,
      snapshot.connector_tested_at,
      snapshot.connector_test_status,
      snapshot.created_at,
    ],
  )
}

fn downgrade_to_v5(connection: &Connection, root_path: &PathBuf) {
  connection
    .execute_batch(
      "DROP TRIGGER trg_collection_runtime_snapshot_immutable_delete;
       DROP TRIGGER trg_collection_runtime_snapshot_immutable_update;
       DROP TRIGGER trg_collection_runtime_snapshot_insert;
       DROP TRIGGER trg_secret_ref_credential_invalidates_connector;
       DROP TRIGGER trg_secret_ref_credential_revision;
       DROP TRIGGER trg_secret_ref_credential_revision_overflow;
       DROP INDEX idx_collection_runtime_snapshot_task_run_id;
       DROP TABLE collection_runtime_snapshot;
       ALTER TABLE secret_ref DROP COLUMN credential_revision;
       DELETE FROM schema_migrations WHERE version = 6;
       UPDATE workspace SET schema_version = 5;",
    )
    .expect("workspace should downgrade to v5 fixture");
  fs::remove_file(crate::api_profiles::api_profile_registry_path(root_path))
    .expect("v5 fixture should not retain the future API profile registry");
}

fn secret_revision(connection: &Connection, secret_ref_id: &str) -> i64 {
  connection
    .query_row(
      "SELECT credential_revision FROM secret_ref WHERE id = ?1",
      params![secret_ref_id],
      |row| row.get(0),
    )
    .expect("credential revision should load")
}

fn connector_test_state(connection: &Connection) -> (i64, Option<String>, Option<String>) {
  connection
    .query_row(
      "SELECT config_version, last_tested_at, last_test_status
       FROM tikhub_connector WHERE id = 'default'",
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .expect("connector test state should load")
}

fn workspace_schema_version(connection: &Connection) -> i64 {
  connection
    .query_row("SELECT schema_version FROM workspace", [], |row| row.get(0))
    .expect("workspace schema version should load")
}

fn foreign_key_violation_count(connection: &Connection) -> i64 {
  let mut statement = connection
    .prepare("PRAGMA foreign_key_check")
    .expect("foreign key check should prepare");
  statement
    .query_map([], |_| Ok(()))
    .expect("foreign key check should query")
    .count() as i64
}

fn object_sql(connection: &Connection, kind: &str, name: &str) -> String {
  connection
    .query_row(
      "SELECT lower(sql) FROM sqlite_schema WHERE type = ?1 AND name = ?2",
      params![kind, name],
      |row| row.get::<_, String>(0),
    )
    .expect("schema object SQL should load")
}

fn scalar_i64(connection: &Connection, sql: &str) -> i64 {
  connection
    .query_row(sql, [], |row| row.get(0))
    .expect("scalar query should load")
}

fn migration_marker(connection: &Connection, version: i64) -> (String, String) {
  connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = ?1",
      params![version],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("migration marker should exist")
}

fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
  let mut statement = connection
    .prepare(&format!("PRAGMA table_info({table})"))
    .expect("table info should prepare");
  statement
    .query_map([], |row| row.get(1))
    .expect("table info should query")
    .collect::<rusqlite::Result<Vec<_>>>()
    .expect("table columns should load")
}

fn table_column_contract(
  connection: &Connection,
  table: &str,
  column: &str,
) -> (String, bool, bool) {
  connection
    .query_row(
      "SELECT type, \"notnull\", pk FROM pragma_table_info(?1) WHERE name = ?2",
      params![table, column],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, i64>(1)? != 0,
          row.get::<_, i64>(2)? != 0,
        ))
      },
    )
    .expect("column contract should load")
}

fn rewrite_schema_sql(
  connection: &Connection,
  kind: &str,
  name: &str,
  original: &str,
  replacement: &str,
  orphaned_autoindex: Option<&str>,
) {
  let schema_version = connection
    .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))
    .expect("schema version should load");
  connection
    .execute_batch("PRAGMA writable_schema = ON;")
    .expect("writable schema should enable");
  assert_eq!(
    connection
      .execute(
        "UPDATE sqlite_schema SET sql = replace(sql, ?1, ?2)
         WHERE type = ?3 AND name = ?4 AND instr(sql, ?1) > 0",
        params![original, replacement, kind, name],
      )
      .expect("schema SQL should rewrite"),
    1
  );
  if let Some(index_name) = orphaned_autoindex {
    assert_eq!(
      connection
        .execute(
          "DELETE FROM sqlite_schema WHERE type = 'index' AND name = ?1",
          params![index_name],
        )
        .expect("orphaned autoindex should delete"),
      1
    );
  }
  connection
    .execute_batch(&format!(
      "PRAGMA writable_schema = OFF; PRAGMA schema_version = {};",
      schema_version + 1
    ))
    .expect("schema cache should invalidate");
  if orphaned_autoindex.is_some() {
    connection
      .execute_batch("VACUUM;")
      .expect("database should rebuild without the removed autoindex");
  }
}

fn swap_index_rootpages(connection: &Connection, first: &str, second: &str) {
  let rootpage = |name: &str| {
    connection
      .query_row(
        "SELECT rootpage FROM sqlite_schema WHERE type = 'index' AND name = ?1",
        params![name],
        |row| row.get::<_, i64>(0),
      )
      .expect("index rootpage should load")
  };
  let first_rootpage = rootpage(first);
  let second_rootpage = rootpage(second);
  assert_ne!(first_rootpage, second_rootpage);

  let schema_version = connection
    .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))
    .expect("schema version should load");
  connection
    .execute_batch("PRAGMA writable_schema = ON;")
    .expect("writable schema should enable");
  assert_eq!(
    connection
      .execute(
        "UPDATE sqlite_schema
         SET rootpage = CASE name WHEN ?1 THEN ?3 WHEN ?2 THEN ?4 END
         WHERE type = 'index' AND name IN (?1, ?2)",
        params![first, second, second_rootpage, first_rootpage],
      )
      .expect("index rootpages should swap"),
    2
  );
  connection
    .execute_batch(&format!(
      "PRAGMA writable_schema = OFF; PRAGMA schema_version = {};",
      schema_version + 1
    ))
    .expect("schema cache should invalidate");
}

fn pragma_results(connection: &Connection, pragma: &str) -> Vec<String> {
  let mut statement = connection.prepare(pragma).expect("pragma should prepare");
  statement
    .query_map([], |row| row.get(0))
    .expect("pragma should query")
    .collect::<rusqlite::Result<Vec<_>>>()
    .expect("pragma results should load")
}

fn object_count(connection: &Connection, kind: &str, name: &str) -> i64 {
  connection
    .query_row(
      "SELECT COUNT(*) FROM sqlite_schema WHERE type = ?1 AND name = ?2",
      params![kind, name],
      |row| row.get(0),
    )
    .expect("schema object count should load")
}

fn unique_temp_workspace(label: &str) -> PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
