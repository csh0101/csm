use std::{
    collections::HashSet,
    env,
    net::{IpAddr, SocketAddr, TcpListener},
    sync::Arc,
};

use backend::{
    api::{self, ScanResponse, SessionMutationResponse, SessionsResponse},
    archive,
    config::Config,
    error::AppError,
    models::{MetadataFile, Session, SessionMeta, SessionStatus},
    scanner,
    state::{AppState, SharedState},
    storage,
    summary::{self, ActivitySummaryRequest, ActivitySummaryResponse},
};
use tauri::Manager;

#[tauri::command]
async fn get_sessions(state: tauri::State<'_, SharedState>) -> Result<SessionsResponse, String> {
    let inner = state.inner.read().await;
    let mut sessions = inner.sessions.values().cloned().collect::<Vec<_>>();
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    let stale_after_days = inner.stale_after_days;

    Ok(SessionsResponse {
        workspace_path: inner.workspace_path.clone(),
        counts: api::filter_counts(&sessions, stale_after_days),
        labels: api::label_counts(&sessions),
        sessions,
        stale_after_days,
    })
}

#[tauri::command]
async fn scan_sessions(
    path: String,
    state: tauri::State<'_, SharedState>,
) -> Result<ScanResponse, String> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err("workspace path cannot be empty".to_string());
    }

    let (metadata, stale_after_days) = {
        let inner = state.inner.read().await;
        (inner.metadata.clone(), inner.stale_after_days)
    };

    let scan = scanner::scan_workspace(
        &path,
        &metadata,
        state.config.max_preview_bytes,
        stale_after_days,
    )
    .map_err(error_message)?;
    let sessions = scan.sessions;

    {
        let mut inner = state.inner.write().await;
        let mut metadata = inner.metadata.clone();
        metadata.workspace_path = Some(scan.workspace_path.clone());
        storage::save_metadata(&state.config.metadata_path, &metadata).map_err(error_message)?;

        inner.metadata = metadata;
        inner.workspace_path = Some(scan.workspace_path.clone());
        inner.sessions = sessions
            .iter()
            .cloned()
            .map(|session| (session.id.clone(), session))
            .collect();
    }

    Ok(ScanResponse {
        workspace_path: scan.workspace_path,
        counts: api::filter_counts(&sessions, stale_after_days),
        labels: api::label_counts(&sessions),
        sessions,
        stale_after_days,
        skipped_files: scan.skipped_files,
    })
}

#[tauri::command]
async fn update_settings(
    stale_after_days: i64,
    state: tauri::State<'_, SharedState>,
) -> Result<SessionsResponse, String> {
    if !(1..=3650).contains(&stale_after_days) {
        return Err("staleAfterDays must be between 1 and 3650".to_string());
    }

    let mut sessions = {
        let mut inner = state.inner.write().await;
        let mut metadata = inner.metadata.clone();
        metadata.stale_after_days = Some(stale_after_days);

        let mut sessions = inner.sessions.values().cloned().collect::<Vec<_>>();
        apply_stale_threshold(&mut sessions, &metadata, stale_after_days);
        storage::save_metadata(&state.config.metadata_path, &metadata).map_err(error_message)?;

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

    Ok(SessionsResponse {
        workspace_path,
        counts: api::filter_counts(&sessions, stale_after_days),
        labels: api::label_counts(&sessions),
        sessions,
        stale_after_days,
    })
}

#[tauri::command]
async fn update_session_labels(
    id: String,
    labels: Vec<String>,
    state: tauri::State<'_, SharedState>,
) -> Result<SessionMutationResponse, String> {
    let labels = normalize_labels(labels);
    let session = {
        let mut inner = state.inner.write().await;
        let mut session = inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("session '{id}' was not found"))?;

        session.labels = labels.clone();
        let mut metadata = inner.metadata.clone();
        let meta = ensure_meta(&mut metadata, &session);
        meta.labels = labels;
        meta.updated_at = chrono::Utc::now();
        storage::save_metadata(&state.config.metadata_path, &metadata).map_err(error_message)?;

        inner.metadata = metadata;
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    };

    Ok(SessionMutationResponse {
        session,
        archive_record: None,
    })
}

#[tauri::command]
async fn update_session_notes(
    id: String,
    notes: String,
    state: tauri::State<'_, SharedState>,
) -> Result<SessionMutationResponse, String> {
    let session = {
        let mut inner = state.inner.write().await;
        let mut session = inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("session '{id}' was not found"))?;

        session.notes = notes.clone();
        let mut metadata = inner.metadata.clone();
        let meta = ensure_meta(&mut metadata, &session);
        meta.notes = notes;
        meta.updated_at = chrono::Utc::now();
        storage::save_metadata(&state.config.metadata_path, &metadata).map_err(error_message)?;

        inner.metadata = metadata;
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    };

    Ok(SessionMutationResponse {
        session,
        archive_record: None,
    })
}

#[tauri::command]
async fn archive_delete_session(
    id: String,
    state: tauri::State<'_, SharedState>,
) -> Result<SessionMutationResponse, String> {
    let session = {
        let inner = state.inner.read().await;
        inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("session '{id}' was not found"))?
    };

    let archive_record =
        archive::archive_session(&state.config, &session).map_err(error_message)?;
    let session = {
        let mut inner = state.inner.write().await;
        let mut session = inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("session '{id}' was not found"))?;

        session.status = SessionStatus::Deleted;
        let mut metadata = inner.metadata.clone();
        let meta = ensure_meta(&mut metadata, &session);
        meta.status_override = Some(SessionStatus::Deleted);
        meta.updated_at = chrono::Utc::now();
        metadata.archive_records.push(archive_record.clone());
        storage::save_metadata(&state.config.metadata_path, &metadata).map_err(error_message)?;

        inner.metadata = metadata;
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    };

    Ok(SessionMutationResponse {
        session,
        archive_record: Some(archive_record),
    })
}

#[tauri::command]
async fn restore_session(
    id: String,
    state: tauri::State<'_, SharedState>,
) -> Result<SessionMutationResponse, String> {
    let session = {
        let mut inner = state.inner.write().await;
        let mut session = inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("session '{id}' was not found"))?;

        session.status = SessionStatus::Active;
        let mut metadata = inner.metadata.clone();
        let meta = ensure_meta(&mut metadata, &session);
        meta.status_override = Some(SessionStatus::Active);
        meta.updated_at = chrono::Utc::now();
        storage::save_metadata(&state.config.metadata_path, &metadata).map_err(error_message)?;

        inner.metadata = metadata;
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    };

    Ok(SessionMutationResponse {
        session,
        archive_record: None,
    })
}

#[tauri::command]
async fn generate_activity_summary(
    days: Option<i64>,
    language: Option<String>,
    state: tauri::State<'_, SharedState>,
) -> Result<ActivitySummaryResponse, String> {
    let sessions = {
        let inner = state.inner.read().await;
        inner.sessions.values().cloned().collect::<Vec<_>>()
    };

    summary::generate_activity_summary(sessions, ActivitySummaryRequest { days, language })
        .await
        .map_err(error_message)
}

#[tauri::command]
fn get_collaboration_api_base_url(state: tauri::State<'_, SharedState>) -> String {
    local_api_base_url(state.config.bind_addr)
}

fn config_for_tauri(app: &tauri::App) -> Result<Config, String> {
    let mut config = Config::from_env();
    if env::var_os("CSM_DATA_DIR").is_none() {
        let data_dir = app.path().app_data_dir().map_err(error_message)?;
        config.data_dir = data_dir.clone();
        config.metadata_path = data_dir.join("metadata.json");
        config.collaboration_path = data_dir.join("collaboration.json");

        if env::var_os("CSM_ARCHIVE_DIR").is_none() {
            config.archive_dir = data_dir.join("archive");
        }
    }

    Ok(config)
}

struct LocalApiBinding {
    bind_addr: SocketAddr,
    listener: TcpListener,
}

fn bind_local_api(config: &mut Config) -> Result<LocalApiBinding, String> {
    let requested_addr = if env::var_os("CSM_BIND_ADDR").is_some() {
        config.bind_addr
    } else {
        SocketAddr::from(([127, 0, 0, 1], 0))
    };
    let listener = TcpListener::bind(requested_addr).map_err(error_message)?;
    let bind_addr = listener.local_addr().map_err(error_message)?;
    config.bind_addr = bind_addr;

    Ok(LocalApiBinding {
        bind_addr,
        listener,
    })
}

fn local_api_base_url(bind_addr: SocketAddr) -> String {
    let host = match bind_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_string(),
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    };

    format!("http://{}:{}", host, bind_addr.port())
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

fn ensure_meta<'a>(metadata: &'a mut MetadataFile, session: &Session) -> &'a mut SessionMeta {
    metadata
        .sessions
        .entry(session.id.clone())
        .or_insert_with(|| SessionMeta {
            session_id: session.id.clone(),
            labels: session.labels.clone(),
            notes: session.notes.clone(),
            status_override: None,
            updated_at: chrono::Utc::now(),
        })
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

fn error_message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let mut config = config_for_tauri(app).map_err(AppError::BadRequest)?;
            let binding = bind_local_api(&mut config).map_err(AppError::BadRequest)?;
            let state = AppState::new(config)?;
            let api_state = Arc::clone(&state);
            tauri::async_runtime::spawn(async move {
                if let Err(error) =
                    backend::server::serve_std_listener(api_state, binding.listener).await
                {
                    eprintln!(
                        "failed to start local API server on http://{}: {error}",
                        binding.bind_addr
                    );
                }
            });
            app.manage(Arc::clone(&state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_sessions,
            scan_sessions,
            update_settings,
            update_session_labels,
            update_session_notes,
            archive_delete_session,
            restore_session,
            generate_activity_summary,
            get_collaboration_api_base_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
