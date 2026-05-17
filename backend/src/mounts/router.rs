use std::{
    collections::HashMap,
    path::{Component, Path},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::json;

use crate::{
    error::AppError,
    mounts::{
        models::{ConnectorKind, CredentialProfile, MountAuditRecord, MountPolicy, MountRecord},
        mysql::{self, MysqlConnector},
        storage,
    },
};

#[derive(Debug, Clone)]
pub struct MountContext {
    pub mount: MountRecord,
    pub policy: MountPolicy,
    pub credential: CredentialProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupPath {
    pub schema: String,
    pub table: String,
    pub kind: LookupKind,
    pub column: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupKind {
    Primary,
    Unique,
    Index,
}

#[derive(Debug, Default)]
pub struct MountCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    expires_at: Instant,
    bytes: Vec<u8>,
}

impl MountCache {
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let mut entries = self.entries.lock().expect("mount cache lock poisoned");
        let entry = entries.get(key)?;
        if entry.expires_at > Instant::now() {
            return Some(entry.bytes.clone());
        }
        entries.remove(key);
        None
    }

    pub fn insert(&self, key: String, ttl: Duration, bytes: Vec<u8>) {
        self.entries
            .lock()
            .expect("mount cache lock poisoned")
            .insert(
                key,
                CacheEntry {
                    expires_at: Instant::now() + ttl,
                    bytes,
                },
            );
    }

    pub fn clear_mount(&self, project_id: &str, mount_id: &str) {
        let prefix = format!("{project_id}::{mount_id}::");
        self.entries
            .lock()
            .expect("mount cache lock poisoned")
            .retain(|key, _| !key.starts_with(&prefix));
    }
}

pub async fn read_virtual_file<C: MysqlConnector>(
    context: &MountContext,
    connector: &C,
    cache: &MountCache,
    virtual_path: &str,
) -> Result<Vec<u8>, AppError> {
    let normalized = normalize_virtual_path(virtual_path);
    let key = cache_key(context, &normalized);
    if let Some(bytes) = cache.get(&key) {
        return Ok(bytes);
    }

    let bytes = render_virtual_file(context, connector, &normalized).await?;
    let bytes = enforce_max_file_bytes(bytes, context.policy.max_file_bytes)?;
    cache.insert(key, cache_ttl(&normalized), bytes.clone());
    Ok(bytes)
}

pub async fn read_virtual_file_audited<C: MysqlConnector>(
    context: &MountContext,
    connector: &C,
    cache: &MountCache,
    store_path: &Path,
    virtual_path: &str,
) -> Result<Vec<u8>, AppError> {
    let started = Instant::now();
    let result = read_virtual_file(context, connector, cache, virtual_path).await;
    let duration_ms = started.elapsed().as_millis();
    let (result_label, bytes_returned, error) = match &result {
        Ok(bytes) => ("ok".to_string(), bytes.len(), None),
        Err(error) => ("error".to_string(), 0, Some(error.to_string())),
    };

    let mut store = storage::load_mount_store(store_path)?;
    storage::append_audit_event(
        &mut store,
        MountAuditRecord {
            timestamp: chrono::Utc::now(),
            project_id: context.mount.project_id.clone(),
            mount_id: context.mount.mount_id.clone(),
            virtual_path: normalize_virtual_path(virtual_path),
            result: result_label,
            bytes_returned,
            duration_ms,
            error,
        },
    );
    storage::save_mount_store(store_path, &store)?;

    result
}

pub async fn readdir_virtual<C: MysqlConnector>(
    context: &MountContext,
    connector: &C,
    virtual_path: &str,
) -> Result<Vec<String>, AppError> {
    let parts = path_parts(virtual_path);
    match parts.as_slice() {
        [] => Ok(vec![
            "README.md".to_string(),
            "connection.json".to_string(),
            "health.json".to_string(),
            "schemas".to_string(),
            "queries".to_string(),
        ]),
        ["schemas"] => connector.schemas(&context.policy).await,
        ["schemas", _schema] => Ok(vec!["schema.json".to_string(), "tables".to_string()]),
        ["schemas", schema, "tables"] => connector.tables(&context.policy, schema).await,
        ["schemas", schema, "tables", table] => {
            let indexes = connector.indexes(&context.policy, schema, table).await?;
            let mut entries = vec![
                "schema.sql".to_string(),
                "columns.json".to_string(),
                "indexes.sql".to_string(),
                "foreign_keys.json".to_string(),
                "inferred_relations.json".to_string(),
                "lookup_manifest.json".to_string(),
                "count.txt".to_string(),
                "sample.jsonl".to_string(),
                "stats".to_string(),
                "lookup".to_string(),
            ];
            if mysql::manifest_from_indexes(&indexes, &context.policy).is_empty() {
                entries.retain(|entry| entry != "lookup");
            }
            Ok(entries)
        }
        ["schemas", _, "tables", _, "stats"] => Ok(vec![
            "status_counts.json".to_string(),
            "null_counts.json".to_string(),
            "top_values".to_string(),
        ]),
        ["schemas", _, "tables", _, "stats", "top_values"] => Ok(vec!["README.md".to_string()]),
        ["schemas", schema, "tables", table, "lookup"] => {
            let indexes = connector.indexes(&context.policy, schema, table).await?;
            let manifest = mysql::manifest_from_indexes(&indexes, &context.policy);
            let mut dirs = Vec::new();
            if manifest
                .iter()
                .any(|entry| entry.query_shape == "primary-key")
            {
                dirs.push("by-primary".to_string());
            }
            if manifest
                .iter()
                .any(|entry| entry.query_shape == "unique-key")
            {
                dirs.push("by-unique".to_string());
            }
            if manifest
                .iter()
                .any(|entry| entry.query_shape == "index-lookup")
            {
                dirs.push("by-index".to_string());
            }
            dirs.push("README.md".to_string());
            Ok(dirs)
        }
        ["schemas", schema, "tables", table, "lookup", lookup_dir] => {
            let indexes = connector.indexes(&context.policy, schema, table).await?;
            Ok(indexes
                .into_iter()
                .filter(|index| index.columns.len() == 1)
                .filter(|index| match *lookup_dir {
                    "by-primary" => index.primary,
                    "by-unique" => index.unique && !index.primary,
                    "by-index" => !index.unique && !index.primary,
                    _ => false,
                })
                .map(|index| index.columns[0].clone())
                .collect())
        }
        ["schemas", _, "tables", _, "lookup", "by-primary", _]
        | ["schemas", _, "tables", _, "lookup", "by-unique", _]
        | ["schemas", _, "tables", _, "lookup", "by-index", _] => Ok(Vec::new()),
        ["queries"] => Ok(vec!["README.md".to_string()]),
        _ => Err(AppError::NotFound(format!(
            "virtual directory '{}' was not found",
            virtual_path
        ))),
    }
}

pub fn parse_lookup_path(virtual_path: &str) -> Option<LookupPath> {
    let parts = path_parts(virtual_path);
    let [
        "schemas",
        schema,
        "tables",
        table,
        "lookup",
        lookup_kind,
        column,
        file,
    ] = parts.as_slice()
    else {
        return None;
    };

    let (kind, suffix) = match *lookup_kind {
        "by-primary" => (LookupKind::Primary, ".json"),
        "by-unique" => (LookupKind::Unique, ".json"),
        "by-index" => (LookupKind::Index, ".jsonl"),
        _ => return None,
    };
    let value = file.strip_suffix(suffix)?;

    Some(LookupPath {
        schema: (*schema).to_string(),
        table: (*table).to_string(),
        kind,
        column: (*column).to_string(),
        value: value.to_string(),
    })
}

async fn render_virtual_file<C: MysqlConnector>(
    context: &MountContext,
    connector: &C,
    virtual_path: &str,
) -> Result<Vec<u8>, AppError> {
    if let Some(lookup) = parse_lookup_path(virtual_path) {
        return render_lookup(context, connector, lookup).await;
    }

    let parts = path_parts(virtual_path);
    match parts.as_slice() {
        ["README.md"] | [] => Ok(readme().into_bytes()),
        ["connection.json"] => to_pretty_bytes(&mysql::connection_summary(&context.credential.redacted_dsn)),
        ["health.json"] => {
            let health = connector.health(&context.policy).await?;
            to_pretty_bytes(&json!({
                "ok": health.ok,
                "message": health.message,
                "mountId": context.mount.mount_id,
                "connectorKind": context.mount.connector_kind
            }))
        }
        ["schemas", schema, "schema.json"] => {
            let tables = connector.tables(&context.policy, schema).await?;
            to_pretty_bytes(&json!({ "schema": schema, "tables": tables }))
        }
        ["schemas", schema, "tables", table, "schema.sql"] => Ok(connector
            .schema_sql(&context.policy, schema, table)
            .await?
            .into_bytes()),
        ["schemas", schema, "tables", table, "columns.json"] => {
            to_pretty_bytes(&connector.columns(&context.policy, schema, table).await?)
        }
        ["schemas", schema, "tables", table, "indexes.sql"] => {
            let indexes = connector.indexes(&context.policy, schema, table).await?;
            Ok(mysql::indexes_sql(&indexes, schema, table).into_bytes())
        }
        ["schemas", schema, "tables", table, "foreign_keys.json"] => {
            to_pretty_bytes(&connector.foreign_keys(&context.policy, schema, table).await?)
        }
        ["schemas", schema, "tables", table, "inferred_relations.json"] => {
            let columns = connector.columns(&context.policy, schema, table).await?;
            let tables = connector.tables(&context.policy, schema).await?;
            to_pretty_bytes(&mysql::infer_relations(&columns, &tables, schema))
        }
        ["schemas", schema, "tables", table, "lookup_manifest.json"] => {
            let indexes = connector.indexes(&context.policy, schema, table).await?;
            to_pretty_bytes(&mysql::manifest_from_indexes(&indexes, &context.policy))
        }
        ["schemas", schema, "tables", table, "sample.jsonl"] => {
            let rows = connector.sample_rows(&context.policy, schema, table).await?;
            Ok(mysql::rows_to_jsonl(&rows).into_bytes())
        }
        ["schemas", schema, "tables", table, "count.txt"] => {
            let count = connector.count(&context.policy, schema, table).await?;
            Ok(match count {
                Some(count) => format!("{count}\n").into_bytes(),
                None => b"count unavailable\n".to_vec(),
            })
        }
        ["schemas", _, "tables", _, "lookup", "README.md"] => Ok(lookup_readme().into_bytes()),
        ["schemas", _, "tables", _, "stats", "top_values", "README.md"] => {
            Ok(top_values_readme().into_bytes())
        }
        ["schemas", _, "tables", _, "stats", "status_counts.json"]
        | ["schemas", _, "tables", _, "stats", "null_counts.json"] => {
            Err(AppError::BadRequest(
                "stats files are not implemented in the Phase 1 MVP".to_string(),
            ))
        }
        ["schemas", _, "tables", _, "stats", "top_values", column_file] => {
            let column = column_file.trim_end_matches(".json");
            if crate::mounts::policy::top_values_allowed(&context.policy, column, None) {
                Err(AppError::BadRequest(
                    "top_values aggregation is not implemented in the Phase 1 MVP".to_string(),
                ))
            } else {
                Err(AppError::BadRequest(format!(
                    "top_values is blocked for column '{column}' by policy"
                )))
            }
        }
        ["queries", "README.md"] => Ok(
            "Custom queries are disabled in the Phase 1 MVP. Use lookup_manifest.json and addressable lookup paths.\n"
                .as_bytes()
                .to_vec(),
        ),
        _ => Err(AppError::NotFound(format!(
            "virtual file '{}' was not found",
            virtual_path
        ))),
    }
}

async fn render_lookup<C: MysqlConnector>(
    context: &MountContext,
    connector: &C,
    lookup: LookupPath,
) -> Result<Vec<u8>, AppError> {
    if !context.policy.allow_addressable_lookups {
        return Err(AppError::BadRequest(
            "addressable lookups are disabled by policy".to_string(),
        ));
    }

    let indexes = connector
        .indexes(&context.policy, &lookup.schema, &lookup.table)
        .await?;
    let allowed = indexes.iter().any(|index| {
        index.columns.len() == 1
            && index.columns[0] == lookup.column
            && match lookup.kind {
                LookupKind::Primary => index.primary,
                LookupKind::Unique => index.unique && !index.primary,
                LookupKind::Index => !index.unique,
            }
    });
    if !allowed {
        return Err(AppError::BadRequest(format!(
            "lookup by '{}' is not allowed for {}.{}",
            lookup.column, lookup.schema, lookup.table
        )));
    }

    let max_rows = match lookup.kind {
        LookupKind::Primary | LookupKind::Unique => 1,
        LookupKind::Index => context.policy.max_lookup_rows,
    };
    let rows = connector
        .lookup_rows(
            &context.policy,
            &lookup.schema,
            &lookup.table,
            &lookup.column,
            &lookup.value,
            max_rows,
        )
        .await?;
    match lookup.kind {
        LookupKind::Primary | LookupKind::Unique => to_pretty_bytes(&mysql::single_row_json(&rows)),
        LookupKind::Index => Ok(mysql::rows_to_jsonl(&rows).into_bytes()),
    }
}

fn enforce_max_file_bytes(bytes: Vec<u8>, max_file_bytes: usize) -> Result<Vec<u8>, AppError> {
    if bytes.len() <= max_file_bytes {
        Ok(bytes)
    } else {
        Err(AppError::BadRequest(format!(
            "virtual file exceeded maxFileBytes ({max_file_bytes})"
        )))
    }
}

fn to_pretty_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, AppError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn normalize_virtual_path(path: &str) -> String {
    path_parts(path).join("/")
}

fn path_parts(path: &str) -> Vec<&str> {
    Path::new(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect()
}

fn cache_key(context: &MountContext, path: &str) -> String {
    format!(
        "{}::{}::{}",
        context.mount.project_id, context.mount.mount_id, path
    )
}

fn cache_ttl(path: &str) -> Duration {
    if parse_lookup_path(path).is_some() {
        Duration::from_secs(30)
    } else if path.ends_with("sample.jsonl") {
        Duration::from_secs(60)
    } else if path.contains("/stats/") {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(300)
    }
}

fn readme() -> String {
    "# MySQL Context Mount\n\nThis is a read-only Traceway context mount.\n\nRecommended order:\n1. Read `connection.json`.\n2. Inspect `schemas/<db>/tables/<table>/schema.sql`.\n3. Read `columns.json`, `indexes.sql`, `foreign_keys.json`, and `inferred_relations.json`.\n4. Read `lookup_manifest.json` to understand safe lookup paths.\n5. Use `sample.jsonl` only as limited sample data.\n6. Use lookup paths for targeted reads, for example `lookup/by-primary/id/123.json`.\n\nDo not assume `sample.jsonl` contains complete table data. This mount does not support arbitrary SQL or arbitrary JOIN in the Phase 1 MVP.\n".to_string()
}

fn lookup_readme() -> String {
    "Addressable lookup value files are virtual and are not listed by directory reads. Use `lookup_manifest.json` for exact path templates.\n".to_string()
}

fn top_values_readme() -> String {
    "Top values are disabled for sensitive or high-cardinality columns. Phase 1 does not materialize top value files by default.\n".to_string()
}

pub fn supports_mount_kind(kind: &ConnectorKind) -> bool {
    matches!(kind, ConnectorKind::Mysql)
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::{Value, json};
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::mounts::models::{
        ColumnInfo, ConnectorKind, CredentialProfile, ForeignKeyInfo, IndexInfo, MountStatus,
    };

    use super::*;

    #[test]
    fn parses_addressable_lookup_paths() {
        assert_eq!(
            parse_lookup_path("schemas/app/tables/users/lookup/by-primary/id/123.json"),
            Some(LookupPath {
                schema: "app".to_string(),
                table: "users".to_string(),
                kind: LookupKind::Primary,
                column: "id".to_string(),
                value: "123".to_string(),
            })
        );
        assert_eq!(
            parse_lookup_path("schemas/app/tables/users/lookup/by-index/user_id/123.jsonl")
                .expect("lookup")
                .kind,
            LookupKind::Index
        );
        assert!(parse_lookup_path("schemas/app/tables/users/lookup/by-index/user_id").is_none());
    }

    #[tokio::test]
    async fn readdir_does_not_enumerate_addressable_lookup_values() {
        let context = test_context();
        let connector = MockConnector;

        let entries = readdir_virtual(
            &context,
            &connector,
            "schemas/app/tables/users/lookup/by-primary/id",
        )
        .await
        .expect("readdir");

        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn primary_lookup_renders_single_redacted_row() {
        let context = test_context();
        let connector = MockConnector;
        let cache = MountCache::default();

        let bytes = read_virtual_file(
            &context,
            &connector,
            &cache,
            "schemas/app/tables/users/lookup/by-primary/id/123.json",
        )
        .await
        .expect("read lookup");
        let value: Value = serde_json::from_slice(&bytes).expect("json");

        assert_eq!(value["id"], 123);
        assert_eq!(value["email"], "[redacted-email]");
    }

    #[tokio::test]
    async fn primary_key_is_not_authorized_as_unique_lookup() {
        let context = test_context();
        let connector = MockConnector;
        let cache = MountCache::default();

        let error = read_virtual_file(
            &context,
            &connector,
            &cache,
            "schemas/app/tables/users/lookup/by-unique/id/123.json",
        )
        .await
        .expect_err("primary key should not be accepted under by-unique");

        assert!(error.to_string().contains("lookup by 'id' is not allowed"));
    }

    #[tokio::test]
    async fn audited_read_records_virtual_file_access() {
        let context = test_context();
        let connector = MockConnector;
        let cache = MountCache::default();
        let temp_dir = unique_temp_dir("audit");
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let store_path = temp_dir.join("mounts.json");

        let bytes = read_virtual_file_audited(
            &context,
            &connector,
            &cache,
            &store_path,
            "schemas/app/tables/users/sample.jsonl",
        )
        .await
        .expect("audited read");
        let store = storage::load_mount_store(&store_path).expect("load store");

        assert!(!bytes.is_empty());
        assert_eq!(store.audit_events.len(), 1);
        assert_eq!(store.audit_events[0].project_id, "project_a");
        assert_eq!(store.audit_events[0].mount_id, "mysql-main");
        assert_eq!(
            store.audit_events[0].virtual_path,
            "schemas/app/tables/users/sample.jsonl"
        );
        assert_eq!(store.audit_events[0].result, "ok");
        assert!(store.audit_events[0].bytes_returned > 0);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    fn test_context() -> MountContext {
        let now = Utc::now();
        MountContext {
            mount: MountRecord {
                project_id: "project_a".to_string(),
                mount_id: "mysql-main".to_string(),
                connector_kind: ConnectorKind::Mysql,
                display_name: "Main MySQL".to_string(),
                mount_point: "/tmp/project/.traceway/mounts/mysql-main".to_string(),
                credential_profile_id: "cred_a".to_string(),
                policy_id: "policy_a".to_string(),
                status: MountStatus::Stopped,
                last_health_check_at: None,
                last_error: None,
                created_at: now,
                updated_at: now,
            },
            policy: MountPolicy::default_for("project_a", "policy_a"),
            credential: CredentialProfile {
                profile_id: "cred_a".to_string(),
                project_id: "project_a".to_string(),
                kind: ConnectorKind::Mysql,
                display_name: "Main MySQL".to_string(),
                redacted_dsn: "mysql://readonly@127.0.0.1:3306/app".to_string(),
                dsn_storage: "local-plaintext-todo-keychain".to_string(),
                created_at: now,
                updated_at: now,
            },
        }
    }

    struct MockConnector;

    #[async_trait]
    impl MysqlConnector for MockConnector {
        async fn health(&self, _policy: &MountPolicy) -> Result<mysql::MysqlHealth, AppError> {
            Ok(mysql::MysqlHealth {
                ok: true,
                message: "ok".to_string(),
            })
        }

        async fn schemas(&self, _policy: &MountPolicy) -> Result<Vec<String>, AppError> {
            Ok(vec!["app".to_string()])
        }

        async fn tables(
            &self,
            _policy: &MountPolicy,
            _schema: &str,
        ) -> Result<Vec<String>, AppError> {
            Ok(vec!["users".to_string(), "orders".to_string()])
        }

        async fn schema_sql(
            &self,
            _policy: &MountPolicy,
            _schema: &str,
            _table: &str,
        ) -> Result<String, AppError> {
            Ok("CREATE TABLE `users` (`id` bigint primary key);\n".to_string())
        }

        async fn columns(
            &self,
            _policy: &MountPolicy,
            _schema: &str,
            _table: &str,
        ) -> Result<Vec<ColumnInfo>, AppError> {
            Ok(vec![
                ColumnInfo {
                    name: "id".to_string(),
                    column_type: "bigint".to_string(),
                    nullable: false,
                    key: Some("PRI".to_string()),
                    default: None,
                    comment: None,
                },
                ColumnInfo {
                    name: "email".to_string(),
                    column_type: "varchar(255)".to_string(),
                    nullable: false,
                    key: Some("UNI".to_string()),
                    default: None,
                    comment: None,
                },
            ])
        }

        async fn indexes(
            &self,
            _policy: &MountPolicy,
            _schema: &str,
            _table: &str,
        ) -> Result<Vec<IndexInfo>, AppError> {
            Ok(vec![
                IndexInfo {
                    name: "PRIMARY".to_string(),
                    columns: vec!["id".to_string()],
                    unique: true,
                    primary: true,
                },
                IndexInfo {
                    name: "idx_user_id".to_string(),
                    columns: vec!["user_id".to_string()],
                    unique: false,
                    primary: false,
                },
                IndexInfo {
                    name: "uniq_email".to_string(),
                    columns: vec!["email".to_string()],
                    unique: true,
                    primary: false,
                },
            ])
        }

        async fn foreign_keys(
            &self,
            _policy: &MountPolicy,
            _schema: &str,
            _table: &str,
        ) -> Result<Vec<ForeignKeyInfo>, AppError> {
            Ok(Vec::new())
        }

        async fn sample_rows(
            &self,
            policy: &MountPolicy,
            _schema: &str,
            _table: &str,
        ) -> Result<Vec<Value>, AppError> {
            Ok(vec![crate::mounts::policy::redact_row(
                policy,
                json!({"id": 123, "email": "alice@example.com"}),
            )])
        }

        async fn lookup_rows(
            &self,
            policy: &MountPolicy,
            _schema: &str,
            _table: &str,
            _column: &str,
            _value: &str,
            _max_rows: usize,
        ) -> Result<Vec<Value>, AppError> {
            self.sample_rows(policy, "app", "users").await
        }

        async fn count(
            &self,
            _policy: &MountPolicy,
            _schema: &str,
            _table: &str,
        ) -> Result<Option<u64>, AppError> {
            Ok(Some(1))
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "csm-router-{prefix}-{}-{stamp}",
            std::process::id()
        ))
    }
}
