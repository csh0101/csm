use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type MountKey = String;

pub fn mount_key(project_id: &str, mount_id: &str) -> MountKey {
    format!("{project_id}::{mount_id}")
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectorKind {
    Mysql,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MountStatus {
    Stopped,
    Starting,
    Running,
    Error,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountRecord {
    pub project_id: String,
    pub mount_id: String,
    pub connector_kind: ConnectorKind,
    pub display_name: String,
    pub mount_point: String,
    pub credential_profile_id: String,
    pub policy_id: String,
    pub status: MountStatus,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MountRecord {
    pub fn key(&self) -> MountKey {
        mount_key(&self.project_id, &self.mount_id)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialProfile {
    pub profile_id: String,
    pub project_id: String,
    pub kind: ConnectorKind,
    pub display_name: String,
    pub redacted_dsn: String,
    pub dsn_storage: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSecret {
    pub profile_id: String,
    pub project_id: String,
    pub kind: ConnectorKind,
    pub local_dsn: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountPolicy {
    pub policy_id: String,
    pub project_id: String,
    pub readonly: bool,
    pub allowed_schemas: Vec<String>,
    pub blocked_schemas: Vec<String>,
    pub allowed_tables: Vec<String>,
    pub blocked_tables: Vec<String>,
    pub max_sample_rows: usize,
    pub max_lookup_rows: usize,
    pub max_file_bytes: usize,
    pub query_timeout_ms: u64,
    pub redact_columns: Vec<String>,
    pub require_tenant_filter: bool,
    pub tenant_columns: Vec<String>,
    pub allow_addressable_lookups: bool,
    pub allow_custom_queries: bool,
    pub updated_at: DateTime<Utc>,
}

impl MountPolicy {
    pub fn default_for(project_id: impl Into<String>, policy_id: impl Into<String>) -> Self {
        Self {
            policy_id: policy_id.into(),
            project_id: project_id.into(),
            readonly: true,
            allowed_schemas: Vec::new(),
            blocked_schemas: ["mysql", "performance_schema", "information_schema", "sys"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            allowed_tables: Vec::new(),
            blocked_tables: Vec::new(),
            max_sample_rows: 100,
            max_lookup_rows: 100,
            max_file_bytes: 1024 * 1024,
            query_timeout_ms: 3000,
            redact_columns: [
                "password", "passwd", "pwd", "secret", "token", "api_key", "apikey", "email",
                "phone", "mobile", "address", "id_card", "ssn",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            require_tenant_filter: false,
            tenant_columns: ["tenant_id", "org_id", "workspace_id"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            allow_addressable_lookups: true,
            allow_custom_queries: false,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountAuditRecord {
    pub timestamp: DateTime<Utc>,
    pub project_id: String,
    pub mount_id: String,
    pub virtual_path: String,
    pub result: String,
    pub bytes_returned: usize,
    pub duration_ms: u128,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountStore {
    #[serde(default)]
    pub mounts: Vec<MountRecord>,
    #[serde(default)]
    pub policies: Vec<MountPolicy>,
    #[serde(default)]
    pub credential_profiles: Vec<CredentialProfile>,
    #[serde(default)]
    pub credential_secrets: Vec<CredentialSecret>,
    #[serde(default)]
    pub audit_events: Vec<MountAuditRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlDiscoveryCandidate {
    pub source: String,
    pub kind: ConnectorKind,
    pub redacted_dsn: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: String,
    pub nullable: bool,
    pub key: Option<String>,
    pub default: Option<Value>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyInfo {
    pub column: String,
    pub references: ForeignKeyReference,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyReference {
    pub schema: String,
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferredRelation {
    pub confidence: String,
    pub column: String,
    pub references: ForeignKeyReference,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupManifestEntry {
    pub name: String,
    pub path_template: String,
    pub query_shape: String,
    pub max_rows: usize,
}
