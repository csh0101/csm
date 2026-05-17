use std::collections::{HashMap, HashSet};
use std::path::{Path as FsPath, PathBuf};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, patch, post},
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    archive,
    collaboration::{self, PeerDeltasQuery, PeerSessionsQuery},
    config::Config,
    discovery,
    error::AppError,
    models::{
        AnalysisCycle, ArchiveRecord, CollaborationSource, CollaborationSourceKind,
        CollaborationStore, CollaborationSummary, FilterCounts, LabelCount, MetadataFile,
        PeerMetadata, PeerPresence, ProjectIdentity, Session, SessionDeltaCursor, SessionMeta,
        SessionStatus, SharePolicy, Subscription, SubscriptionStatus,
    },
    mounts::{
        fuse,
        models::{
            ConnectorKind, CredentialProfile, CredentialSecret, MountPolicy, MountRecord,
            MountStatus, MountStore, MysqlDiscoveryCandidate,
        },
        mysql::{self, LiveMysqlConnector, MysqlConnector},
        storage as mount_storage,
    },
    project, scanner,
    state::SharedState,
    storage,
    summary::{self, ActivitySummaryRequest, ActivitySummaryResponse},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PeerHealthResponse {
    status: String,
    service: String,
    peer_id: String,
    display_name: String,
    base_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsResponse {
    pub workspace_path: Option<String>,
    pub sessions: Vec<Session>,
    pub counts: FilterCounts,
    pub labels: Vec<LabelCount>,
    pub stale_after_days: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResponse {
    pub workspace_path: String,
    pub sessions: Vec<Session>,
    pub counts: FilterCounts,
    pub labels: Vec<LabelCount>,
    pub stale_after_days: i64,
    pub skipped_files: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsRequest {
    pub stale_after_days: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLabelsRequest {
    pub labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotesRequest {
    pub notes: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMutationResponse {
    pub session: Session,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_record: Option<ArchiveRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationStateResponse {
    pub store: CollaborationStore,
    pub projects: Vec<ProjectIdentity>,
    pub discovered_peers: Vec<PeerPresence>,
    pub local_config: LocalCollaborationConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveProjectRequest {
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveProjectResponse {
    pub project: ProjectIdentity,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverMysqlMountRequest {
    pub project_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverMysqlMountResponse {
    pub project_id: String,
    pub candidates: Vec<MysqlDiscoveryCandidate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMountRequest {
    pub connector_kind: ConnectorKind,
    pub mount_id: String,
    pub display_name: Option<String>,
    pub dsn: String,
    pub mount_point_mode: Option<String>,
    pub mount_point: Option<String>,
    pub project_path: Option<String>,
    pub policy: Option<MountPolicyPatch>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MountPolicyPatch {
    pub readonly: Option<bool>,
    pub allowed_schemas: Option<Vec<String>>,
    pub blocked_schemas: Option<Vec<String>>,
    pub allowed_tables: Option<Vec<String>>,
    pub blocked_tables: Option<Vec<String>>,
    pub max_sample_rows: Option<usize>,
    pub max_lookup_rows: Option<usize>,
    pub max_file_bytes: Option<usize>,
    pub query_timeout_ms: Option<u64>,
    pub redact_columns: Option<Vec<String>>,
    pub require_tenant_filter: Option<bool>,
    pub tenant_columns: Option<Vec<String>>,
    pub allow_addressable_lookups: Option<bool>,
    pub allow_custom_queries: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountResponse {
    pub mount: MountRecord,
    pub policy: MountPolicy,
    pub credential: CredentialProfile,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountListResponse {
    pub mounts: Vec<MountResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountActionResponse {
    pub mount: MountRecord,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCollaborationConfig {
    pub peer_id: String,
    pub display_name: String,
    pub base_url: String,
    pub bind_address: String,
    pub peer_token: Option<String>,
    pub peer_token_configured: bool,
    pub lan_discovery_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSharePolicyRequest {
    pub project_path: Option<String>,
    pub enabled: Option<bool>,
    pub shared_labels: Option<Vec<String>>,
    pub blocked_labels: Option<Vec<String>>,
    pub max_excerpt_chars: Option<usize>,
    pub max_delta_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLocalCollaborationConfigRequest {
    pub display_name: Option<String>,
    pub peer_token: Option<String>,
    pub refresh_peer_token: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairPeerRequest {
    pub peer_base_url: String,
    pub peer_access_token: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairPeerResponse {
    pub state: CollaborationStateResponse,
    pub peer: PeerMetadata,
    pub peer_projects: Vec<collaboration::PeerProject>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerProjectsRequest {
    pub peer_access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePeerAccessTokenRequest {
    pub peer_access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscriptionRequest {
    pub peer_id: Option<String>,
    pub peer_base_url: Option<String>,
    pub peer_access_token: Option<String>,
    pub project_id: String,
    pub days: Option<i64>,
    pub language: Option<String>,
    pub topics: Option<Vec<String>>,
    pub analysis_cycle: Option<AnalysisCycle>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscriptionResponse {
    pub state: CollaborationStateResponse,
    pub subscription: Subscription,
    pub summary: CollaborationSummary,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalSummaryRequest {
    pub subscription_id: String,
    pub peer_access_token: Option<String>,
    pub since: Option<chrono::DateTime<Utc>>,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSubscriptionScheduleRequest {
    pub analysis_cycle: AnalysisCycle,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalSummaryResponse {
    pub state: CollaborationStateResponse,
    pub summary: CollaborationSummary,
}

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/peer/health", get(peer_health))
        .route("/peer/projects", get(peer_projects))
        .route("/peer/sessions", get(peer_sessions))
        .route("/peer/sessions/{id}", get(peer_session_detail))
        .route("/peer/sessions/{id}/deltas", get(peer_session_deltas))
        .route(
            "/peer/streams/session-deltas",
            get(peer_session_delta_stream),
        )
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/scan", post(scan_sessions))
        .route("/api/settings", patch(update_settings))
        .route("/api/sessions/{id}/labels", patch(update_labels))
        .route("/api/sessions/{id}/notes", patch(update_notes))
        .route("/api/summaries/activity", post(generate_activity_summary))
        .route("/api/projects", get(list_projects))
        .route("/api/projects/resolve", post(resolve_project))
        .route(
            "/api/projects/{projectId}/mounts/mysql/discover",
            post(discover_mysql_mounts),
        )
        .route(
            "/api/projects/{projectId}/mounts",
            get(list_project_mounts).post(create_project_mount),
        )
        .route(
            "/api/projects/{projectId}/mounts/{mountId}",
            get(get_project_mount),
        )
        .route(
            "/api/projects/{projectId}/mounts/{mountId}/start",
            post(start_project_mount),
        )
        .route(
            "/api/projects/{projectId}/mounts/{mountId}/stop",
            post(stop_project_mount),
        )
        .route(
            "/api/projects/{projectId}/mounts/{mountId}/policy",
            patch(update_project_mount_policy),
        )
        .route("/api/collaboration", get(get_collaboration_state))
        .route(
            "/api/collaboration/local-config",
            patch(update_local_collaboration_config),
        )
        .route(
            "/api/collaboration/share-policies/{projectId}",
            patch(update_share_policy),
        )
        .route("/api/collaboration/peers/pair", post(pair_peer))
        .route(
            "/api/collaboration/subscriptions/{subscriptionId}/schedule",
            patch(update_subscription_schedule),
        )
        .route(
            "/api/collaboration/peers/{peerId}/token",
            patch(update_trusted_peer_token),
        )
        .route(
            "/api/collaboration/peers/{peerId}/projects",
            post(trusted_peer_projects),
        )
        .route(
            "/api/collaboration/subscriptions",
            post(create_subscription),
        )
        .route(
            "/api/collaboration/summaries/baseline",
            post(generate_collaboration_baseline),
        )
        .route(
            "/api/collaboration/summaries/incremental",
            post(generate_collaboration_incremental),
        )
        .route(
            "/api/sessions/{id}/archive-delete",
            post(archive_delete_session),
        )
        .route("/api/sessions/{id}/restore", post(restore_session))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "traceway-backend",
    })
}

async fn peer_health(State(state): State<SharedState>) -> Json<PeerHealthResponse> {
    let peer = {
        let inner = state.inner.read().await;
        inner.collaboration.local_peer.clone()
    };
    let fallback_base_url = format!("http://{}", state.config.bind_addr);
    let peer = peer.unwrap_or_else(|| PeerMetadata {
        peer_id: collaboration::peer_id_for_base_url(&fallback_base_url),
        display_name: state.config.peer_display_name.clone(),
        trusted: true,
        public_key: None,
        base_url: Some(fallback_base_url),
        last_seen_at: Some(Utc::now()),
        access_token: None,
    });

    Json(PeerHealthResponse {
        status: "ok".to_string(),
        service: "traceway-peer".to_string(),
        peer_id: peer.peer_id,
        display_name: peer.display_name,
        base_url: peer.base_url,
    })
}

async fn update_local_collaboration_config(
    State(state): State<SharedState>,
    Json(request): Json<UpdateLocalCollaborationConfigRequest>,
) -> Result<Json<CollaborationStateResponse>, AppError> {
    let display_name = request
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|display_name| !display_name.is_empty())
        .map(|display_name| display_name.chars().take(80).collect::<String>());
    if request.display_name.is_some() && display_name.is_none() {
        return Err(AppError::BadRequest(
            "displayName must not be empty".to_string(),
        ));
    }
    let requested_peer_token =
        normalize_optional_text(request.peer_token.clone().unwrap_or_default());
    if request.peer_token.is_some() && requested_peer_token.is_none() {
        return Err(AppError::BadRequest(
            "peerToken must not be empty".to_string(),
        ));
    }

    let (peer, display_name_changed) = {
        let mut inner = state.inner.write().await;
        let base_url = format!("http://{}", state.config.bind_addr);
        let display_name = display_name
            .clone()
            .or_else(|| {
                inner
                    .collaboration
                    .local_peer
                    .as_ref()
                    .map(|peer| peer.display_name.clone())
            })
            .unwrap_or_else(|| state.config.peer_display_name.clone());
        let display_name_changed = inner
            .collaboration
            .local_peer
            .as_ref()
            .is_none_or(|peer| peer.display_name != display_name);
        let peer = collaboration::ensure_local_peer(
            &mut inner.collaboration,
            display_name.clone(),
            base_url,
        );
        if request.refresh_peer_token.unwrap_or(false) {
            inner.collaboration.local_peer_token = Some(collaboration::generate_peer_token());
        } else if let Some(peer_token) = requested_peer_token {
            inner.collaboration.local_peer_token = Some(peer_token);
        } else {
            collaboration::ensure_local_peer_token(
                &mut inner.collaboration,
                state.config.peer_token.clone(),
            );
        }
        storage::save_collaboration_store(&state.config.collaboration_path, &inner.collaboration)?;
        (peer, display_name_changed)
    };

    if state.config.lan_discovery_enabled && display_name_changed {
        {
            let mut discovery = state
                .lan_discovery
                .lock()
                .expect("LAN discovery lock poisoned");
            let previous = discovery.take();
            drop(discovery);
            drop(previous);
        }

        if let Some(handle) = discovery::start(
            state.clone(),
            peer.peer_id.clone(),
            peer.display_name.clone(),
        )? {
            *state
                .lan_discovery
                .lock()
                .expect("LAN discovery lock poisoned") = Some(handle);
        }
    }

    let inner = state.inner.read().await;
    Ok(Json(collaboration_state_response(
        &inner.collaboration,
        &inner.sessions,
        &inner.peer_presence,
        &state.config,
    )))
}

async fn peer_projects(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Json<Vec<collaboration::PeerProject>>, AppError> {
    let (sessions, policies, token) = {
        let inner = state.inner.read().await;
        (
            inner.sessions.values().cloned().collect::<Vec<_>>(),
            inner.collaboration.project_policies.clone(),
            effective_local_peer_token(&inner.collaboration, &state.config),
        )
    };
    require_peer_token(&headers, token.as_deref())?;

    Ok(Json(collaboration::peer_projects(&sessions, &policies)))
}

async fn peer_sessions(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<PeerSessionsQuery>,
) -> Result<Json<Vec<collaboration::PeerSessionSummary>>, AppError> {
    let (sessions, policies, token) = {
        let inner = state.inner.read().await;
        (
            inner.sessions.values().cloned().collect::<Vec<_>>(),
            inner.collaboration.project_policies.clone(),
            effective_local_peer_token(&inner.collaboration, &state.config),
        )
    };
    require_peer_token(&headers, token.as_deref())?;

    Ok(Json(collaboration::peer_session_summaries(
        &sessions, &policies, &query,
    )))
}

async fn peer_session_detail(
    Path(id): Path<String>,
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Json<collaboration::PeerSessionDetail>, AppError> {
    let (session, policies, token) = {
        let inner = state.inner.read().await;
        let session = find_session_by_public_id(&inner.sessions, &id)
            .ok_or_else(|| AppError::NotFound(format!("session '{id}' was not found")))?;
        (
            session.clone(),
            inner.collaboration.project_policies.clone(),
            effective_local_peer_token(&inner.collaboration, &state.config),
        )
    };
    require_peer_token(&headers, token.as_deref())?;

    Ok(Json(collaboration::peer_session_detail(
        &session, &policies,
    )?))
}

async fn peer_session_deltas(
    Path(id): Path<String>,
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<PeerDeltasQuery>,
) -> Result<Json<collaboration::PeerDeltasResponse>, AppError> {
    let (session, policies, token) = {
        let inner = state.inner.read().await;
        let session = find_session_by_public_id(&inner.sessions, &id)
            .ok_or_else(|| AppError::NotFound(format!("session '{id}' was not found")))?;
        (
            session.clone(),
            inner.collaboration.project_policies.clone(),
            effective_local_peer_token(&inner.collaboration, &state.config),
        )
    };
    require_peer_token(&headers, token.as_deref())?;

    Ok(Json(collaboration::peer_session_deltas(
        &session, &policies, &query,
    )?))
}

async fn list_projects(State(state): State<SharedState>) -> Json<Vec<ProjectIdentity>> {
    let sessions = {
        let inner = state.inner.read().await;
        inner.sessions.values().cloned().collect::<Vec<_>>()
    };

    Json(collaboration_projects(&sessions))
}

async fn resolve_project(
    Json(request): Json<ResolveProjectRequest>,
) -> Json<ResolveProjectResponse> {
    Json(ResolveProjectResponse {
        project: project::project_identity_for_path(Some(&request.path)),
    })
}

async fn discover_mysql_mounts(
    Path(project_id): Path<String>,
    State(state): State<SharedState>,
    Json(request): Json<DiscoverMysqlMountRequest>,
) -> Result<Json<DiscoverMysqlMountResponse>, AppError> {
    let project_root =
        project_root_for_request(&state, &project_id, request.project_path.clone()).await?;
    Ok(Json(DiscoverMysqlMountResponse {
        project_id,
        candidates: mysql::discover_mysql_candidates(&project_root),
    }))
}

async fn list_project_mounts(
    Path(project_id): Path<String>,
    State(state): State<SharedState>,
) -> Result<Json<MountListResponse>, AppError> {
    let store = load_mount_store_for_state(&state)?;
    let mounts = store
        .mounts
        .iter()
        .filter(|mount| mount.project_id == project_id)
        .map(|mount| mount_response(&store, mount))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(MountListResponse { mounts }))
}

async fn get_project_mount(
    Path((project_id, mount_id)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> Result<Json<MountResponse>, AppError> {
    let store = load_mount_store_for_state(&state)?;
    let mount = mount_storage::find_mount(&store, &project_id, &mount_id)
        .ok_or_else(|| AppError::NotFound(format!("mount '{mount_id}' was not found")))?;
    Ok(Json(mount_response(&store, mount)?))
}

async fn create_project_mount(
    Path(project_id): Path<String>,
    State(state): State<SharedState>,
    Json(request): Json<CreateMountRequest>,
) -> Result<Json<MountResponse>, AppError> {
    if request.connector_kind != ConnectorKind::Mysql {
        return Err(AppError::BadRequest(
            "only mysql mounts are supported in the Phase 1 MVP".to_string(),
        ));
    }
    let mount_id = normalize_mount_id(&request.mount_id)?;
    let project_root =
        project_root_for_request(&state, &project_id, request.project_path.clone()).await?;
    let mount_point = resolve_mount_point(&project_root, &mount_id, &request)?;
    let now = Utc::now();
    let policy_id = format!("{project_id}:{mount_id}:policy");
    let credential_profile_id = format!("{project_id}:{mount_id}:credential");
    let mut policy = MountPolicy::default_for(project_id.clone(), policy_id.clone());
    if let Some(patch) = request.policy.clone() {
        apply_policy_patch(&mut policy, patch)?;
    }
    policy.updated_at = now;

    let display_name = request
        .display_name
        .clone()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| mount_id.clone());
    let credential = CredentialProfile {
        profile_id: credential_profile_id.clone(),
        project_id: project_id.clone(),
        kind: ConnectorKind::Mysql,
        display_name: display_name.clone(),
        redacted_dsn: mysql::redact_dsn(&request.dsn),
        dsn_storage: "local-plaintext-todo-keychain".to_string(),
        created_at: now,
        updated_at: now,
    };
    let secret = CredentialSecret {
        profile_id: credential_profile_id.clone(),
        project_id: project_id.clone(),
        kind: ConnectorKind::Mysql,
        local_dsn: request.dsn.clone(),
        created_at: now,
        updated_at: now,
    };
    let mount = MountRecord {
        project_id: project_id.clone(),
        mount_id: mount_id.clone(),
        connector_kind: ConnectorKind::Mysql,
        display_name,
        mount_point: mount_point.to_string_lossy().to_string(),
        credential_profile_id,
        policy_id,
        status: MountStatus::Stopped,
        last_health_check_at: None,
        last_error: None,
        created_at: now,
        updated_at: now,
    };

    let path = mount_storage::mounts_path(&state.config.data_dir);
    let mut store = mount_storage::load_mount_store(&path)?;
    mount_storage::upsert_mount(&mut store, mount.clone());
    mount_storage::upsert_policy(&mut store, policy.clone());
    mount_storage::upsert_credential_profile(&mut store, credential.clone());
    mount_storage::upsert_credential_secret(&mut store, secret);
    mount_storage::save_mount_store(&path, &store)?;

    Ok(Json(MountResponse {
        mount,
        policy,
        credential,
    }))
}

async fn start_project_mount(
    Path((project_id, mount_id)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> Result<Json<MountActionResponse>, AppError> {
    let path = mount_storage::mounts_path(&state.config.data_dir);
    let mut store = mount_storage::load_mount_store(&path)?;
    let mount = mount_storage::find_mount(&store, &project_id, &mount_id)
        .ok_or_else(|| AppError::NotFound(format!("mount '{mount_id}' was not found")))?
        .clone();
    let policy = mount_storage::find_policy(&store, &mount.policy_id)
        .ok_or_else(|| AppError::NotFound("mount policy was not found".to_string()))?
        .clone();
    let secret = mount_storage::find_credential_secret(&store, &mount.credential_profile_id)
        .ok_or_else(|| AppError::NotFound("mount credential secret was not found".to_string()))?
        .clone();

    let connector = LiveMysqlConnector::new(secret.local_dsn);
    let start_result = async {
        connector.health(&policy).await?;
        fuse::start_readonly_mount(&mount)?;
        Ok::<(), AppError>(())
    }
    .await;

    let now = Utc::now();
    let message = match start_result {
        Ok(()) => {
            let mount = mount_storage::find_mount_mut(&mut store, &project_id, &mount_id)
                .expect("mount exists");
            mount.status = MountStatus::Running;
            mount.last_health_check_at = Some(now);
            mount.last_error = None;
            mount.updated_at = now;
            None
        }
        Err(error) => {
            let message = error.to_string();
            let mount = mount_storage::find_mount_mut(&mut store, &project_id, &mount_id)
                .expect("mount exists");
            mount.status = MountStatus::Error;
            mount.last_health_check_at = Some(now);
            mount.last_error = Some(message.clone());
            mount.updated_at = now;
            Some(message)
        }
    };
    mount_storage::save_mount_store(&path, &store)?;
    let mount = mount_storage::find_mount(&store, &project_id, &mount_id)
        .expect("mount exists after start")
        .clone();

    Ok(Json(MountActionResponse { mount, message }))
}

async fn stop_project_mount(
    Path((project_id, mount_id)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> Result<Json<MountActionResponse>, AppError> {
    let path = mount_storage::mounts_path(&state.config.data_dir);
    let mut store = mount_storage::load_mount_store(&path)?;
    let mount = mount_storage::find_mount_mut(&mut store, &project_id, &mount_id)
        .ok_or_else(|| AppError::NotFound(format!("mount '{mount_id}' was not found")))?;
    let report = fuse::stop_readonly_mount(mount);
    mount.status = MountStatus::Stopped;
    mount.last_error = None;
    mount.updated_at = Utc::now();
    let mount = mount.clone();
    mount_storage::save_mount_store(&path, &store)?;
    state.mount_cache.clear_mount(&project_id, &mount_id);

    Ok(Json(MountActionResponse {
        mount,
        message: Some(report.message),
    }))
}

async fn update_project_mount_policy(
    Path((project_id, mount_id)): Path<(String, String)>,
    State(state): State<SharedState>,
    Json(request): Json<MountPolicyPatch>,
) -> Result<Json<MountResponse>, AppError> {
    let path = mount_storage::mounts_path(&state.config.data_dir);
    let mut store = mount_storage::load_mount_store(&path)?;
    let mount = mount_storage::find_mount(&store, &project_id, &mount_id)
        .ok_or_else(|| AppError::NotFound(format!("mount '{mount_id}' was not found")))?
        .clone();
    let policy = mount_storage::find_policy_mut(&mut store, &mount.policy_id)
        .ok_or_else(|| AppError::NotFound("mount policy was not found".to_string()))?;
    apply_policy_patch(policy, request)?;
    policy.updated_at = Utc::now();
    let response = mount_response(&store, &mount)?;
    mount_storage::save_mount_store(&path, &store)?;
    state.mount_cache.clear_mount(&project_id, &mount_id);

    Ok(Json(response))
}

async fn peer_session_delta_stream(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<PeerSessionsQuery>,
) -> Result<Json<Vec<collaboration::PeerDeltasResponse>>, AppError> {
    let (sessions, policies, token) = {
        let inner = state.inner.read().await;
        (
            inner.sessions.values().cloned().collect::<Vec<_>>(),
            inner.collaboration.project_policies.clone(),
            effective_local_peer_token(&inner.collaboration, &state.config),
        )
    };
    require_peer_token(&headers, token.as_deref())?;
    let visible_sessions = collaboration::visible_project_sessions(sessions.iter(), &policies)
        .into_iter()
        .filter(|(session, _, identity)| {
            query
                .project_id
                .as_ref()
                .is_none_or(|project_id| project_id == &identity.project_id)
                && query
                    .since
                    .is_none_or(|since| session.last_modified >= since)
        })
        .take(query.limit.unwrap_or(20).clamp(1, 100))
        .map(|(session, _, _)| {
            collaboration::peer_session_deltas(
                session,
                &policies,
                &PeerDeltasQuery {
                    since: query.since,
                    cursor: None,
                    limit: Some(100),
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(visible_sessions))
}

async fn list_sessions(State(state): State<SharedState>) -> Json<SessionsResponse> {
    let inner = state.inner.read().await;
    let mut sessions = inner.sessions.values().cloned().collect::<Vec<_>>();
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    let stale_after_days = inner.stale_after_days;

    Json(SessionsResponse {
        workspace_path: inner.workspace_path.clone(),
        counts: filter_counts(&sessions, stale_after_days),
        labels: label_counts(&sessions),
        sessions,
        stale_after_days,
    })
}

async fn scan_sessions(
    State(state): State<SharedState>,
    Json(request): Json<ScanRequest>,
) -> Result<Json<ScanResponse>, AppError> {
    let path = request.path.trim();
    if path.is_empty() {
        return Err(AppError::BadRequest(
            "workspace path cannot be empty".to_string(),
        ));
    }

    let (metadata, stale_after_days) = {
        let inner = state.inner.read().await;
        (inner.metadata.clone(), inner.stale_after_days)
    };

    let scan = scanner::scan_workspace(
        path,
        &metadata,
        state.config.max_preview_bytes,
        stale_after_days,
    )?;
    let mut sessions = scan.sessions;

    {
        let mut inner = state.inner.write().await;
        let previous_workspace_path = inner.metadata.workspace_path.clone();
        inner.metadata.workspace_path = Some(scan.workspace_path.clone());
        if let Err(error) = storage::save_metadata(&state.config.metadata_path, &inner.metadata) {
            inner.metadata.workspace_path = previous_workspace_path;
            return Err(error);
        }

        apply_metadata_overrides(&mut sessions, &inner.metadata);
        let sessions_by_id = sessions
            .iter()
            .cloned()
            .map(|session| (session.id.clone(), session))
            .collect();

        inner.workspace_path = Some(scan.workspace_path.clone());
        inner.sessions = sessions_by_id;
    }

    Ok(Json(ScanResponse {
        workspace_path: scan.workspace_path,
        counts: filter_counts(&sessions, stale_after_days),
        labels: label_counts(&sessions),
        sessions,
        stale_after_days,
        skipped_files: scan.skipped_files,
    }))
}

async fn update_settings(
    State(state): State<SharedState>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<SessionsResponse>, AppError> {
    let stale_after_days = request.stale_after_days;
    if !(1..=3650).contains(&stale_after_days) {
        return Err(AppError::BadRequest(
            "staleAfterDays must be between 1 and 3650".to_string(),
        ));
    }

    let mut sessions = {
        let mut inner = state.inner.write().await;
        let mut metadata = inner.metadata.clone();
        metadata.stale_after_days = Some(stale_after_days);

        let mut sessions = inner.sessions.values().cloned().collect::<Vec<_>>();
        apply_stale_threshold(&mut sessions, &metadata, stale_after_days);

        storage::save_metadata(&state.config.metadata_path, &metadata)?;

        inner.metadata = metadata;
        inner.stale_after_days = stale_after_days;
        inner.sessions = sessions
            .iter()
            .cloned()
            .map(|session| (session.id.clone(), session))
            .collect();

        sessions
    };
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    let workspace_path = {
        let inner = state.inner.read().await;
        inner.workspace_path.clone()
    };

    Ok(Json(SessionsResponse {
        workspace_path,
        counts: filter_counts(&sessions, stale_after_days),
        labels: label_counts(&sessions),
        sessions,
        stale_after_days,
    }))
}

async fn update_labels(
    Path(id): Path<String>,
    State(state): State<SharedState>,
    Json(request): Json<UpdateLabelsRequest>,
) -> Result<Json<SessionMutationResponse>, AppError> {
    let labels = normalize_labels(request.labels);
    let session = {
        let mut inner = state.inner.write().await;
        let mut session = inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("session '{id}' was not found")))?;

        session.labels = labels.clone();
        let mut metadata = inner.metadata.clone();
        let meta = ensure_meta(&mut metadata, &session);
        meta.labels = labels;
        meta.updated_at = Utc::now();
        storage::save_metadata(&state.config.metadata_path, &metadata)?;

        inner.metadata = metadata;
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    };

    Ok(Json(SessionMutationResponse {
        session,
        archive_record: None,
    }))
}

async fn update_notes(
    Path(id): Path<String>,
    State(state): State<SharedState>,
    Json(request): Json<UpdateNotesRequest>,
) -> Result<Json<SessionMutationResponse>, AppError> {
    let notes = request.notes;
    let session = {
        let mut inner = state.inner.write().await;
        let mut session = inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("session '{id}' was not found")))?;

        session.notes = notes.clone();
        let mut metadata = inner.metadata.clone();
        let meta = ensure_meta(&mut metadata, &session);
        meta.notes = notes;
        meta.updated_at = Utc::now();
        storage::save_metadata(&state.config.metadata_path, &metadata)?;

        inner.metadata = metadata;
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    };

    Ok(Json(SessionMutationResponse {
        session,
        archive_record: None,
    }))
}

async fn archive_delete_session(
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Result<Json<SessionMutationResponse>, AppError> {
    let session = {
        let inner = state.inner.read().await;
        inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("session '{id}' was not found")))?
    };

    let archive_record = archive::archive_session(&state.config, &session)?;
    let session = {
        let mut inner = state.inner.write().await;
        let mut session = inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("session '{id}' was not found")))?;

        session.status = SessionStatus::Deleted;
        let mut metadata = inner.metadata.clone();
        let meta = ensure_meta(&mut metadata, &session);
        meta.status_override = Some(SessionStatus::Deleted);
        meta.updated_at = Utc::now();
        metadata.archive_records.push(archive_record.clone());
        storage::save_metadata(&state.config.metadata_path, &metadata)?;

        inner.metadata = metadata;
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    };

    Ok(Json(SessionMutationResponse {
        session,
        archive_record: Some(archive_record),
    }))
}

async fn restore_session(
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Result<Json<SessionMutationResponse>, AppError> {
    let session = {
        let mut inner = state.inner.write().await;
        let mut session = inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("session '{id}' was not found")))?;

        session.status = SessionStatus::Active;
        let mut metadata = inner.metadata.clone();
        let meta = ensure_meta(&mut metadata, &session);
        meta.status_override = Some(SessionStatus::Active);
        meta.updated_at = Utc::now();
        storage::save_metadata(&state.config.metadata_path, &metadata)?;

        inner.metadata = metadata;
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    };

    Ok(Json(SessionMutationResponse {
        session,
        archive_record: None,
    }))
}

async fn generate_activity_summary(
    State(state): State<SharedState>,
    Json(request): Json<ActivitySummaryRequest>,
) -> Result<Json<ActivitySummaryResponse>, AppError> {
    let sessions = {
        let inner = state.inner.read().await;
        inner.sessions.values().cloned().collect::<Vec<_>>()
    };

    Ok(Json(
        summary::generate_activity_summary(sessions, request).await?,
    ))
}

async fn get_collaboration_state(
    State(state): State<SharedState>,
) -> Json<CollaborationStateResponse> {
    let (store, sessions, discovered_peers) = {
        let inner = state.inner.read().await;
        (
            inner.collaboration.clone(),
            inner.sessions.values().cloned().collect::<Vec<_>>(),
            sorted_peer_presence(&inner.peer_presence),
        )
    };

    Json(CollaborationStateResponse {
        local_config: local_collaboration_config(&store, &state.config),
        store,
        projects: collaboration_projects(&sessions),
        discovered_peers,
    })
}

async fn update_share_policy(
    Path(project_id): Path<String>,
    State(state): State<SharedState>,
    Json(request): Json<UpdateSharePolicyRequest>,
) -> Result<Json<CollaborationStateResponse>, AppError> {
    let mut inner = state.inner.write().await;
    let mut collaboration = inner.collaboration.clone();
    let policy = collaboration
        .project_policies
        .iter_mut()
        .find(|policy| policy.project_id == project_id);

    match policy {
        Some(policy) => merge_share_policy(policy, request),
        None => {
            let mut policy =
                collaboration::default_share_policy(project_id, request.project_path.clone());
            merge_share_policy(&mut policy, request);
            collaboration.project_policies.push(policy);
        }
    }

    storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)?;
    inner.collaboration = collaboration.clone();
    let projects = collaboration_projects(&inner.sessions.values().cloned().collect::<Vec<_>>());

    Ok(Json(CollaborationStateResponse {
        local_config: local_collaboration_config(&collaboration, &state.config),
        store: collaboration,
        projects,
        discovered_peers: sorted_peer_presence(&inner.peer_presence),
    }))
}

async fn pair_peer(
    State(state): State<SharedState>,
    Json(request): Json<PairPeerRequest>,
) -> Result<Json<PairPeerResponse>, AppError> {
    let peer_base_url = collaboration::normalize_peer_base_url(&request.peer_base_url)?;
    let access_token = normalize_optional_text(request.peer_access_token.unwrap_or_default());
    let peer_projects = fetch_peer_projects(&peer_base_url, access_token.as_deref()).await?;
    let peer_health = fetch_peer_health(&peer_base_url).await.ok();
    let now = Utc::now();
    let requested_display_name = normalize_optional_text(request.display_name.unwrap_or_default());

    let mut inner = state.inner.write().await;
    let discovered_peer = inner
        .peer_presence
        .values()
        .find(|presence| presence.base_url == peer_base_url)
        .cloned();
    let peer = PeerMetadata {
        peer_id: discovered_peer
            .as_ref()
            .map(|presence| presence.peer_id.clone())
            .or_else(|| peer_health.as_ref().map(|health| health.peer_id.clone()))
            .unwrap_or_else(|| collaboration::peer_id_for_base_url(&peer_base_url)),
        display_name: requested_display_name
            .or_else(|| discovered_peer.map(|presence| presence.display_name))
            .or_else(|| peer_health.map(|health| health.display_name))
            .unwrap_or_else(|| peer_base_url.clone()),
        trusted: true,
        public_key: None,
        base_url: Some(peer_base_url),
        last_seen_at: Some(now),
        access_token: access_token.clone(),
    };

    let mut collaboration = inner.collaboration.clone();
    upsert_trusted_peer(&mut collaboration, peer.clone());
    upsert_peer_source(&mut collaboration, &peer, now);
    storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)?;
    inner.collaboration = collaboration.clone();
    let state_response = collaboration_state_response(
        &collaboration,
        &inner.sessions,
        &inner.peer_presence,
        &state.config,
    );

    Ok(Json(PairPeerResponse {
        state: state_response,
        peer,
        peer_projects,
    }))
}

async fn update_trusted_peer_token(
    Path(peer_id): Path<String>,
    State(state): State<SharedState>,
    Json(request): Json<UpdatePeerAccessTokenRequest>,
) -> Result<Json<CollaborationStateResponse>, AppError> {
    let access_token = normalize_optional_text(request.peer_access_token.unwrap_or_default())
        .ok_or_else(|| AppError::BadRequest("peerAccessToken must not be empty".to_string()))?;

    let mut inner = state.inner.write().await;
    let mut collaboration = inner.collaboration.clone();
    let peer = collaboration
        .trusted_peers
        .iter_mut()
        .find(|peer| peer.peer_id == peer_id)
        .ok_or_else(|| AppError::NotFound(format!("paired peer '{peer_id}' was not found")))?;
    peer.access_token = Some(access_token);
    peer.last_seen_at = Some(Utc::now());

    storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)?;
    inner.collaboration = collaboration.clone();

    Ok(Json(collaboration_state_response(
        &collaboration,
        &inner.sessions,
        &inner.peer_presence,
        &state.config,
    )))
}

async fn trusted_peer_projects(
    Path(peer_id): Path<String>,
    State(state): State<SharedState>,
    Json(request): Json<PeerProjectsRequest>,
) -> Result<Json<Vec<collaboration::PeerProject>>, AppError> {
    let peer = {
        let inner = state.inner.read().await;
        inner
            .collaboration
            .trusted_peers
            .iter()
            .find(|peer| peer.peer_id == peer_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("paired peer '{peer_id}' was not found")))?
    };
    let peer_base_url = peer
        .base_url
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("paired peer has no baseUrl".to_string()))?;
    let access_token = peer_request_token(
        normalize_optional_text(request.peer_access_token.unwrap_or_default()),
        &peer,
    );
    let projects = fetch_peer_projects(peer_base_url, access_token.as_deref()).await?;
    mark_trusted_peer_seen(&state, &peer_id).await?;

    Ok(Json(projects))
}

async fn create_subscription(
    State(state): State<SharedState>,
    Json(request): Json<CreateSubscriptionRequest>,
) -> Result<Json<CreateSubscriptionResponse>, AppError> {
    if request.project_id.trim().is_empty() {
        return Err(AppError::BadRequest(
            "projectId cannot be empty".to_string(),
        ));
    }

    let (sessions, peer) = {
        let inner = state.inner.read().await;
        let peer = resolve_subscription_peer(&inner.collaboration, &request)?;
        (inner.sessions.values().cloned().collect::<Vec<_>>(), peer)
    };
    let peer_base_url = peer
        .base_url
        .clone()
        .ok_or_else(|| AppError::BadRequest("paired peer has no baseUrl".to_string()))?;
    let peer_access_token = peer_request_token(
        normalize_optional_text(request.peer_access_token.unwrap_or_default()),
        &peer,
    );
    let peer_projects = fetch_peer_projects(&peer_base_url, peer_access_token.as_deref()).await?;
    ensure_peer_project_available(&peer_projects, &request.project_id)?;

    let summary = collaboration::generate_baseline_summary(
        sessions,
        collaboration::BaselineSummaryRequest {
            peer_base_url: peer_base_url.clone(),
            peer_access_token,
            project_id: request.project_id.clone(),
            peer_id: Some(peer.peer_id.clone()),
            peer_display_name: Some(peer.display_name.clone()),
            peer_trusted: Some(peer.trusted),
            peer_last_seen_at: peer.last_seen_at,
            days: request.days,
            language: request.language,
        },
    )
    .await?;

    let now = Utc::now();
    let analysis_cycle = request.analysis_cycle.unwrap_or_default();
    let next_run_at = next_run_after(summary.generated_at, &analysis_cycle);
    let topics = normalize_labels(request.topics.unwrap_or_else(|| {
        vec![
            "boundary".to_string(),
            "confirmation".to_string(),
            "conflict".to_string(),
        ]
    }));
    let subscription = Subscription {
        subscription_id: subscription_id_for(&peer.peer_id, &request.project_id),
        peer_id: peer.peer_id.clone(),
        project_id: request.project_id,
        status: SubscriptionStatus::Active,
        topics,
        created_at: now,
        baseline_generated_at: Some(summary.generated_at),
        analysis_cycle,
        next_run_at,
        last_run_at: Some(summary.generated_at),
        last_run_status: Some("success".to_string()),
        last_run_error: None,
    };

    let mut inner = state.inner.write().await;
    let mut collaboration = inner.collaboration.clone();
    collaboration.subscriptions.retain(|existing| {
        !(existing.peer_id == subscription.peer_id
            && existing.project_id == subscription.project_id)
    });
    reset_subscription_delta_cursor(&mut collaboration, &subscription);
    collaboration.subscriptions.push(subscription.clone());
    collaboration
        .summaries
        .retain(|existing| existing.summary_id != summary.summary_id);
    collaboration.summaries.push(summary.clone());
    storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)?;
    inner.collaboration = collaboration.clone();
    let state_response = collaboration_state_response(
        &collaboration,
        &inner.sessions,
        &inner.peer_presence,
        &state.config,
    );

    Ok(Json(CreateSubscriptionResponse {
        state: state_response,
        subscription,
        summary,
    }))
}

async fn update_subscription_schedule(
    Path(subscription_id): Path<String>,
    State(state): State<SharedState>,
    Json(request): Json<UpdateSubscriptionScheduleRequest>,
) -> Result<Json<CollaborationStateResponse>, AppError> {
    let now = Utc::now();
    let mut inner = state.inner.write().await;
    let mut collaboration = inner.collaboration.clone();
    let subscription = collaboration
        .subscriptions
        .iter_mut()
        .find(|subscription| subscription.subscription_id == subscription_id)
        .ok_or_else(|| {
            AppError::NotFound(format!("subscription '{subscription_id}' was not found"))
        })?;
    subscription.analysis_cycle = request.analysis_cycle;
    subscription.next_run_at = next_run_after(now, &subscription.analysis_cycle);
    if subscription.analysis_cycle == AnalysisCycle::Manual {
        subscription.last_run_error = None;
    }

    storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)?;
    inner.collaboration = collaboration.clone();

    Ok(Json(collaboration_state_response(
        &collaboration,
        &inner.sessions,
        &inner.peer_presence,
        &state.config,
    )))
}

async fn generate_collaboration_baseline(
    State(state): State<SharedState>,
    Json(mut request): Json<collaboration::BaselineSummaryRequest>,
) -> Result<Json<crate::models::CollaborationSummary>, AppError> {
    let sessions = {
        let inner = state.inner.read().await;
        let peer = resolve_paired_baseline_peer(&inner.collaboration, &request)?;
        request.peer_id = Some(peer.peer_id.clone());
        request.peer_display_name = Some(peer.display_name.clone());
        request.peer_trusted = Some(peer.trusted);
        request.peer_last_seen_at = peer.last_seen_at;
        request.peer_base_url = peer
            .base_url
            .clone()
            .ok_or_else(|| AppError::BadRequest("paired peer has no baseUrl".to_string()))?;
        request.peer_access_token = peer_request_token(
            normalize_optional_text(request.peer_access_token.unwrap_or_default()),
            &peer,
        );
        inner.sessions.values().cloned().collect::<Vec<_>>()
    };
    let summary = collaboration::generate_baseline_summary(sessions, request).await?;

    {
        let mut inner = state.inner.write().await;
        let mut collaboration = inner.collaboration.clone();
        collaboration
            .summaries
            .retain(|existing| existing.summary_id != summary.summary_id);
        collaboration.summaries.push(summary.clone());
        storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)?;
        inner.collaboration = collaboration;
    }

    Ok(Json(summary))
}

async fn generate_collaboration_incremental(
    State(state): State<SharedState>,
    Json(request): Json<IncrementalSummaryRequest>,
) -> Result<Json<IncrementalSummaryResponse>, AppError> {
    Ok(Json(run_incremental_summary(state, request).await?))
}

pub(crate) async fn run_incremental_summary(
    state: SharedState,
    request: IncrementalSummaryRequest,
) -> Result<IncrementalSummaryResponse, AppError> {
    if !try_begin_incremental_run(&state, &request.subscription_id) {
        return Err(AppError::BadRequest(
            "incremental summary is already running for this subscription".to_string(),
        ));
    }

    let subscription_id = request.subscription_id.clone();
    let result = run_incremental_summary_inner(state.clone(), request).await;
    if let Err(error) = &result {
        record_incremental_failure(&state, &subscription_id, error.to_string()).await;
    }
    finish_incremental_run(&state, &subscription_id);

    result
}

async fn run_incremental_summary_inner(
    state: SharedState,
    request: IncrementalSummaryRequest,
) -> Result<IncrementalSummaryResponse, AppError> {
    let (sessions, peer, subscription, cursor, active_since, previous_summaries) = {
        let inner = state.inner.read().await;
        let subscription = inner
            .collaboration
            .subscriptions
            .iter()
            .find(|subscription| subscription.subscription_id == request.subscription_id)
            .cloned()
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "subscription '{}' was not found",
                    request.subscription_id
                ))
            })?;
        let peer = inner
            .collaboration
            .trusted_peers
            .iter()
            .find(|peer| peer.peer_id == subscription.peer_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound("subscription peer was not found".to_string()))?;
        let cursor = inner.collaboration.delta_cursors.iter().find(|cursor| {
            cursor.source_id == subscription.peer_id
                && cursor.session_path == subscription_cursor_path(&subscription)
        });
        let active_since = request
            .since
            .or_else(|| cursor.and_then(|cursor| cursor.last_record_timestamp))
            .or(subscription.baseline_generated_at)
            .unwrap_or_else(|| Utc::now() - Duration::days(1));
        let mut previous_summaries = inner
            .collaboration
            .summaries
            .iter()
            .filter(|summary| {
                summary.project_id == subscription.project_id
                    && summary
                        .source_ids
                        .iter()
                        .any(|source_id| source_id == &subscription.peer_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        previous_summaries.sort_by(|a, b| b.generated_at.cmp(&a.generated_at));
        previous_summaries.truncate(8);

        (
            inner.sessions.values().cloned().collect::<Vec<_>>(),
            peer,
            subscription,
            cursor.cloned(),
            active_since,
            previous_summaries,
        )
    };
    let peer_base_url = peer
        .base_url
        .clone()
        .ok_or_else(|| AppError::BadRequest("paired peer has no baseUrl".to_string()))?;
    let peer_access_token = peer_request_token(
        normalize_optional_text(request.peer_access_token.unwrap_or_default()),
        &peer,
    );
    let peer_sessions = fetch_peer_session_summaries(
        &peer_base_url,
        peer_access_token.as_deref(),
        &subscription.project_id,
        active_since,
    )
    .await?;
    let mut peer_deltas = Vec::new();
    for peer_session in &peer_sessions {
        let mut page_cursor = None;
        for _ in 0..20 {
            let response = fetch_peer_session_deltas(
                &peer_base_url,
                peer_access_token.as_deref(),
                &peer_session.session_id,
                active_since,
                page_cursor.as_deref(),
            )
            .await?;
            peer_deltas.extend(response.deltas);
            let Some(next_cursor) = response.next_cursor else {
                break;
            };
            if page_cursor.as_deref() == Some(next_cursor.as_str()) {
                break;
            }
            page_cursor = Some(next_cursor);
        }
    }
    sort_and_dedupe_peer_deltas(&mut peer_deltas);
    retain_unprocessed_peer_deltas(&mut peer_deltas, cursor.as_ref());
    peer_deltas.truncate(500);

    let latest_delta = peer_deltas.last().cloned();
    let summary = collaboration::generate_incremental_summary(
        sessions,
        collaboration::IncrementalSummaryInput {
            peer_id: peer.peer_id.clone(),
            peer_display_name: Some(peer.display_name.clone()),
            peer_base_url,
            peer_trusted: peer.trusted,
            peer_last_seen_at: peer.last_seen_at,
            project_id: subscription.project_id.clone(),
            active_since,
            language: request.language,
            previous_summaries,
            peer_sessions,
            peer_deltas,
        },
    )
    .await?;

    let mut inner = state.inner.write().await;
    let mut collaboration = inner.collaboration.clone();
    collaboration
        .summaries
        .retain(|existing| existing.summary_id != summary.summary_id);
    collaboration.summaries.push(summary.clone());
    if let Some(latest_delta) = latest_delta {
        let cursor_path = subscription_cursor_path(&subscription);
        collaboration.delta_cursors.retain(|cursor| {
            !(cursor.source_id == subscription.peer_id && cursor.session_path == cursor_path)
        });
        collaboration.delta_cursors.push(SessionDeltaCursor {
            source_id: subscription.peer_id,
            session_path: cursor_path,
            last_offset: 0,
            last_record_timestamp: Some(latest_delta.timestamp),
            last_record_hash: Some(latest_delta.delta_id),
            updated_at: Utc::now(),
        });
    }
    let generated_at = summary.generated_at;
    if let Some(subscription) = collaboration
        .subscriptions
        .iter_mut()
        .find(|item| item.subscription_id == request.subscription_id)
    {
        subscription.last_run_at = Some(generated_at);
        subscription.last_run_status = Some("success".to_string());
        subscription.last_run_error = None;
        subscription.next_run_at = next_run_after(generated_at, &subscription.analysis_cycle);
    }
    storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)?;
    inner.collaboration = collaboration.clone();
    let state_response = collaboration_state_response(
        &collaboration,
        &inner.sessions,
        &inner.peer_presence,
        &state.config,
    );

    Ok(IncrementalSummaryResponse {
        state: state_response,
        summary,
    })
}

pub fn filter_counts(sessions: &[Session], stale_after_days: i64) -> FilterCounts {
    let now = Utc::now();
    let seven_days_ago = now - Duration::days(7);
    let stale_cutoff = now - Duration::days(stale_after_days);

    let active = sessions
        .iter()
        .filter(|session| session.status != SessionStatus::Deleted)
        .collect::<Vec<_>>();

    FilterCounts {
        all: active.len(),
        recent: active
            .iter()
            .filter(|session| session.last_modified >= seven_days_ago)
            .count(),
        stale: active
            .iter()
            .filter(|session| {
                session.status == SessionStatus::Stale || session.last_modified < stale_cutoff
            })
            .count(),
        unlabeled: active
            .iter()
            .filter(|session| session.labels.is_empty())
            .count(),
        deleted: sessions
            .iter()
            .filter(|session| session.status == SessionStatus::Deleted)
            .count(),
    }
}

pub fn label_counts(sessions: &[Session]) -> Vec<LabelCount> {
    let mut counts = HashMap::<String, usize>::new();

    for session in sessions
        .iter()
        .filter(|session| session.status != SessionStatus::Deleted)
    {
        for label in &session.labels {
            *counts.entry(label.clone()).or_default() += 1;
        }
    }

    let mut labels = counts
        .into_iter()
        .map(|(name, count)| LabelCount { name, count })
        .collect::<Vec<_>>();

    labels.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    labels
}

fn ensure_meta<'a>(
    metadata: &'a mut crate::models::MetadataFile,
    session: &Session,
) -> &'a mut SessionMeta {
    metadata
        .sessions
        .entry(session.id.clone())
        .or_insert_with(|| SessionMeta {
            session_id: session.id.clone(),
            labels: session.labels.clone(),
            notes: session.notes.clone(),
            status_override: None,
            updated_at: Utc::now(),
        })
}

fn find_session_by_public_id<'a>(
    sessions: &'a HashMap<String, Session>,
    id: &str,
) -> Option<&'a Session> {
    sessions.get(id).or_else(|| {
        sessions
            .values()
            .find(|session| session.codex_session_id.as_deref() == Some(id))
    })
}

fn require_peer_token(headers: &HeaderMap, configured_token: Option<&str>) -> Result<(), AppError> {
    let Some(configured_token) = configured_token else {
        return Err(AppError::Unauthorized(
            "peer token must be configured before peer sharing is available".to_string(),
        ));
    };
    let header_token = headers
        .get("x-csm-peer-token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        });

    match header_token {
        Some(token) if token == configured_token => Ok(()),
        _ => Err(AppError::Unauthorized(
            "valid peer token is required".to_string(),
        )),
    }
}

fn load_mount_store_for_state(state: &SharedState) -> Result<MountStore, AppError> {
    mount_storage::load_mount_store(&mount_storage::mounts_path(&state.config.data_dir))
}

fn mount_response(store: &MountStore, mount: &MountRecord) -> Result<MountResponse, AppError> {
    let policy = mount_storage::find_policy(store, &mount.policy_id)
        .ok_or_else(|| AppError::NotFound("mount policy was not found".to_string()))?
        .clone();
    let credential = mount_storage::find_credential_profile(store, &mount.credential_profile_id)
        .ok_or_else(|| AppError::NotFound("mount credential was not found".to_string()))?
        .clone();

    Ok(MountResponse {
        mount: mount.clone(),
        policy,
        credential,
    })
}

async fn project_root_for_request(
    state: &SharedState,
    project_id: &str,
    project_path: Option<String>,
) -> Result<PathBuf, AppError> {
    if let Some(path) = project_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let identity = project::project_identity_for_path(Some(path));
        if identity.project_id != project_id {
            return Err(AppError::BadRequest(
                "projectPath resolves to a different projectId".to_string(),
            ));
        }
        return identity
            .root_path
            .map(PathBuf::from)
            .ok_or_else(|| AppError::BadRequest("project root could not be resolved".to_string()));
    }

    let sessions = {
        let inner = state.inner.read().await;
        inner.sessions.values().cloned().collect::<Vec<_>>()
    };
    collaboration_projects(&sessions)
        .into_iter()
        .find(|project| project.project_id == project_id)
        .and_then(|project| project.root_path.map(PathBuf::from))
        .ok_or_else(|| {
            AppError::BadRequest(
                "projectId is not known locally; include projectPath to resolve it".to_string(),
            )
        })
}

fn resolve_mount_point(
    project_root: &FsPath,
    mount_id: &str,
    request: &CreateMountRequest,
) -> Result<PathBuf, AppError> {
    match request.mount_point.as_deref().map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => Ok(PathBuf::from(path)),
        None => match request.mount_point_mode.as_deref().unwrap_or("project") {
            "project" => Ok(fuse::project_mount_point(project_root, mount_id)),
            other => Err(AppError::BadRequest(format!(
                "unsupported mountPointMode '{other}'"
            ))),
        },
    }
}

fn normalize_mount_id(mount_id: &str) -> Result<String, AppError> {
    let mount_id = mount_id.trim();
    if mount_id.is_empty()
        || mount_id.contains('/')
        || mount_id.contains('\\')
        || mount_id == "."
        || mount_id == ".."
    {
        return Err(AppError::BadRequest(
            "mountId must be a non-empty path segment".to_string(),
        ));
    }
    Ok(mount_id.to_string())
}

fn apply_policy_patch(policy: &mut MountPolicy, patch: MountPolicyPatch) -> Result<(), AppError> {
    if let Some(readonly) = patch.readonly {
        if !readonly {
            return Err(AppError::BadRequest(
                "Phase 1 mounts are always readonly".to_string(),
            ));
        }
        policy.readonly = true;
    }
    if let Some(allowed_schemas) = patch.allowed_schemas {
        policy.allowed_schemas = normalize_mount_list(allowed_schemas);
    }
    if let Some(blocked_schemas) = patch.blocked_schemas {
        policy.blocked_schemas = normalize_mount_list(blocked_schemas);
    }
    if let Some(allowed_tables) = patch.allowed_tables {
        policy.allowed_tables = normalize_mount_list(allowed_tables);
    }
    if let Some(blocked_tables) = patch.blocked_tables {
        policy.blocked_tables = normalize_mount_list(blocked_tables);
    }
    if let Some(max_sample_rows) = patch.max_sample_rows {
        policy.max_sample_rows = max_sample_rows.clamp(1, 1000);
    }
    if let Some(max_lookup_rows) = patch.max_lookup_rows {
        policy.max_lookup_rows = max_lookup_rows.clamp(1, 1000);
    }
    if let Some(max_file_bytes) = patch.max_file_bytes {
        policy.max_file_bytes = max_file_bytes.clamp(1024, 16 * 1024 * 1024);
    }
    if let Some(query_timeout_ms) = patch.query_timeout_ms {
        policy.query_timeout_ms = query_timeout_ms.clamp(100, 30_000);
    }
    if let Some(redact_columns) = patch.redact_columns {
        policy.redact_columns = normalize_mount_list(redact_columns);
    }
    if let Some(require_tenant_filter) = patch.require_tenant_filter {
        policy.require_tenant_filter = require_tenant_filter;
    }
    if let Some(tenant_columns) = patch.tenant_columns {
        policy.tenant_columns = normalize_mount_list(tenant_columns);
    }
    if let Some(allow_addressable_lookups) = patch.allow_addressable_lookups {
        policy.allow_addressable_lookups = allow_addressable_lookups;
    }
    if let Some(allow_custom_queries) = patch.allow_custom_queries {
        if allow_custom_queries {
            return Err(AppError::BadRequest(
                "arbitrary SQL is not supported in the Phase 1 MVP".to_string(),
            ));
        }
        policy.allow_custom_queries = false;
    }
    Ok(())
}

fn normalize_mount_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn collaboration_projects(sessions: &[Session]) -> Vec<ProjectIdentity> {
    let mut projects = Vec::<ProjectIdentity>::new();

    for session in sessions {
        if session.status == SessionStatus::Deleted {
            continue;
        }

        let identity = project::project_identity_for_path(session.project_path.as_deref());
        if !projects
            .iter()
            .any(|project| project.project_id == identity.project_id)
        {
            projects.push(identity);
        }
    }

    projects.sort_by(|a, b| a.path_label.cmp(&b.path_label));
    projects
}

fn merge_share_policy(policy: &mut SharePolicy, request: UpdateSharePolicyRequest) {
    if let Some(project_path) = request.project_path {
        policy.project_path = normalize_optional_text(project_path);
    }
    if let Some(enabled) = request.enabled {
        policy.enabled = enabled;
    }
    if let Some(shared_labels) = request.shared_labels {
        policy.shared_labels = normalize_labels(shared_labels);
    }
    if let Some(blocked_labels) = request.blocked_labels {
        policy.blocked_labels = normalize_labels(blocked_labels);
    }
    if let Some(max_excerpt_chars) = request.max_excerpt_chars {
        policy.max_excerpt_chars = max_excerpt_chars.clamp(100, 40_000);
    }
    if let Some(max_delta_chars) = request.max_delta_chars {
        policy.max_delta_chars = max_delta_chars.clamp(100, 10_000);
    }
    policy.updated_at = Utc::now();
}

fn normalize_optional_text(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn normalize_labels(labels: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for label in labels {
        let label = label.trim();
        if label.is_empty() || !seen.insert(label.to_string()) {
            continue;
        }

        normalized.push(label.to_string());
    }

    normalized
}

async fn fetch_peer_health(peer_base_url: &str) -> Result<PeerHealthResponse, AppError> {
    let url = format!("{}/peer/health", peer_base_url.trim_end_matches('/'));
    send_peer_json(peer_get_request(&url, None)).await
}

async fn fetch_peer_projects(
    peer_base_url: &str,
    access_token: Option<&str>,
) -> Result<Vec<collaboration::PeerProject>, AppError> {
    let url = format!("{}/peer/projects", peer_base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut request = client.get(url);
    if let Some(access_token) = access_token {
        request = request.header("x-csm-peer-token", access_token);
    }

    let response = request
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|error| AppError::External(format!("failed to reach peer: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::External(format!(
            "peer rejected read API verification with HTTP {status}"
        )));
    }

    response
        .json::<Vec<collaboration::PeerProject>>()
        .await
        .map_err(|error| {
            AppError::External(format!("peer returned invalid projects JSON: {error}"))
        })
}

fn ensure_peer_project_available(
    peer_projects: &[collaboration::PeerProject],
    project_id: &str,
) -> Result<(), AppError> {
    if peer_projects
        .iter()
        .any(|project| project.project_id == project_id)
    {
        return Ok(());
    }

    Err(AppError::BadRequest(
        "paired peer has not shared the requested project".to_string(),
    ))
}

fn sort_and_dedupe_peer_deltas(deltas: &mut Vec<crate::models::SessionDelta>) {
    let mut seen = HashSet::new();
    deltas.retain(|delta| seen.insert(delta.delta_id.clone()));
    deltas.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| a.delta_id.cmp(&b.delta_id))
    });
}

fn retain_unprocessed_peer_deltas(
    deltas: &mut Vec<crate::models::SessionDelta>,
    cursor: Option<&SessionDeltaCursor>,
) {
    let Some(cursor) = cursor else {
        return;
    };
    let Some(last_timestamp) = cursor.last_record_timestamp else {
        return;
    };

    deltas.retain(|delta| delta.timestamp >= last_timestamp);
    if let Some(last_hash) = cursor.last_record_hash.as_deref() {
        if let Some(position) = deltas.iter().position(|delta| delta.delta_id == last_hash) {
            deltas.drain(..=position);
            return;
        }
    }

    deltas.retain(|delta| delta.timestamp > last_timestamp);
}

async fn fetch_peer_session_summaries(
    peer_base_url: &str,
    access_token: Option<&str>,
    project_id: &str,
    since: chrono::DateTime<Utc>,
) -> Result<Vec<collaboration::PeerSessionSummary>, AppError> {
    let url = format!("{}/peer/sessions", peer_base_url.trim_end_matches('/'));
    let request = peer_get_request(&url, access_token).query(&[
        ("projectId", project_id.to_string()),
        ("since", since.to_rfc3339()),
        ("limit", "100".to_string()),
    ]);
    send_peer_json(request).await
}

async fn fetch_peer_session_deltas(
    peer_base_url: &str,
    access_token: Option<&str>,
    session_id: &str,
    since: chrono::DateTime<Utc>,
    cursor: Option<&str>,
) -> Result<collaboration::PeerDeltasResponse, AppError> {
    let url = format!(
        "{}/peer/sessions/{}/deltas",
        peer_base_url.trim_end_matches('/'),
        session_id
    );
    let mut query = vec![("since", since.to_rfc3339()), ("limit", "100".to_string())];
    if let Some(cursor) = cursor {
        query.push(("cursor", cursor.to_string()));
    }
    let request = peer_get_request(&url, access_token).query(&query);
    send_peer_json(request).await
}

fn peer_get_request(url: &str, access_token: Option<&str>) -> reqwest::RequestBuilder {
    let client = reqwest::Client::new();
    let mut request = client.get(url);
    if let Some(access_token) = access_token {
        request = request.header("x-csm-peer-token", access_token);
    }

    request
}

async fn send_peer_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, AppError> {
    let response = request
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|error| AppError::External(format!("failed to reach peer: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::External(format!(
            "peer rejected read API request with HTTP {status}"
        )));
    }

    response
        .json::<T>()
        .await
        .map_err(|error| AppError::External(format!("peer returned invalid JSON: {error}")))
}

fn upsert_trusted_peer(store: &mut CollaborationStore, peer: PeerMetadata) {
    store
        .trusted_peers
        .retain(|existing| existing.peer_id != peer.peer_id);
    store.trusted_peers.push(peer);
    store
        .trusted_peers
        .sort_by(|a, b| a.display_name.cmp(&b.display_name));
}

fn peer_request_token(request_token: Option<String>, peer: &PeerMetadata) -> Option<String> {
    request_token.or_else(|| peer.access_token.clone())
}

pub(crate) fn next_run_after(
    anchor: chrono::DateTime<Utc>,
    analysis_cycle: &AnalysisCycle,
) -> Option<chrono::DateTime<Utc>> {
    analysis_cycle
        .duration_minutes()
        .map(|minutes| anchor + Duration::minutes(minutes))
}

fn try_begin_incremental_run(state: &SharedState, subscription_id: &str) -> bool {
    let mut active_runs = state
        .active_incremental_runs
        .lock()
        .expect("incremental run lock poisoned");
    active_runs.insert(subscription_id.to_string())
}

fn finish_incremental_run(state: &SharedState, subscription_id: &str) {
    let mut active_runs = state
        .active_incremental_runs
        .lock()
        .expect("incremental run lock poisoned");
    active_runs.remove(subscription_id);
}

async fn record_incremental_failure(state: &SharedState, subscription_id: &str, error: String) {
    let mut inner = state.inner.write().await;
    let mut collaboration = inner.collaboration.clone();
    let Some(subscription) = collaboration
        .subscriptions
        .iter_mut()
        .find(|subscription| subscription.subscription_id == subscription_id)
    else {
        return;
    };

    let now = Utc::now();
    subscription.last_run_at = Some(now);
    subscription.last_run_status = Some("failed".to_string());
    subscription.last_run_error = Some(error.chars().take(500).collect());
    subscription.next_run_at = next_run_after(now, &subscription.analysis_cycle);

    if let Err(error) =
        storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)
    {
        tracing::warn!("failed to persist scheduled incremental failure: {error}");
        return;
    }
    inner.collaboration = collaboration;
}

async fn mark_trusted_peer_seen(state: &SharedState, peer_id: &str) -> Result<(), AppError> {
    let mut inner = state.inner.write().await;
    let mut collaboration = inner.collaboration.clone();
    let Some(peer) = collaboration
        .trusted_peers
        .iter_mut()
        .find(|peer| peer.peer_id == peer_id)
    else {
        return Ok(());
    };

    peer.last_seen_at = Some(Utc::now());
    storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)?;
    inner.collaboration = collaboration;

    Ok(())
}

fn upsert_peer_source(
    store: &mut CollaborationStore,
    peer: &PeerMetadata,
    now: chrono::DateTime<Utc>,
) {
    store
        .sources
        .retain(|source| source.source_id != peer.peer_id);
    store.sources.push(CollaborationSource {
        source_id: peer.peer_id.clone(),
        kind: CollaborationSourceKind::LanPeer,
        display_name: peer.display_name.clone(),
        session_root: None,
        peer_id: Some(peer.peer_id.clone()),
        enabled: true,
        created_at: now,
    });
}

fn resolve_subscription_peer(
    store: &CollaborationStore,
    request: &CreateSubscriptionRequest,
) -> Result<PeerMetadata, AppError> {
    if let Some(peer_id) = request
        .peer_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return store
            .trusted_peers
            .iter()
            .find(|peer| peer.peer_id == peer_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("paired peer '{peer_id}' was not found")));
    }

    let peer_base_url = request
        .peer_base_url
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("peerId or peerBaseUrl is required".to_string()))
        .and_then(collaboration::normalize_peer_base_url)?;
    store
        .trusted_peers
        .iter()
        .find(|peer| peer.base_url.as_deref() == Some(peer_base_url.as_str()))
        .cloned()
        .ok_or_else(|| AppError::NotFound("peer must be paired before subscribing".to_string()))
}

fn resolve_paired_baseline_peer(
    store: &CollaborationStore,
    request: &collaboration::BaselineSummaryRequest,
) -> Result<PeerMetadata, AppError> {
    let requested_base_url = if request.peer_base_url.trim().is_empty() {
        None
    } else {
        Some(collaboration::normalize_peer_base_url(
            &request.peer_base_url,
        )?)
    };

    if let Some(peer_id) = request
        .peer_id
        .as_deref()
        .map(str::trim)
        .filter(|peer_id| !peer_id.is_empty())
    {
        let peer = store
            .trusted_peers
            .iter()
            .find(|peer| peer.trusted && peer.peer_id == peer_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("paired peer '{peer_id}' was not found")))?;
        let peer_base_url = peer
            .base_url
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("paired peer has no baseUrl".to_string()))?;

        if requested_base_url
            .as_deref()
            .is_some_and(|base_url| base_url != peer_base_url)
        {
            return Err(AppError::BadRequest(
                "peerBaseUrl does not match the paired peer".to_string(),
            ));
        }

        return Ok(peer);
    }

    let requested_base_url = requested_base_url
        .ok_or_else(|| AppError::BadRequest("peerId or peerBaseUrl is required".to_string()))?;
    store
        .trusted_peers
        .iter()
        .find(|peer| peer.trusted && peer.base_url.as_deref() == Some(requested_base_url.as_str()))
        .cloned()
        .ok_or_else(|| {
            AppError::NotFound("peer must be paired before baseline analysis".to_string())
        })
}

fn subscription_id_for(peer_id: &str, project_id: &str) -> String {
    format!("sub_{}_{}", peer_id, project_id)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn subscription_cursor_path(subscription: &Subscription) -> String {
    format!("subscription:{}", subscription.subscription_id)
}

fn reset_subscription_delta_cursor(store: &mut CollaborationStore, subscription: &Subscription) {
    let cursor_path = subscription_cursor_path(subscription);
    store.delta_cursors.retain(|cursor| {
        !(cursor.source_id == subscription.peer_id && cursor.session_path == cursor_path)
    });
}

fn collaboration_state_response(
    store: &CollaborationStore,
    sessions: &HashMap<String, Session>,
    peer_presence: &HashMap<String, PeerPresence>,
    config: &Config,
) -> CollaborationStateResponse {
    CollaborationStateResponse {
        local_config: local_collaboration_config(store, config),
        store: store.clone(),
        projects: collaboration_projects(&sessions.values().cloned().collect::<Vec<_>>()),
        discovered_peers: sorted_peer_presence(peer_presence),
    }
}

fn local_collaboration_config(
    store: &CollaborationStore,
    config: &Config,
) -> LocalCollaborationConfig {
    let fallback_base_url = format!("http://{}", config.bind_addr);
    let local_peer = store.local_peer.clone().unwrap_or_else(|| PeerMetadata {
        peer_id: collaboration::peer_id_for_base_url(&fallback_base_url),
        display_name: config.peer_display_name.clone(),
        trusted: true,
        public_key: None,
        base_url: Some(fallback_base_url.clone()),
        last_seen_at: Some(Utc::now()),
        access_token: None,
    });

    LocalCollaborationConfig {
        peer_id: local_peer.peer_id,
        display_name: local_peer.display_name,
        base_url: local_peer.base_url.unwrap_or(fallback_base_url),
        bind_address: config.bind_addr.to_string(),
        peer_token_configured: effective_local_peer_token(store, config).is_some(),
        peer_token: effective_local_peer_token(store, config),
        lan_discovery_enabled: config.lan_discovery_enabled,
    }
}

fn effective_local_peer_token(store: &CollaborationStore, config: &Config) -> Option<String> {
    store
        .local_peer_token
        .clone()
        .or_else(|| config.peer_token.clone())
        .filter(|token| !token.trim().is_empty())
}

const DISCOVERED_PEER_TTL_SECONDS: i64 = 30;

fn sorted_peer_presence(peer_presence: &HashMap<String, PeerPresence>) -> Vec<PeerPresence> {
    let stale_cutoff = Utc::now() - Duration::seconds(DISCOVERED_PEER_TTL_SECONDS);
    let mut peers = peer_presence
        .values()
        .filter(|presence| presence.last_seen_at >= stale_cutoff)
        .cloned()
        .collect::<Vec<_>>();
    peers.sort_by(|a, b| {
        b.last_seen_at
            .cmp(&a.last_seen_at)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    peers
}

fn apply_metadata_overrides(sessions: &mut [Session], metadata: &MetadataFile) {
    for session in sessions {
        let Some(meta) = metadata.sessions.get(&session.id) else {
            continue;
        };

        merge_labels(&mut session.labels, &meta.labels);
        session.notes = meta.notes.clone();
        if let Some(status) = meta.status_override.clone() {
            session.status = status;
        }
    }
}

fn apply_stale_threshold(sessions: &mut [Session], metadata: &MetadataFile, stale_after_days: i64) {
    for session in sessions {
        if let Some(status) = metadata
            .sessions
            .get(&session.id)
            .and_then(|meta| meta.status_override.clone())
        {
            session.status = status;
        } else {
            session.status =
                scanner::status_from_last_modified(session.last_modified, stale_after_days);
        }
    }
}

fn merge_labels(labels: &mut Vec<String>, metadata_labels: &[String]) {
    let mut merged = metadata_labels.to_vec();

    for label in labels.iter() {
        if !merged.iter().any(|existing| existing == label) {
            merged.push(label.clone());
        }
    }

    *labels = merged;
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        net::SocketAddr,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    use axum::extract::{Path, State};
    use chrono::TimeZone;
    use tokio::sync::RwLock;

    use super::*;
    use crate::{
        config::Config,
        state::{AppData, AppState},
    };

    #[test]
    fn collaboration_projects_exclude_deleted_sessions() {
        let active_session = Session {
            id: "active".to_string(),
            codex_session_id: None,
            name: "Active".to_string(),
            excerpt: "excerpt".to_string(),
            full_content: "content".to_string(),
            path: "/tmp/active.md".to_string(),
            project_path: Some("/work/active-project".to_string()),
            labels: Vec::new(),
            last_modified: Utc::now(),
            size: 7,
            status: SessionStatus::Active,
            notes: String::new(),
        };
        let deleted_session = Session {
            id: "deleted".to_string(),
            codex_session_id: None,
            name: "Deleted".to_string(),
            excerpt: "excerpt".to_string(),
            full_content: "content".to_string(),
            path: "/tmp/deleted.md".to_string(),
            project_path: Some("/work/deleted-project".to_string()),
            labels: Vec::new(),
            last_modified: Utc::now(),
            size: 7,
            status: SessionStatus::Deleted,
            notes: String::new(),
        };

        let projects = collaboration_projects(&[active_session, deleted_session]);

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path_label, "active-project");
    }

    #[tokio::test]
    async fn list_projects_returns_known_non_deleted_projects() {
        let temp_dir = unique_temp_dir("list-projects");
        let alpha_path = temp_dir.join("alpha");
        let beta_path = temp_dir.join("beta");
        fs::create_dir_all(&alpha_path).expect("create alpha");
        fs::create_dir_all(&beta_path).expect("create beta");
        let alpha_session = test_session(
            "alpha-1",
            "Alpha 1",
            Some(alpha_path.to_string_lossy().to_string()),
            SessionStatus::Active,
        );
        let duplicate_alpha_session = test_session(
            "alpha-2",
            "Alpha 2",
            Some(alpha_path.to_string_lossy().to_string()),
            SessionStatus::Active,
        );
        let deleted_beta_session = test_session(
            "beta-1",
            "Beta 1",
            Some(beta_path.to_string_lossy().to_string()),
            SessionStatus::Deleted,
        );
        let state = test_state(
            &temp_dir,
            HashMap::from([
                (alpha_session.id.clone(), alpha_session),
                (duplicate_alpha_session.id.clone(), duplicate_alpha_session),
                (deleted_beta_session.id.clone(), deleted_beta_session),
            ]),
        );

        let projects = list_projects(State(state)).await.0;

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path_label, "alpha");

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn resolve_project_returns_project_identity_for_path() {
        let temp_dir = unique_temp_dir("resolve-project");
        let project_path = temp_dir.join("resolved-project");
        fs::create_dir_all(&project_path).expect("create project path");

        let response = resolve_project(Json(ResolveProjectRequest {
            path: project_path.to_string_lossy().to_string(),
        }))
        .await
        .0;

        assert_eq!(response.project.path_label, "resolved-project");
        assert_eq!(
            response.project.root_path.as_deref(),
            Some(project_path.to_string_lossy().as_ref())
        );
        assert!(response.project.project_id.starts_with("project_"));

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn label_update_does_not_mutate_state_when_metadata_save_fails() {
        let temp_dir = unique_temp_dir("metadata-save-failure");
        let metadata_path = temp_dir.join("metadata.json");
        fs::create_dir_all(&metadata_path).expect("create directory at metadata path");

        let session = Session {
            id: "session-1".to_string(),
            codex_session_id: None,
            name: "Session 1".to_string(),
            excerpt: "excerpt".to_string(),
            full_content: "content".to_string(),
            path: temp_dir.join("session.md").to_string_lossy().to_string(),
            project_path: None,
            labels: Vec::new(),
            last_modified: Utc::now(),
            size: 7,
            status: SessionStatus::Active,
            notes: String::new(),
        };

        let state = Arc::new(AppState {
            config: Config {
                bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                data_dir: temp_dir.clone(),
                metadata_path,
                collaboration_path: temp_dir.join("collaboration.json"),
                peer_token: None,
                lan_discovery_enabled: false,
                peer_display_name: "Test".to_string(),
                archive_dir: temp_dir.join("archive"),
                max_preview_bytes: 1024,
                stale_after_days: 15,
            },
            lan_discovery: Mutex::new(None),
            active_incremental_runs: Mutex::new(HashSet::new()),
            mount_cache: crate::mounts::router::MountCache::default(),
            inner: RwLock::new(AppData {
                metadata: MetadataFile::default(),
                collaboration: CollaborationStore::default(),
                peer_presence: HashMap::new(),
                sessions: HashMap::from([(session.id.clone(), session)]),
                workspace_path: Some(temp_dir.to_string_lossy().to_string()),
                stale_after_days: 15,
            }),
        });

        let result = update_labels(
            Path("session-1".to_string()),
            State(state.clone()),
            Json(UpdateLabelsRequest {
                labels: vec!["backend".to_string()],
            }),
        )
        .await;

        assert!(result.is_err());

        let inner = state.inner.read().await;
        let session = inner.sessions.get("session-1").expect("session exists");
        assert!(session.labels.is_empty());
        assert!(inner.metadata.sessions.is_empty());

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn settings_update_refreshes_stale_status_and_persists_threshold() {
        let temp_dir = unique_temp_dir("settings-stale-threshold");
        let metadata_path = temp_dir.join("metadata.json");

        let session = Session {
            id: "session-1".to_string(),
            codex_session_id: None,
            name: "Session 1".to_string(),
            excerpt: "excerpt".to_string(),
            full_content: "content".to_string(),
            path: temp_dir.join("session.md").to_string_lossy().to_string(),
            project_path: None,
            labels: Vec::new(),
            last_modified: Utc::now() - Duration::days(20),
            size: 7,
            status: SessionStatus::Stale,
            notes: String::new(),
        };

        let state = Arc::new(AppState {
            config: Config {
                bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                data_dir: temp_dir.clone(),
                metadata_path: metadata_path.clone(),
                collaboration_path: temp_dir.join("collaboration.json"),
                peer_token: None,
                lan_discovery_enabled: false,
                peer_display_name: "Test".to_string(),
                archive_dir: temp_dir.join("archive"),
                max_preview_bytes: 1024,
                stale_after_days: 15,
            },
            lan_discovery: Mutex::new(None),
            active_incremental_runs: Mutex::new(HashSet::new()),
            mount_cache: crate::mounts::router::MountCache::default(),
            inner: RwLock::new(AppData {
                metadata: MetadataFile::default(),
                collaboration: CollaborationStore::default(),
                peer_presence: HashMap::new(),
                sessions: HashMap::from([(session.id.clone(), session)]),
                workspace_path: Some(temp_dir.to_string_lossy().to_string()),
                stale_after_days: 15,
            }),
        });

        let response = update_settings(
            State(state.clone()),
            Json(UpdateSettingsRequest {
                stale_after_days: 30,
            }),
        )
        .await
        .expect("update settings")
        .0;

        assert_eq!(response.stale_after_days, 30);
        assert_eq!(response.sessions[0].status, SessionStatus::Active);

        let inner = state.inner.read().await;
        assert_eq!(inner.stale_after_days, 30);
        assert_eq!(
            inner
                .sessions
                .get("session-1")
                .expect("session exists")
                .status,
            SessionStatus::Active
        );

        let stored = storage::load_metadata(&metadata_path).expect("load saved metadata");
        assert_eq!(stored.stale_after_days, Some(30));

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn local_collaboration_config_update_persists_display_name() {
        let temp_dir = unique_temp_dir("local-display-name");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let state = Arc::new(AppState {
            config: Config {
                bind_addr: SocketAddr::from(([127, 0, 0, 1], 4100)),
                data_dir: temp_dir.clone(),
                metadata_path: temp_dir.join("metadata.json"),
                collaboration_path: temp_dir.join("collaboration.json"),
                peer_token: Some("token-1".to_string()),
                lan_discovery_enabled: false,
                peer_display_name: "Initial".to_string(),
                archive_dir: temp_dir.join("archive"),
                max_preview_bytes: 1024,
                stale_after_days: 15,
            },
            lan_discovery: Mutex::new(None),
            active_incremental_runs: Mutex::new(HashSet::new()),
            mount_cache: crate::mounts::router::MountCache::default(),
            inner: RwLock::new(AppData {
                metadata: MetadataFile::default(),
                collaboration: CollaborationStore::default(),
                peer_presence: HashMap::new(),
                sessions: HashMap::new(),
                workspace_path: None,
                stale_after_days: 15,
            }),
        });

        let response = update_local_collaboration_config(
            State(state.clone()),
            Json(UpdateLocalCollaborationConfigRequest {
                display_name: Some("  Team Workstation  ".to_string()),
                peer_token: None,
                refresh_peer_token: None,
            }),
        )
        .await
        .expect("update local display name")
        .0;

        assert_eq!(response.local_config.display_name, "Team Workstation");
        assert_eq!(response.local_config.peer_token.as_deref(), Some("token-1"));

        let stored = storage::load_collaboration_store(&state.config.collaboration_path)
            .expect("load collaboration store");
        assert_eq!(
            stored
                .local_peer
                .as_ref()
                .expect("stored local peer")
                .display_name,
            "Team Workstation"
        );

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn local_collaboration_config_refreshes_persisted_peer_token() {
        let temp_dir = unique_temp_dir("local-token-refresh");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let state = Arc::new(AppState {
            config: Config {
                bind_addr: SocketAddr::from(([127, 0, 0, 1], 4100)),
                data_dir: temp_dir.clone(),
                metadata_path: temp_dir.join("metadata.json"),
                collaboration_path: temp_dir.join("collaboration.json"),
                peer_token: Some("token-1".to_string()),
                lan_discovery_enabled: false,
                peer_display_name: "Initial".to_string(),
                archive_dir: temp_dir.join("archive"),
                max_preview_bytes: 1024,
                stale_after_days: 15,
            },
            lan_discovery: Mutex::new(None),
            active_incremental_runs: Mutex::new(HashSet::new()),
            mount_cache: crate::mounts::router::MountCache::default(),
            inner: RwLock::new(AppData {
                metadata: MetadataFile::default(),
                collaboration: CollaborationStore {
                    local_peer_token: Some("token-1".to_string()),
                    ..CollaborationStore::default()
                },
                peer_presence: HashMap::new(),
                sessions: HashMap::new(),
                workspace_path: None,
                stale_after_days: 15,
            }),
        });

        let response = update_local_collaboration_config(
            State(state.clone()),
            Json(UpdateLocalCollaborationConfigRequest {
                display_name: None,
                peer_token: None,
                refresh_peer_token: Some(true),
            }),
        )
        .await
        .expect("refresh local peer token")
        .0;

        let refreshed = response
            .local_config
            .peer_token
            .expect("refreshed peer token");
        assert_ne!(refreshed, "token-1");
        assert_eq!(refreshed.len(), 36);

        let stored = storage::load_collaboration_store(&state.config.collaboration_path)
            .expect("load collaboration store");
        assert_eq!(stored.local_peer_token.as_deref(), Some(refreshed.as_str()));

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn app_state_persists_stable_local_peer_id() {
        let temp_dir = unique_temp_dir("stable-local-peer");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let config = Config {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 4100)),
            data_dir: temp_dir.clone(),
            metadata_path: temp_dir.join("metadata.json"),
            collaboration_path: temp_dir.join("collaboration.json"),
            peer_token: None,
            lan_discovery_enabled: false,
            peer_display_name: "Local Peer".to_string(),
            archive_dir: temp_dir.join("archive"),
            max_preview_bytes: 1024,
            stale_after_days: 15,
        };

        let first = AppState::new(config.clone()).expect("first state");
        let first_peer_id = first
            .inner
            .read()
            .await
            .collaboration
            .local_peer
            .as_ref()
            .expect("local peer")
            .peer_id
            .clone();
        drop(first);

        let second = AppState::new(Config {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 4200)),
            ..config
        })
        .expect("second state");
        let second_peer = second
            .inner
            .read()
            .await
            .collaboration
            .local_peer
            .as_ref()
            .expect("local peer")
            .clone();

        assert_eq!(second_peer.peer_id, first_peer_id);
        assert_eq!(
            second_peer.base_url.as_deref(),
            Some("http://127.0.0.1:4200")
        );

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn incremental_cursor_drops_processed_delta_and_keeps_same_timestamp_tail() {
        let timestamp = Utc.with_ymd_and_hms(2026, 5, 16, 12, 0, 0).unwrap();
        let mut deltas = vec![
            test_delta("delta_c", timestamp),
            test_delta("delta_a", timestamp),
            test_delta("delta_b", timestamp),
        ];
        let cursor = SessionDeltaCursor {
            source_id: "peer_1".to_string(),
            session_path: "subscription:sub_1".to_string(),
            last_offset: 0,
            last_record_timestamp: Some(timestamp),
            last_record_hash: Some("delta_b".to_string()),
            updated_at: Utc::now(),
        };

        sort_and_dedupe_peer_deltas(&mut deltas);
        retain_unprocessed_peer_deltas(&mut deltas, Some(&cursor));

        assert_eq!(
            deltas
                .iter()
                .map(|delta| delta.delta_id.as_str())
                .collect::<Vec<_>>(),
            vec!["delta_c"]
        );
    }

    #[tokio::test]
    async fn pair_peer_verifies_token_gated_projects_and_persists_peer() {
        let peer_temp_dir = unique_temp_dir("peer-api");
        fs::create_dir_all(&peer_temp_dir).expect("create peer temp dir");
        let session = Session {
            id: "session-1".to_string(),
            codex_session_id: Some("codex-session-1".to_string()),
            name: "Shared session".to_string(),
            excerpt: "excerpt".to_string(),
            full_content: "Changed backend/src/api.rs".to_string(),
            path: peer_temp_dir
                .join("session.md")
                .to_string_lossy()
                .to_string(),
            project_path: Some("/work/shared-project".to_string()),
            labels: vec!["share".to_string()],
            last_modified: Utc::now(),
            size: 7,
            status: SessionStatus::Active,
            notes: String::new(),
        };
        let identity = collaboration::project_identity_for_path(session.project_path.as_deref());
        let policy =
            collaboration::default_share_policy(identity.project_id, session.project_path.clone());
        let peer_state = Arc::new(AppState {
            config: Config {
                bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                data_dir: peer_temp_dir.clone(),
                metadata_path: peer_temp_dir.join("metadata.json"),
                collaboration_path: peer_temp_dir.join("collaboration.json"),
                peer_token: Some("secret".to_string()),
                lan_discovery_enabled: false,
                peer_display_name: "Peer".to_string(),
                archive_dir: peer_temp_dir.join("archive"),
                max_preview_bytes: 1024,
                stale_after_days: 15,
            },
            lan_discovery: Mutex::new(None),
            active_incremental_runs: Mutex::new(HashSet::new()),
            mount_cache: crate::mounts::router::MountCache::default(),
            inner: RwLock::new(AppData {
                metadata: MetadataFile::default(),
                collaboration: CollaborationStore {
                    project_policies: vec![policy],
                    ..CollaborationStore::default()
                },
                peer_presence: HashMap::new(),
                sessions: HashMap::from([(session.id.clone(), session)]),
                workspace_path: Some(peer_temp_dir.to_string_lossy().to_string()),
                stale_after_days: 15,
            }),
        });
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("bind peer test server");
        let peer_addr = listener.local_addr().expect("peer test address");
        let peer_base_url = format!("http://{peer_addr}");
        let server = tokio::spawn(async move {
            axum::serve(listener, router(peer_state)).await.ok();
        });

        let local_temp_dir = unique_temp_dir("local-pair");
        fs::create_dir_all(&local_temp_dir).expect("create local temp dir");
        let local_state = Arc::new(AppState {
            config: Config {
                bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                data_dir: local_temp_dir.clone(),
                metadata_path: local_temp_dir.join("metadata.json"),
                collaboration_path: local_temp_dir.join("collaboration.json"),
                peer_token: None,
                lan_discovery_enabled: false,
                peer_display_name: "Local".to_string(),
                archive_dir: local_temp_dir.join("archive"),
                max_preview_bytes: 1024,
                stale_after_days: 15,
            },
            lan_discovery: Mutex::new(None),
            active_incremental_runs: Mutex::new(HashSet::new()),
            mount_cache: crate::mounts::router::MountCache::default(),
            inner: RwLock::new(AppData {
                metadata: MetadataFile::default(),
                collaboration: CollaborationStore::default(),
                peer_presence: HashMap::from([(
                    "peer_discovered".to_string(),
                    PeerPresence {
                        peer_id: "peer_discovered".to_string(),
                        service_name: "Discovered._csm-codex._tcp.local.".to_string(),
                        display_name: "Discovered Alice".to_string(),
                        version: Some("0.1.0".to_string()),
                        base_url: peer_base_url.clone(),
                        host_name: "alice.local.".to_string(),
                        port: peer_addr.port(),
                        last_seen_at: Utc::now(),
                    },
                )]),
                sessions: HashMap::new(),
                workspace_path: None,
                stale_after_days: 15,
            }),
        });

        let response = pair_peer(
            State(local_state.clone()),
            Json(PairPeerRequest {
                peer_base_url,
                peer_access_token: Some("secret".to_string()),
                display_name: None,
            }),
        )
        .await
        .expect("pair peer")
        .0;

        assert_eq!(response.peer.peer_id, "peer_discovered");
        assert_eq!(response.peer.display_name, "Discovered Alice");
        assert_eq!(response.peer_projects.len(), 1);
        assert_eq!(response.state.store.trusted_peers.len(), 1);
        assert_eq!(
            response.state.store.trusted_peers[0]
                .access_token
                .as_deref(),
            Some("secret")
        );
        assert_eq!(response.state.store.sources.len(), 1);

        server.abort();
        fs::remove_dir_all(peer_temp_dir).ok();
        fs::remove_dir_all(local_temp_dir).ok();
    }

    #[test]
    fn baseline_peer_resolution_requires_trusted_pair() {
        let trusted_peer = PeerMetadata {
            peer_id: "peer_1".to_string(),
            display_name: "Alice".to_string(),
            trusted: true,
            public_key: None,
            base_url: Some("http://127.0.0.1:4001".to_string()),
            last_seen_at: Some(Utc::now()),
            access_token: None,
        };
        let store = CollaborationStore {
            trusted_peers: vec![trusted_peer],
            ..CollaborationStore::default()
        };

        let resolved = resolve_paired_baseline_peer(
            &store,
            &collaboration::BaselineSummaryRequest {
                peer_base_url: "http://127.0.0.1:4001".to_string(),
                peer_access_token: None,
                project_id: "project_1".to_string(),
                peer_id: None,
                peer_display_name: None,
                peer_trusted: None,
                peer_last_seen_at: None,
                days: None,
                language: None,
            },
        )
        .expect("paired peer should resolve");
        assert_eq!(resolved.peer_id, "peer_1");

        let rejected = resolve_paired_baseline_peer(
            &store,
            &collaboration::BaselineSummaryRequest {
                peer_base_url: "http://127.0.0.1:4999".to_string(),
                peer_access_token: None,
                project_id: "project_1".to_string(),
                peer_id: Some("peer_1".to_string()),
                peer_display_name: None,
                peer_trusted: None,
                peer_last_seen_at: None,
                days: None,
                language: None,
            },
        );
        assert!(rejected.is_err());
    }

    #[test]
    fn peer_request_token_prefers_request_and_falls_back_to_stored_token() {
        let peer = PeerMetadata {
            peer_id: "peer_1".to_string(),
            display_name: "Alice".to_string(),
            trusted: true,
            public_key: None,
            base_url: Some("http://127.0.0.1:4001".to_string()),
            last_seen_at: Some(Utc::now()),
            access_token: Some("stored-secret".to_string()),
        };

        assert_eq!(
            peer_request_token(Some("request-secret".to_string()), &peer).as_deref(),
            Some("request-secret")
        );
        assert_eq!(
            peer_request_token(None, &peer).as_deref(),
            Some("stored-secret")
        );
    }

    #[test]
    fn reset_subscription_delta_cursor_removes_only_matching_subscription_cursor() {
        let now = Utc.with_ymd_and_hms(2026, 5, 17, 7, 0, 0).unwrap();
        let subscription = Subscription {
            subscription_id: "sub_peer_1_project_a".to_string(),
            peer_id: "peer_1".to_string(),
            project_id: "project_a".to_string(),
            status: SubscriptionStatus::Active,
            topics: Vec::new(),
            created_at: now,
            baseline_generated_at: Some(now),
            analysis_cycle: AnalysisCycle::Hourly,
            next_run_at: Some(now + Duration::hours(1)),
            last_run_at: Some(now),
            last_run_status: Some("success".to_string()),
            last_run_error: None,
        };
        let mut store = CollaborationStore {
            delta_cursors: vec![
                SessionDeltaCursor {
                    source_id: "peer_1".to_string(),
                    session_path: "subscription:sub_peer_1_project_a".to_string(),
                    last_offset: 0,
                    last_record_timestamp: Some(now),
                    last_record_hash: Some("delta_1".to_string()),
                    updated_at: now,
                },
                SessionDeltaCursor {
                    source_id: "peer_1".to_string(),
                    session_path: "subscription:sub_peer_1_project_b".to_string(),
                    last_offset: 0,
                    last_record_timestamp: Some(now),
                    last_record_hash: Some("delta_2".to_string()),
                    updated_at: now,
                },
                SessionDeltaCursor {
                    source_id: "peer_2".to_string(),
                    session_path: "subscription:sub_peer_1_project_a".to_string(),
                    last_offset: 0,
                    last_record_timestamp: Some(now),
                    last_record_hash: Some("delta_3".to_string()),
                    updated_at: now,
                },
            ],
            ..CollaborationStore::default()
        };

        reset_subscription_delta_cursor(&mut store, &subscription);

        assert_eq!(store.delta_cursors.len(), 2);
        assert!(store.delta_cursors.iter().all(|cursor| {
            !(cursor.source_id == "peer_1"
                && cursor.session_path == "subscription:sub_peer_1_project_a")
        }));
    }

    #[test]
    fn subscription_requires_peer_to_share_project() {
        let shared_project = collaboration::PeerProject {
            project_id: "project_shared".to_string(),
            path_label: "shared".to_string(),
            active_session_count: 1,
            latest_record_at: Some(Utc::now()),
        };

        assert!(ensure_peer_project_available(&[shared_project.clone()], "project_shared").is_ok());

        let rejected = ensure_peer_project_available(&[shared_project], "project_missing");
        assert!(matches!(rejected, Err(AppError::BadRequest(_))));
    }

    #[test]
    fn peer_query_builder_percent_encodes_rfc3339_timestamps() {
        let since = Utc
            .with_ymd_and_hms(2026, 5, 16, 8, 30, 0)
            .unwrap()
            .to_rfc3339();
        let request = peer_get_request("http://127.0.0.1:4001/peer/sessions", Some("secret"))
            .query(&[
                ("projectId", "project_git_abc".to_string()),
                ("since", since),
                ("limit", "100".to_string()),
            ])
            .build()
            .expect("build peer request");
        let query = request.url().query().expect("query string");

        assert!(query.contains("since=2026-05-16T08%3A30%3A00%2B00%3A00"));
        assert_eq!(
            request
                .headers()
                .get("x-csm-peer-token")
                .and_then(|value| value.to_str().ok()),
            Some("secret")
        );
    }

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("csm-api-{prefix}-{}-{stamp}", std::process::id()))
    }

    fn test_state(temp_dir: &std::path::Path, sessions: HashMap<String, Session>) -> SharedState {
        Arc::new(AppState {
            config: Config {
                bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                data_dir: temp_dir.to_path_buf(),
                metadata_path: temp_dir.join("metadata.json"),
                collaboration_path: temp_dir.join("collaboration.json"),
                peer_token: None,
                lan_discovery_enabled: false,
                peer_display_name: "Test".to_string(),
                archive_dir: temp_dir.join("archive"),
                max_preview_bytes: 1024,
                stale_after_days: 15,
            },
            lan_discovery: Mutex::new(None),
            active_incremental_runs: Mutex::new(HashSet::new()),
            mount_cache: crate::mounts::router::MountCache::default(),
            inner: RwLock::new(AppData {
                metadata: MetadataFile::default(),
                collaboration: CollaborationStore::default(),
                peer_presence: HashMap::new(),
                sessions,
                workspace_path: Some(temp_dir.to_string_lossy().to_string()),
                stale_after_days: 15,
            }),
        })
    }

    fn test_session(
        id: &str,
        name: &str,
        project_path: Option<String>,
        status: SessionStatus,
    ) -> Session {
        Session {
            id: id.to_string(),
            codex_session_id: None,
            name: name.to_string(),
            excerpt: "excerpt".to_string(),
            full_content: "content".to_string(),
            path: format!("/tmp/{id}.md"),
            project_path,
            labels: Vec::new(),
            last_modified: Utc::now(),
            size: 7,
            status,
            notes: String::new(),
        }
    }

    fn test_delta(id: &str, timestamp: chrono::DateTime<Utc>) -> crate::models::SessionDelta {
        crate::models::SessionDelta {
            delta_id: id.to_string(),
            session_id: "session_1".to_string(),
            project_id: "project_1".to_string(),
            timestamp,
            role: "assistant".to_string(),
            kind: "message".to_string(),
            text_excerpt: "changed file".to_string(),
            paths_mentioned: Vec::new(),
            commands_mentioned: Vec::new(),
            git_refs: Vec::new(),
            redaction_result: crate::models::RedactionResult {
                status: crate::models::RedactionStatus::Clean,
                reasons: Vec::new(),
                redacted_text: "changed file".to_string(),
                original_char_count: 12,
                redacted_char_count: 12,
            },
        }
    }
}
