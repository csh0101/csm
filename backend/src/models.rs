use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Stale,
    Deleted,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub codex_session_id: Option<String>,
    pub name: String,
    pub excerpt: String,
    pub full_content: String,
    pub path: String,
    pub project_path: Option<String>,
    pub labels: Vec<String>,
    pub last_modified: DateTime<Utc>,
    pub size: u64,
    pub status: SessionStatus,
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub session_id: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub status_override: Option<SessionStatus>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRecord {
    pub session_id: String,
    pub source_path: String,
    pub archive_provider: String,
    pub archive_uri: String,
    pub archived_at: DateTime<Utc>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataFile {
    #[serde(default)]
    pub workspace_path: Option<String>,
    #[serde(default)]
    pub stale_after_days: Option<i64>,
    #[serde(default)]
    pub sessions: HashMap<String, SessionMeta>,
    #[serde(default)]
    pub archive_records: Vec<ArchiveRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterCounts {
    pub all: usize,
    pub recent: usize,
    pub stale: usize,
    pub unlabeled: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CollaborationSourceKind {
    LocalSimulated,
    LanPeer,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationSource {
    pub source_id: String,
    pub kind: CollaborationSourceKind,
    pub display_name: String,
    pub session_root: Option<String>,
    pub peer_id: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIdentity {
    pub project_id: String,
    pub root_path: Option<String>,
    pub path_label: String,
    pub git_remote_hash: Option<String>,
    pub git_branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePolicy {
    pub project_id: String,
    pub project_path: Option<String>,
    pub enabled: bool,
    pub shared_labels: Vec<String>,
    pub blocked_labels: Vec<String>,
    pub max_excerpt_chars: usize,
    pub max_delta_chars: usize,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RedactionStatus {
    Clean,
    Redacted,
    Blocked,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionResult {
    pub status: RedactionStatus,
    pub reasons: Vec<String>,
    pub redacted_text: String,
    pub original_char_count: usize,
    pub redacted_char_count: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDelta {
    pub delta_id: String,
    pub session_id: String,
    pub project_id: String,
    pub timestamp: DateTime<Utc>,
    pub role: String,
    pub kind: String,
    pub text_excerpt: String,
    pub paths_mentioned: Vec<String>,
    pub commands_mentioned: Vec<String>,
    pub git_refs: Vec<String>,
    pub redaction_result: RedactionResult,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDeltaCursor {
    pub source_id: String,
    pub session_path: String,
    pub last_offset: u64,
    pub last_record_timestamp: Option<DateTime<Utc>>,
    pub last_record_hash: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationEvidence {
    pub source_id: String,
    pub peer_id: Option<String>,
    pub session_id: String,
    pub delta_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub path: Option<String>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationSummary {
    pub summary_id: String,
    pub project_id: String,
    pub source_ids: Vec<String>,
    pub markdown: String,
    pub generated_at: DateTime<Utc>,
    pub active_since: DateTime<Utc>,
    pub engine: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CollaborationHintType {
    Boundary,
    Confirmation,
    Conflict,
    Info,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CollaborationHintSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CollaborationHintStatus {
    Unread,
    Read,
    Dismissed,
    Resolved,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationHint {
    pub hint_id: String,
    pub hint_type: CollaborationHintType,
    pub severity: CollaborationHintSeverity,
    pub project_id: String,
    pub title: String,
    pub summary: String,
    pub evidence: Vec<CollaborationEvidence>,
    pub status: CollaborationHintStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerMetadata {
    pub peer_id: String,
    pub display_name: String,
    pub trusted: bool,
    pub public_key: Option<String>,
    pub base_url: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerPresence {
    pub peer_id: String,
    pub service_name: String,
    pub display_name: String,
    pub version: Option<String>,
    pub base_url: String,
    pub host_name: String,
    pub port: u16,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionStatus {
    Requested,
    Approved,
    Active,
    Paused,
    Revoked,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub subscription_id: String,
    pub peer_id: String,
    pub project_id: String,
    pub status: SubscriptionStatus,
    pub topics: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub baseline_generated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationStore {
    pub schema_version: u32,
    pub local_peer: Option<PeerMetadata>,
    pub sources: Vec<CollaborationSource>,
    pub trusted_peers: Vec<PeerMetadata>,
    pub project_policies: Vec<SharePolicy>,
    pub subscriptions: Vec<Subscription>,
    pub delta_cursors: Vec<SessionDeltaCursor>,
    pub summaries: Vec<CollaborationSummary>,
    pub hints: Vec<CollaborationHint>,
}

impl Default for CollaborationStore {
    fn default() -> Self {
        Self {
            schema_version: 1,
            local_peer: None,
            sources: Vec::new(),
            trusted_peers: Vec::new(),
            project_policies: Vec::new(),
            subscriptions: Vec::new(),
            delta_cursors: Vec::new(),
            summaries: Vec::new(),
            hints: Vec::new(),
        }
    }
}
