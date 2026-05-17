use std::{collections::BTreeMap, fs, path::Path, time::Duration};

use async_trait::async_trait;
use mysql_async::{Opts, Pool, Value as MysqlValue, params, prelude::*};
use serde_json::{Value, json};
use tokio::time::timeout;

use crate::{
    error::AppError,
    mounts::{
        models::{
            ColumnInfo, ConnectorKind, ForeignKeyInfo, ForeignKeyReference, IndexInfo,
            InferredRelation, MountPolicy, MysqlDiscoveryCandidate,
        },
        policy,
    },
};

#[derive(Debug, Clone)]
pub struct MysqlHealth {
    pub ok: bool,
    pub message: String,
}

#[async_trait]
pub trait MysqlConnector: Send + Sync {
    async fn health(&self, policy: &MountPolicy) -> Result<MysqlHealth, AppError>;
    async fn schemas(&self, policy: &MountPolicy) -> Result<Vec<String>, AppError>;
    async fn tables(&self, policy: &MountPolicy, schema: &str) -> Result<Vec<String>, AppError>;
    async fn schema_sql(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<String, AppError>;
    async fn columns(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, AppError>;
    async fn indexes(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Vec<IndexInfo>, AppError>;
    async fn foreign_keys(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, AppError>;
    async fn sample_rows(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Vec<Value>, AppError>;
    async fn lookup_rows(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
        column: &str,
        value: &str,
        max_rows: usize,
    ) -> Result<Vec<Value>, AppError>;
    async fn count(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Option<u64>, AppError>;
}

#[derive(Debug, Clone)]
pub struct LiveMysqlConnector {
    dsn: String,
}

impl LiveMysqlConnector {
    pub fn new(dsn: impl Into<String>) -> Self {
        Self { dsn: dsn.into() }
    }

    fn pool(&self) -> Result<Pool, AppError> {
        let opts =
            Opts::from_url(&self.dsn).map_err(|error| AppError::BadRequest(error.to_string()))?;
        Ok(Pool::new(opts))
    }

    async fn with_conn<T, F, Fut>(&self, policy: &MountPolicy, f: F) -> Result<T, AppError>
    where
        T: Send,
        F: FnOnce(mysql_async::Conn) -> Fut + Send,
        Fut: std::future::Future<Output = Result<(mysql_async::Conn, T), AppError>> + Send,
    {
        let pool = self.pool()?;
        let mut conn = timeout(
            Duration::from_millis(policy.query_timeout_ms),
            pool.get_conn(),
        )
        .await
        .map_err(|_| AppError::External("MySQL connection timed out".to_string()))?
        .map_err(|error| AppError::External(error.to_string()))?;
        let result = timeout(Duration::from_millis(policy.query_timeout_ms), f(conn))
            .await
            .map_err(|_| AppError::External("MySQL query timed out".to_string()))?;
        let (returned_conn, value) = result?;
        conn = returned_conn;
        let disconnect = pool.disconnect().await;
        drop(conn);
        if let Err(error) = disconnect {
            tracing::debug!("failed to disconnect MySQL pool cleanly: {error}");
        }
        Ok(value)
    }
}

#[async_trait]
impl MysqlConnector for LiveMysqlConnector {
    async fn health(&self, policy: &MountPolicy) -> Result<MysqlHealth, AppError> {
        self.with_conn(policy, |mut conn| async move {
            conn.query_drop("SELECT 1")
                .await
                .map_err(|error| AppError::External(error.to_string()))?;
            Ok((
                conn,
                MysqlHealth {
                    ok: true,
                    message: "ok".to_string(),
                },
            ))
        })
        .await
    }

    async fn schemas(&self, policy: &MountPolicy) -> Result<Vec<String>, AppError> {
        self.with_conn(policy, |mut conn| {
            let policy = policy.clone();
            async move {
                let mut schemas: Vec<String> = conn
                    .query("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME")
                    .await
                    .map_err(|error| AppError::External(error.to_string()))?;
                schemas.retain(|schema| policy::schema_allowed(&policy, schema));
                Ok((conn, schemas))
            }
        })
        .await
    }

    async fn tables(&self, policy: &MountPolicy, schema: &str) -> Result<Vec<String>, AppError> {
        ensure_schema(policy, schema)?;
        let schema = schema.to_string();
        self.with_conn(policy, |mut conn| {
            let policy = policy.clone();
            async move {
                let mut tables: Vec<String> = conn
                    .exec(
                        "SELECT TABLE_NAME FROM information_schema.TABLES \
                         WHERE TABLE_SCHEMA = :schema AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME",
                        params! { "schema" => &schema },
                    )
                    .await
                    .map_err(|error| AppError::External(error.to_string()))?;
                tables.retain(|table| policy::table_allowed(&policy, &schema, table));
                Ok((conn, tables))
            }
        })
        .await
    }

    async fn schema_sql(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<String, AppError> {
        ensure_table(policy, schema, table)?;
        let query = format!(
            "SHOW CREATE TABLE {}.{}",
            quote_identifier(schema),
            quote_identifier(table)
        );
        self.with_conn(policy, |mut conn| async move {
            let row: Option<(String, String)> = conn
                .query_first(query)
                .await
                .map_err(|error| AppError::External(error.to_string()))?;
            Ok((
                conn,
                row.map(|(_, sql)| format!("{sql};\n"))
                    .ok_or_else(|| AppError::NotFound("table schema not found".to_string()))?,
            ))
        })
        .await
    }

    async fn columns(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, AppError> {
        ensure_table(policy, schema, table)?;
        let schema = schema.to_string();
        let table = table.to_string();
        self.with_conn(policy, |mut conn| async move {
            let columns = conn
                .exec_map(
                    "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, COLUMN_COMMENT \
                     FROM information_schema.COLUMNS \
                     WHERE TABLE_SCHEMA = :schema AND TABLE_NAME = :table \
                     ORDER BY ORDINAL_POSITION",
                    params! { "schema" => &schema, "table" => &table },
                    |(name, column_type, nullable, key, default_value, comment): (
                        String,
                        String,
                        String,
                        String,
                        Option<String>,
                        String,
                    )| ColumnInfo {
                        name,
                        column_type,
                        nullable: nullable.eq_ignore_ascii_case("YES"),
                        key: if key.is_empty() { None } else { Some(key) },
                        default: default_value.map(Value::String),
                        comment: if comment.is_empty() { None } else { Some(comment) },
                    },
                )
                .await
                .map_err(|error| AppError::External(error.to_string()))?;
            Ok((conn, columns))
        })
        .await
    }

    async fn indexes(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Vec<IndexInfo>, AppError> {
        ensure_table(policy, schema, table)?;
        let schema = schema.to_string();
        let table = table.to_string();
        self.with_conn(policy, |mut conn| async move {
            let rows: Vec<(String, String, u64)> = conn
                .exec(
                    "SELECT INDEX_NAME, COLUMN_NAME, NON_UNIQUE \
                     FROM information_schema.STATISTICS \
                     WHERE TABLE_SCHEMA = :schema AND TABLE_NAME = :table \
                     ORDER BY INDEX_NAME, SEQ_IN_INDEX",
                    params! { "schema" => &schema, "table" => &table },
                )
                .await
                .map_err(|error| AppError::External(error.to_string()))?;
            let mut grouped: BTreeMap<String, IndexInfo> = BTreeMap::new();
            for (name, column, non_unique) in rows {
                let entry = grouped.entry(name.clone()).or_insert_with(|| IndexInfo {
                    name: name.clone(),
                    columns: Vec::new(),
                    unique: non_unique == 0,
                    primary: name == "PRIMARY",
                });
                entry.columns.push(column);
            }
            Ok((conn, grouped.into_values().collect()))
        })
        .await
    }

    async fn foreign_keys(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, AppError> {
        ensure_table(policy, schema, table)?;
        let schema = schema.to_string();
        let table = table.to_string();
        self.with_conn(policy, |mut conn| async move {
            let rows = conn
                .exec_map(
                    "SELECT COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
                     FROM information_schema.KEY_COLUMN_USAGE \
                     WHERE TABLE_SCHEMA = :schema AND TABLE_NAME = :table \
                       AND REFERENCED_TABLE_SCHEMA IS NOT NULL \
                     ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
                    params! { "schema" => &schema, "table" => &table },
                    |(column, ref_schema, ref_table, ref_column): (String, String, String, String)| {
                        ForeignKeyInfo {
                            column,
                            references: ForeignKeyReference {
                                schema: ref_schema,
                                table: ref_table,
                                column: ref_column,
                            },
                        }
                    },
                )
                .await
                .map_err(|error| AppError::External(error.to_string()))?;
            Ok((conn, rows))
        })
        .await
    }

    async fn sample_rows(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Vec<Value>, AppError> {
        ensure_table(policy, schema, table)?;
        let limit = policy.max_sample_rows.min(1000);
        json_rows(self, policy, schema, table, None, None, limit).await
    }

    async fn lookup_rows(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
        column: &str,
        value: &str,
        max_rows: usize,
    ) -> Result<Vec<Value>, AppError> {
        ensure_table(policy, schema, table)?;
        let limit = max_rows.min(policy.max_lookup_rows).min(1000);
        json_rows(self, policy, schema, table, Some(column), Some(value), limit).await
    }

    async fn count(
        &self,
        policy: &MountPolicy,
        schema: &str,
        table: &str,
    ) -> Result<Option<u64>, AppError> {
        ensure_table(policy, schema, table)?;
        let query = format!(
            "SELECT TABLE_ROWS FROM information_schema.TABLES WHERE TABLE_SCHEMA = :schema AND TABLE_NAME = :table"
        );
        let schema = schema.to_string();
        let table = table.to_string();
        self.with_conn(policy, |mut conn| async move {
            let count: Option<Option<u64>> = conn
                .exec_first(query, params! { "schema" => &schema, "table" => &table })
                .await
                .map_err(|error| AppError::External(error.to_string()))?;
            Ok((conn, count.flatten()))
        })
        .await
    }
}

pub fn infer_relations(columns: &[ColumnInfo], known_tables: &[String], schema: &str) -> Vec<InferredRelation> {
    columns
        .iter()
        .filter_map(|column| {
            let name = column.name.as_str();
            let stem = name.strip_suffix("_id")?;
            let plural = format!("{stem}s");
            let table = known_tables
                .iter()
                .find(|table| table.eq_ignore_ascii_case(&plural) || table.eq_ignore_ascii_case(stem))?;
            Some(InferredRelation {
                confidence: "medium".to_string(),
                column: column.name.clone(),
                references: ForeignKeyReference {
                    schema: schema.to_string(),
                    table: table.clone(),
                    column: "id".to_string(),
                },
                reason: format!("column name matches {}.id convention", table),
            })
        })
        .collect()
}

pub fn indexes_sql(indexes: &[IndexInfo], schema: &str, table: &str) -> String {
    if indexes.is_empty() {
        return "-- no indexes found\n".to_string();
    }

    indexes
        .iter()
        .map(|index| {
            if index.primary {
                format!(
                    "ALTER TABLE {}.{} ADD PRIMARY KEY ({});",
                    quote_identifier(schema),
                    quote_identifier(table),
                    quoted_columns(&index.columns)
                )
            } else if index.unique {
                format!(
                    "CREATE UNIQUE INDEX {} ON {}.{} ({});",
                    quote_identifier(&index.name),
                    quote_identifier(schema),
                    quote_identifier(table),
                    quoted_columns(&index.columns)
                )
            } else {
                format!(
                    "CREATE INDEX {} ON {}.{} ({});",
                    quote_identifier(&index.name),
                    quote_identifier(schema),
                    quote_identifier(table),
                    quoted_columns(&index.columns)
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn quoted_columns(columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ")
}

async fn json_rows(
    connector: &LiveMysqlConnector,
    policy: &MountPolicy,
    schema: &str,
    table: &str,
    where_column: Option<&str>,
    where_value: Option<&str>,
    limit: usize,
) -> Result<Vec<Value>, AppError> {
    let columns = connector.columns(policy, schema, table).await?;
    if columns.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(column) = where_column {
        if !columns.iter().any(|known| known.name == column) {
            return Err(AppError::NotFound(format!("column '{column}' was not found")));
        }
    }

    let json_args = columns
        .iter()
        .map(|column| {
            format!(
                "'{}', {}",
                column.name.replace('\\', "\\\\").replace('\'', "\\'"),
                quote_identifier(&column.name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut query = format!(
        "SELECT JSON_OBJECT({json_args}) AS row_json FROM {}.{}",
        quote_identifier(schema),
        quote_identifier(table)
    );
    if let Some(column) = where_column {
        query.push_str(&format!(" WHERE {} = :lookup_value", quote_identifier(column)));
    }
    query.push_str(" LIMIT :limit");

    let lookup_value = where_value.map(str::to_string);
    connector
        .with_conn(policy, move |mut conn| {
            let query = query.clone();
            async move {
                let rows: Vec<String> = conn
                    .exec_map(
                        query,
                        params! {
                            "lookup_value" => lookup_value,
                            "limit" => limit as u64,
                        },
                        mysql_value_to_string,
                    )
                    .await
                    .map_err(|error| AppError::External(error.to_string()))?;
                let values = rows
                    .into_iter()
                    .map(|raw| serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({ "raw": raw })))
                    .map(|row| policy::redact_row(policy, row))
                    .collect();
                Ok((conn, values))
            }
        })
        .await
}

fn mysql_value_to_string(value: MysqlValue) -> String {
    match value {
        MysqlValue::Bytes(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        MysqlValue::NULL => "null".to_string(),
        other => format!("{other:?}"),
    }
}

fn ensure_schema(policy: &MountPolicy, schema: &str) -> Result<(), AppError> {
    if policy::schema_allowed(policy, schema) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!("schema '{schema}' is blocked by policy")))
    }
}

fn ensure_table(policy: &MountPolicy, schema: &str, table: &str) -> Result<(), AppError> {
    if policy::table_allowed(policy, schema, table) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "table '{schema}.{table}' is blocked by policy"
        )))
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}

pub fn connection_summary(redacted_dsn: &str) -> Value {
    let Ok(url) = url::Url::parse(redacted_dsn) else {
        return json!({
            "kind": "mysql",
            "redactedDsn": redacted_dsn,
            "readonly": true,
            "redacted": true
        });
    };
    json!({
        "kind": "mysql",
        "host": url.host_str(),
        "port": url.port().unwrap_or(3306),
        "database": url.path().trim_start_matches('/'),
        "user": url.username(),
        "readonly": true,
        "redacted": true
    })
}

pub fn redact_dsn(dsn: &str) -> String {
    let Ok(mut url) = url::Url::parse(dsn.trim()) else {
        return "[redacted-dsn]".to_string();
    };
    let _ = url.set_password(None);
    url.to_string()
}

pub fn discover_mysql_candidates(project_root: &Path) -> Vec<MysqlDiscoveryCandidate> {
    let mut candidates = Vec::new();
    for file_name in [".env", ".env.local"] {
        let path = project_root.join(file_name);
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let env = parse_env_lines(&content);
        if let Some(dsn) = env
            .get("DATABASE_URL")
            .or_else(|| env.get("MYSQL_URL"))
            .filter(|dsn| dsn.trim_start().starts_with("mysql://"))
        {
            candidates.push(MysqlDiscoveryCandidate {
                source: file_name.to_string(),
                kind: ConnectorKind::Mysql,
                redacted_dsn: redact_dsn(dsn),
                confidence: "high".to_string(),
            });
        } else if let (Some(host), Some(database), Some(user)) = (
            env.get("MYSQL_HOST"),
            env.get("MYSQL_DATABASE"),
            env.get("MYSQL_USER"),
        ) {
            let port = env.get("MYSQL_PORT").map(String::as_str).unwrap_or("3306");
            candidates.push(MysqlDiscoveryCandidate {
                source: file_name.to_string(),
                kind: ConnectorKind::Mysql,
                redacted_dsn: format!("mysql://{user}@{host}:{port}/{database}"),
                confidence: "medium".to_string(),
            });
        }
    }
    candidates
}

fn parse_env_lines(content: &str) -> BTreeMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((
                key.trim().to_string(),
                value.trim().trim_matches('"').trim_matches('\'').to_string(),
            ))
        })
        .collect()
}

pub fn rows_to_jsonl(rows: &[Value]) -> String {
    rows.iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + if rows.is_empty() { "" } else { "\n" }
}

pub fn single_row_json(rows: &[Value]) -> Value {
    rows.first().cloned().unwrap_or(Value::Null)
}

pub fn row_list_json(rows: Vec<Value>) -> Value {
    Value::Array(rows)
}

pub fn manifest_from_indexes(indexes: &[IndexInfo], policy: &MountPolicy) -> Vec<crate::mounts::models::LookupManifestEntry> {
    let mut entries = Vec::new();
    for index in indexes {
        if index.columns.len() != 1 {
            continue;
        }
        let column = &index.columns[0];
        if policy::is_sensitive_column(policy, column) {
            continue;
        }
        if index.primary {
            entries.push(crate::mounts::models::LookupManifestEntry {
                name: format!("{}_by_{}", "row", column),
                path_template: format!("lookup/by-primary/{column}/{{value}}.json"),
                query_shape: "primary-key".to_string(),
                max_rows: 1,
            });
        } else if index.unique {
            entries.push(crate::mounts::models::LookupManifestEntry {
                name: format!("row_by_unique_{column}"),
                path_template: format!("lookup/by-unique/{column}/{{value}}.json"),
                query_shape: "unique-key".to_string(),
                max_rows: 1,
            });
        } else {
            entries.push(crate::mounts::models::LookupManifestEntry {
                name: format!("rows_by_index_{column}"),
                path_template: format!("lookup/by-index/{column}/{{value}}.jsonl"),
                query_shape: "index-lookup".to_string(),
                max_rows: policy.max_lookup_rows,
            });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_dsn_removes_password() {
        let redacted = redact_dsn("mysql://readonly:secret-password@127.0.0.1:3306/app");

        assert_eq!(redacted, "mysql://readonly@127.0.0.1:3306/app");
        assert!(!redacted.contains("secret-password"));
    }
}
