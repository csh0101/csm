use serde_json::{Map, Value};

use crate::mounts::models::MountPolicy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryPolicyError {
    CustomSqlDisabled,
    JoinDisabled,
}

pub fn schema_allowed(policy: &MountPolicy, schema: &str) -> bool {
    let schema = schema.to_ascii_lowercase();
    if policy
        .blocked_schemas
        .iter()
        .any(|blocked| blocked.eq_ignore_ascii_case(&schema))
    {
        return false;
    }

    policy.allowed_schemas.is_empty()
        || policy
            .allowed_schemas
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&schema))
}

pub fn table_allowed(policy: &MountPolicy, schema: &str, table: &str) -> bool {
    if !schema_allowed(policy, schema) {
        return false;
    }

    let qualified = format!("{schema}.{table}");
    if policy.blocked_tables.iter().any(|blocked| {
        blocked.eq_ignore_ascii_case(table) || blocked.eq_ignore_ascii_case(&qualified)
    }) {
        return false;
    }

    policy.allowed_tables.is_empty()
        || policy.allowed_tables.iter().any(|allowed| {
            allowed.eq_ignore_ascii_case(table) || allowed.eq_ignore_ascii_case(&qualified)
        })
}

pub fn authorize_custom_query(policy: &MountPolicy, sql: &str) -> Result<(), QueryPolicyError> {
    if !policy.allow_custom_queries {
        return Err(QueryPolicyError::CustomSqlDisabled);
    }
    if contains_join(sql) {
        return Err(QueryPolicyError::JoinDisabled);
    }
    Ok(())
}

pub fn contains_join(sql: &str) -> bool {
    sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .any(|token| token.eq_ignore_ascii_case("join"))
}

pub fn is_sensitive_column(policy: &MountPolicy, column: &str) -> bool {
    let normalized = normalize_column_name(column);
    policy
        .redact_columns
        .iter()
        .map(|column| normalize_column_name(column))
        .any(|sensitive| normalized == sensitive || normalized.contains(&sensitive))
}

pub fn is_large_json_column(column: &str) -> bool {
    matches!(
        normalize_column_name(column).as_str(),
        "payload" | "metadata" | "extra" | "raw_json"
    )
}

pub fn redact_row(policy: &MountPolicy, row: Value) -> Value {
    match row {
        Value::Object(fields) => Value::Object(redact_object(policy, fields)),
        other => other,
    }
}

pub fn redact_object(policy: &MountPolicy, fields: Map<String, Value>) -> Map<String, Value> {
    fields
        .into_iter()
        .map(|(column, value)| {
            let redacted = if is_sensitive_column(policy, &column) {
                sensitive_placeholder(&column)
            } else if is_large_json_column(&column) {
                redact_large_json_value(value)
            } else {
                value
            };
            (column, redacted)
        })
        .collect()
}

pub fn top_values_allowed(policy: &MountPolicy, column: &str, distinct_estimate: Option<u64>) -> bool {
    !is_sensitive_column(policy, column)
        && !is_large_json_column(column)
        && distinct_estimate.is_none_or(|estimate| estimate <= 100)
}

fn redact_large_json_value(value: Value) -> Value {
    match value {
        Value::Null => Value::Null,
        _ => Value::String("[redacted-large-json]".to_string()),
    }
}

fn sensitive_placeholder(column: &str) -> Value {
    let normalized = normalize_column_name(column);
    let marker = if normalized.contains("email") {
        "[redacted-email]"
    } else if normalized.contains("phone") || normalized.contains("mobile") {
        "[redacted-phone]"
    } else {
        "[redacted]"
    };
    Value::String(marker.to_string())
}

fn normalize_column_name(column: &str) -> String {
    column
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn policy_blocks_custom_sql_and_join() {
        let mut policy = MountPolicy::default_for("project_a", "policy_a");

        assert_eq!(
            authorize_custom_query(&policy, "select * from users").unwrap_err(),
            QueryPolicyError::CustomSqlDisabled
        );

        policy.allow_custom_queries = true;
        assert_eq!(
            authorize_custom_query(&policy, "select * from users join orders on users.id = orders.user_id")
                .unwrap_err(),
            QueryPolicyError::JoinDisabled
        );
    }

    #[test]
    fn default_policy_redacts_sensitive_and_large_json_columns() {
        let policy = MountPolicy::default_for("project_a", "policy_a");
        let row = json!({
            "id": 1,
            "email": "alice@example.com",
            "api_token": "secret-token",
            "payload": {"deep": "data"}
        });

        assert_eq!(
            redact_row(&policy, row),
            json!({
                "id": 1,
                "email": "[redacted-email]",
                "api_token": "[redacted]",
                "payload": "[redacted-large-json]"
            })
        );
    }

    #[test]
    fn top_values_blocks_sensitive_and_high_cardinality_columns() {
        let policy = MountPolicy::default_for("project_a", "policy_a");

        assert!(!top_values_allowed(&policy, "email", Some(5)));
        assert!(!top_values_allowed(&policy, "status", Some(10_000)));
        assert!(top_values_allowed(&policy, "status", Some(4)));
    }
}
