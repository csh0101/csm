use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Take},
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    config::expand_tilde,
    error::AppError,
    models::{MetadataFile, Session, SessionMeta, SessionStatus},
};

const TEXT_EXTENSIONS: &[&str] = &[
    "jsonl", "json", "md", "markdown", "txt", "log", "toml", "yaml", "yml",
];
const JSONL_PROJECT_PATH_RECORD_LIMIT: usize = 64;

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub workspace_path: String,
    pub sessions: Vec<Session>,
    pub skipped_files: usize,
}

#[derive(Debug)]
struct ContentPreview {
    text: String,
    title: Option<String>,
    project_path: Option<String>,
    codex_session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexProjectPathRecord {
    #[serde(rename = "type")]
    record_type: Option<String>,
    payload: Option<CodexProjectPathPayload>,
}

#[derive(Debug, Deserialize)]
struct CodexProjectPathPayload {
    cwd: Option<String>,
}

pub fn scan_workspace(
    workspace_path: &str,
    metadata: &MetadataFile,
    max_preview_bytes: usize,
    stale_after_days: i64,
) -> Result<ScanResult, AppError> {
    let root = expand_tilde(workspace_path);
    let root = root.canonicalize().map_err(|error| {
        AppError::BadRequest(format!(
            "cannot access workspace path '{}': {error}",
            workspace_path
        ))
    })?;

    if !root.is_dir() {
        return Err(AppError::BadRequest(format!(
            "workspace path '{}' is not a directory",
            root.display()
        )));
    }

    let mut sessions = Vec::new();
    let mut skipped_files = 0;

    for entry in WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.path() == root.as_path() || !is_hidden_dir(entry.path()))
    {
        let Ok(entry) = entry else {
            skipped_files += 1;
            continue;
        };

        if !entry.file_type().is_file() || !is_supported_text_file(entry.path()) {
            continue;
        }

        match scan_file(entry.path(), metadata, max_preview_bytes, stale_after_days) {
            Ok(session) => sessions.push(session),
            Err(_) => skipped_files += 1,
        }
    }

    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    Ok(ScanResult {
        workspace_path: root.to_string_lossy().to_string(),
        sessions,
        skipped_files,
    })
}

fn scan_file(
    path: &Path,
    metadata_file: &MetadataFile,
    max_preview_bytes: usize,
    stale_after_days: i64,
) -> Result<Session, AppError> {
    let file_metadata = fs::metadata(path)?;
    let path = path.canonicalize()?;
    let path_string = path.to_string_lossy().to_string();
    let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, path_string.as_bytes()).to_string();
    let preview = read_content_preview(&path, max_preview_bytes)?;
    let metadata = metadata_file.sessions.get(&id);
    let last_modified = system_time_to_utc(file_metadata.modified()?);
    let base_status = status_from_last_modified(last_modified, stale_after_days);

    let project_path = preview.project_path;
    let labels = labels_from_metadata_and_project(metadata, project_path.as_deref());

    Ok(Session {
        id: id.clone(),
        codex_session_id: preview.codex_session_id,
        name: preview
            .title
            .or_else(|| title_from_content(&preview.text))
            .unwrap_or_else(|| name_from_path(&path)),
        excerpt: excerpt_from_content(&preview.text),
        full_content: preview.text,
        path: path_string,
        project_path,
        labels,
        last_modified,
        size: file_metadata.len(),
        status: metadata
            .and_then(|meta| meta.status_override.clone())
            .unwrap_or(base_status),
        notes: metadata.map(|meta| meta.notes.clone()).unwrap_or_default(),
    })
}

fn labels_from_metadata_and_project(
    metadata: Option<&SessionMeta>,
    project_path: Option<&str>,
) -> Vec<String> {
    let mut labels = metadata
        .map(|metadata| metadata.labels.clone())
        .unwrap_or_default();

    if let Some(project_label) = project_label_from_path(project_path) {
        push_label_once(&mut labels, project_label);
    }

    labels
}

fn project_label_from_path(project_path: Option<&str>) -> Option<String> {
    let project_path = project_path?.trim();
    if project_path.is_empty() {
        return None;
    }

    Path::new(project_path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn push_label_once(labels: &mut Vec<String>, label: String) {
    if !labels.iter().any(|existing| existing == &label) {
        labels.push(label);
    }
}

fn is_hidden_dir(path: &Path) -> bool {
    path.is_dir()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.') && name != ".")
}

fn is_supported_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            TEXT_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
        .unwrap_or(false)
}

fn read_content_preview(path: &Path, max_preview_bytes: usize) -> Result<ContentPreview, AppError> {
    let is_jsonl = is_jsonl_file(path);
    let (raw, truncated) = if is_jsonl {
        read_jsonl_preview_text(path, max_preview_bytes)?
    } else {
        read_limited_text(path, max_preview_bytes)?
    };
    let mut preview = if is_jsonl {
        let mut preview = parse_codex_jsonl_preview(&raw).unwrap_or(ContentPreview {
            text: raw,
            title: None,
            project_path: None,
            codex_session_id: None,
        });

        if preview.project_path.is_none() {
            preview.project_path = codex_project_path_from_jsonl_file(path)?;
        }
        if preview.codex_session_id.is_none() {
            preview.codex_session_id = codex_session_id_from_path(path);
        }

        preview
    } else {
        ContentPreview {
            text: raw,
            title: None,
            project_path: None,
            codex_session_id: None,
        }
    };

    if truncated {
        preview.text.push_str("\n\n[preview truncated]");
    }

    Ok(preview)
}

fn codex_project_path_from_jsonl_file(path: &Path) -> Result<Option<String>, AppError> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    for _ in 0..JSONL_PROJECT_PATH_RECORD_LIMIT {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let Ok(record) = serde_json::from_str::<CodexProjectPathRecord>(&line) else {
            continue;
        };

        if record.record_type.as_deref() != Some("session_meta") {
            continue;
        }

        let cwd = record
            .payload
            .and_then(|payload| payload.cwd)
            .map(|cwd| cwd.trim().to_string())
            .filter(|cwd| !cwd.is_empty());

        if cwd.is_some() {
            return Ok(cwd);
        }
    }

    Ok(None)
}

fn read_limited_text(path: &Path, max_preview_bytes: usize) -> Result<(String, bool), AppError> {
    let file = File::open(path)?;
    let mut reader: Take<File> = file.take(max_preview_bytes as u64 + 1);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;

    let truncated = bytes.len() > max_preview_bytes;
    if truncated {
        bytes.truncate(max_preview_bytes);
    }

    let mut content = String::from_utf8_lossy(&bytes).to_string();

    if truncated {
        content = content.trim_end().to_string();
    }

    Ok((content, truncated))
}

fn read_jsonl_preview_text(
    path: &Path,
    max_preview_bytes: usize,
) -> Result<(String, bool), AppError> {
    let file_size = fs::metadata(path)?.len() as usize;
    if file_size <= max_preview_bytes {
        return read_limited_text(path, max_preview_bytes);
    }

    let head_budget = (max_preview_bytes / 2).max(1);
    let tail_budget = max_preview_bytes.saturating_sub(head_budget).max(1);
    let mut file = File::open(path)?;

    let mut head_bytes = vec![0; head_budget.min(file_size)];
    file.read_exact(&mut head_bytes)?;

    let tail_start = file_size.saturating_sub(tail_budget) as u64;
    file.seek(SeekFrom::Start(tail_start))?;
    let mut tail_bytes = Vec::new();
    file.read_to_end(&mut tail_bytes)?;

    let head = trim_after_last_newline(String::from_utf8_lossy(&head_bytes).to_string());
    let tail = trim_before_first_newline(String::from_utf8_lossy(&tail_bytes).to_string());
    let content = format!("{head}\n\n[preview middle truncated]\n\n{tail}")
        .trim()
        .to_string();

    Ok((content, true))
}

fn trim_after_last_newline(value: String) -> String {
    if value.ends_with('\n') {
        return value;
    }

    value
        .rfind('\n')
        .map(|index| value[..index].to_string())
        .unwrap_or(value)
}

fn trim_before_first_newline(value: String) -> String {
    value
        .find('\n')
        .map(|index| value[index + 1..].to_string())
        .unwrap_or(value)
}

fn is_jsonl_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

fn parse_codex_jsonl_preview(raw: &str) -> Option<ContentPreview> {
    let mut codex_session_id = None;
    let mut project_path = None;
    let mut response_messages = Vec::<TranscriptMessage>::new();
    let mut event_messages = Vec::<TranscriptMessage>::new();

    for line in raw.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if project_path.is_none() {
            project_path = codex_project_path_from_record(&value);
        }
        if codex_session_id.is_none() {
            codex_session_id = codex_session_id_from_record(&value);
        }

        let Some((is_response_item, role, text)) = codex_message_from_record(&value) else {
            continue;
        };

        let text = clean_transcript_text(&text);
        if text.is_empty() {
            continue;
        }

        let message = TranscriptMessage {
            timestamp: codex_timestamp_from_record(&value),
            role,
            text,
        };

        if is_response_item {
            response_messages.push(message);
        } else {
            event_messages.push(message);
        }
    }

    let messages = if response_messages.is_empty() {
        event_messages
    } else {
        response_messages
    };

    if messages.is_empty() && project_path.is_none() && codex_session_id.is_none() {
        return None;
    }

    let title = messages.iter().find_map(|message| {
        if message.role == "User" {
            first_meaningful_line(&message.text).map(|line| limit_chars(line, 96))
        } else {
            None
        }
    });

    let text = if messages.is_empty() {
        raw.to_string()
    } else {
        messages
            .into_iter()
            .map(|message| {
                if let Some(timestamp) = message.timestamp {
                    format!("## {} ({timestamp})\n\n{}", message.role, message.text)
                } else {
                    format!("## {}\n\n{}", message.role, message.text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    Some(ContentPreview {
        text,
        title,
        project_path,
        codex_session_id,
    })
}

struct TranscriptMessage {
    timestamp: Option<String>,
    role: String,
    text: String,
}

fn codex_timestamp_from_record(value: &Value) -> Option<String> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|timestamp| !timestamp.is_empty())
        .map(str::to_string)
}

fn codex_session_id_from_record(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str)? != "session_meta" {
        return None;
    }

    let id = value
        .get("payload")
        .and_then(|payload| payload.get("id"))
        .and_then(Value::as_str)?
        .trim();

    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn codex_session_id_from_path(path: &Path) -> Option<String> {
    let file_stem = path.file_stem()?.to_str()?;
    let id = file_stem.get(file_stem.len().checked_sub(36)?..)?;

    if is_uuid_like(id) {
        Some(id.to_string())
    } else {
        None
    }
}

fn is_uuid_like(value: &str) -> bool {
    value.len() == 36
        && value.chars().enumerate().all(|(index, ch)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                ch == '-'
            } else {
                ch.is_ascii_hexdigit()
            }
        })
}

fn codex_project_path_from_record(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str)? != "session_meta" {
        return None;
    }

    let cwd = value
        .get("payload")
        .and_then(|payload| payload.get("cwd"))
        .and_then(Value::as_str)?
        .trim();

    if cwd.is_empty() {
        None
    } else {
        Some(cwd.to_string())
    }
}

fn codex_message_from_record(value: &Value) -> Option<(bool, String, String)> {
    let payload = value.get("payload")?;
    match value.get("type").and_then(Value::as_str)? {
        "response_item" => {
            if payload.get("type").and_then(Value::as_str)? != "message" {
                return None;
            }

            let role = match payload.get("role").and_then(Value::as_str)? {
                "user" => "User",
                "assistant" => "Assistant",
                _ => return None,
            };
            let text = text_from_content(payload.get("content")?)?;
            Some((true, role.to_string(), text))
        }
        "event_msg" => {
            let role = match payload.get("type").and_then(Value::as_str)? {
                "user_message" => "User",
                "agent_message" => "Assistant",
                _ => return None,
            };
            let text = payload
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| text_from_content(payload.get("text_elements")?))?;
            Some((false, role.to_string(), text))
        }
        _ => None,
    }
}

fn text_from_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n");

            if text.trim().is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

fn clean_transcript_text(text: &str) -> String {
    let mut cleaned = Vec::new();
    let mut in_environment_context = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "<environment_context>" {
            in_environment_context = true;
            continue;
        }

        if trimmed == "</environment_context>" {
            in_environment_context = false;
            continue;
        }

        if !in_environment_context {
            cleaned.push(line);
        }
    }

    cleaned.join("\n").trim().to_string()
}

fn first_meaningful_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn system_time_to_utc(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

pub fn status_from_last_modified(
    last_modified: DateTime<Utc>,
    stale_after_days: i64,
) -> SessionStatus {
    if Utc::now().signed_duration_since(last_modified) > Duration::days(stale_after_days) {
        SessionStatus::Stale
    } else {
        SessionStatus::Active
    }
}

fn title_from_content(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        let title = trimmed.strip_prefix("# ")?;
        let title = title.trim();

        if title.is_empty() {
            None
        } else {
            Some(limit_chars(title, 96))
        }
    })
}

fn name_from_path(path: &PathBuf) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .and_then(|name| name.to_str())
        .map(|name| name.replace(['_', '-'], " "))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Untitled session".to_string())
}

fn excerpt_from_content(content: &str) -> String {
    let excerpt = content
        .lines()
        .map(str::trim)
        .filter(|line| !is_transcript_heading(line))
        .find(|line| !line.is_empty())
        .map(clean_excerpt)
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "No text content preview available.".to_string());

    limit_chars(&excerpt, 180)
}

fn is_transcript_heading(line: &str) -> bool {
    matches!(line, "## User" | "## Assistant")
        || line.starts_with("## User (")
        || line.starts_with("## Assistant (")
}

fn clean_excerpt(line: &str) -> String {
    line.trim_start_matches('#')
        .trim_matches('`')
        .trim()
        .replace('\t', " ")
}

fn limit_chars(value: &str, max_chars: usize) -> String {
    let mut limited = value.chars().take(max_chars).collect::<String>();

    if value.chars().count() > max_chars {
        limited.push_str("...");
    }

    limited
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn scans_hidden_workspace_root() {
        let temp_dir = unique_temp_dir("hidden-root");
        let root = temp_dir.join(".codex");
        let sessions_dir = root.join("sessions");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");
        fs::write(
            sessions_dir.join("session.md"),
            "# Hidden root session\nbody",
        )
        .expect("write session");

        let scan = scan_workspace(
            root.to_str().expect("utf-8 temp path"),
            &MetadataFile::default(),
            1024,
            15,
        )
        .expect("scan hidden root");

        assert_eq!(scan.sessions.len(), 1);
        assert_eq!(scan.sessions[0].name, "Hidden root session");

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn skips_hidden_subdirectories() {
        let temp_dir = unique_temp_dir("hidden-subdir");
        let root = temp_dir.join("workspace");
        fs::create_dir_all(root.join(".git")).expect("create hidden dir");
        fs::write(root.join("visible.md"), "# Visible session\nbody").expect("write visible");
        fs::write(
            root.join(".git").join("ignored.md"),
            "# Ignored session\nbody",
        )
        .expect("write ignored");

        let scan = scan_workspace(
            root.to_str().expect("utf-8 temp path"),
            &MetadataFile::default(),
            1024,
            15,
        )
        .expect("scan workspace");

        assert_eq!(scan.sessions.len(), 1);
        assert_eq!(scan.sessions[0].name, "Visible session");

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn renders_codex_jsonl_as_markdown_transcript() {
        let temp_dir = unique_temp_dir("jsonl-transcript");
        let root = temp_dir.join("workspace");
        fs::create_dir_all(&root).expect("create workspace");
        fs::write(
            root.join("rollout.jsonl"),
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"session_meta","payload":{"id":"abc","cwd":"/tmp/project"}}
{"timestamp":"2026-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Build the Rust backend\nwith scanner support"}]}}
{"timestamp":"2026-01-01T00:00:02Z","type":"event_msg","payload":{"type":"agent_message","message":"duplicate streaming output"}}
{"timestamp":"2026-01-01T00:00:03Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Implemented the scan endpoint."}]}}"#,
        )
        .expect("write jsonl");

        let scan = scan_workspace(
            root.to_str().expect("utf-8 temp path"),
            &MetadataFile::default(),
            4096,
            15,
        )
        .expect("scan workspace");

        let session = &scan.sessions[0];
        assert_eq!(session.name, "Build the Rust backend");
        assert_eq!(session.excerpt, "Build the Rust backend");
        assert_eq!(session.codex_session_id.as_deref(), Some("abc"));
        assert_eq!(session.project_path.as_deref(), Some("/tmp/project"));
        assert_eq!(session.labels, vec!["project".to_string()]);
        assert!(
            session
                .full_content
                .contains("## User (2026-01-01T00:00:01Z)")
        );
        assert!(session.full_content.contains("Build the Rust backend"));
        assert!(
            session
                .full_content
                .contains("## Assistant (2026-01-01T00:00:03Z)")
        );
        assert!(
            session
                .full_content
                .contains("Implemented the scan endpoint.")
        );
        assert!(!session.full_content.contains("duplicate streaming output"));
        assert!(!session.full_content.contains("\"payload\""));

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn jsonl_preview_includes_tail_records_for_long_sessions() {
        let temp_dir = unique_temp_dir("jsonl-tail");
        let root = temp_dir.join("workspace");
        fs::create_dir_all(&root).expect("create workspace");
        let filler = "x".repeat(4096);
        fs::write(
            root.join("rollout.jsonl"),
            format!(
                r##"{{"timestamp":"2026-04-01T00:00:00Z","type":"session_meta","payload":{{"id":"abc","cwd":"/tmp/dev-machine"}}}}
{{"timestamp":"2026-04-01T00:00:01Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"# AGENTS.md instructions for /tmp/dev-machine"}}]}}}}
{{"timestamp":"2026-04-01T00:00:02Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"{filler}"}}]}}}}
{{"timestamp":"2026-05-14T12:58:34Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"Committed 7dbf9e4 Fix node type scheduling from SKU and pushed ack-fat."}}]}}}}"##
            ),
        )
        .expect("write jsonl");

        let scan = scan_workspace(
            root.to_str().expect("utf-8 temp path"),
            &MetadataFile::default(),
            512,
            15,
        )
        .expect("scan workspace");

        let session = &scan.sessions[0];
        assert_eq!(session.project_path.as_deref(), Some("/tmp/dev-machine"));
        assert!(session.full_content.contains("[preview truncated]"));
        assert!(
            session
                .full_content
                .contains("## Assistant (2026-05-14T12:58:34Z)")
        );
        assert!(
            session
                .full_content
                .contains("Fix node type scheduling from SKU")
        );

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn extracts_codex_session_id_from_rollout_filename_when_metadata_is_absent() {
        let temp_dir = unique_temp_dir("jsonl-filename-session-id");
        let root = temp_dir.join("workspace");
        fs::create_dir_all(&root).expect("create workspace");
        fs::write(
            root.join("rollout-2026-05-15T20-03-14-019e2b84-f726-7923-a314-a437029866ca.jsonl"),
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Resume me"}]}}"#,
        )
        .expect("write jsonl");

        let scan = scan_workspace(
            root.to_str().expect("utf-8 temp path"),
            &MetadataFile::default(),
            4096,
            15,
        )
        .expect("scan workspace");

        assert_eq!(
            scan.sessions[0].codex_session_id.as_deref(),
            Some("019e2b84-f726-7923-a314-a437029866ca")
        );

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn preserves_metadata_labels_and_adds_project_label() {
        let temp_dir = unique_temp_dir("project-label");
        let root = temp_dir.join("workspace");
        fs::create_dir_all(&root).expect("create workspace");
        let path = root.join("rollout.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"session_meta","payload":{"id":"abc","cwd":"/Users/example/codex-session-manager"}}
{"timestamp":"2026-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Check labels"}]}}"#,
        )
        .expect("write jsonl");

        let id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            path.canonicalize()
                .expect("canonical path")
                .to_string_lossy()
                .as_bytes(),
        )
        .to_string();
        let mut metadata = MetadataFile::default();
        metadata.sessions.insert(
            id.clone(),
            SessionMeta {
                session_id: id,
                labels: vec!["manual".to_string()],
                notes: String::new(),
                status_override: None,
                updated_at: Utc::now(),
            },
        );

        let scan = scan_workspace(root.to_str().expect("utf-8 temp path"), &metadata, 4096, 15)
            .expect("scan workspace");

        assert_eq!(
            scan.sessions[0].labels,
            vec!["manual".to_string(), "codex-session-manager".to_string()]
        );

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn extracts_project_path_when_session_meta_exceeds_preview_limit() {
        let temp_dir = unique_temp_dir("jsonl-large-meta");
        let root = temp_dir.join("workspace");
        fs::create_dir_all(&root).expect("create workspace");
        let ignored_instructions = "x".repeat(2048);
        fs::write(
            root.join("rollout.jsonl"),
            format!(
                r#"{{"timestamp":"2026-01-01T00:00:00Z","type":"session_meta","payload":{{"id":"abc","cwd":"/tmp/large-meta","base_instructions":{{"text":"{ignored_instructions}"}}}}}}
{{"timestamp":"2026-01-01T00:00:01Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"After metadata"}}]}}}}"#
            ),
        )
        .expect("write jsonl");

        let scan = scan_workspace(
            root.to_str().expect("utf-8 temp path"),
            &MetadataFile::default(),
            128,
            15,
        )
        .expect("scan workspace");

        let session = &scan.sessions[0];
        assert_eq!(session.project_path.as_deref(), Some("/tmp/large-meta"));
        assert!(session.full_content.contains("[preview truncated]"));

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn strips_environment_context_from_codex_jsonl_title() {
        let temp_dir = unique_temp_dir("jsonl-env-context");
        let root = temp_dir.join("workspace");
        fs::create_dir_all(&root).expect("create workspace");
        fs::write(
            root.join("rollout.jsonl"),
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>\n  <cwd>/secret/project</cwd>\n</environment_context>\nContinue the implementation"}]}}"#,
        )
        .expect("write jsonl");

        let scan = scan_workspace(
            root.to_str().expect("utf-8 temp path"),
            &MetadataFile::default(),
            4096,
            15,
        )
        .expect("scan workspace");

        let session = &scan.sessions[0];
        assert_eq!(session.name, "Continue the implementation");
        assert!(!session.full_content.contains("<cwd>"));
        assert!(session.full_content.contains("Continue the implementation"));

        fs::remove_dir_all(temp_dir).ok();
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "csm-scanner-{prefix}-{}-{stamp}",
            std::process::id()
        ))
    }
}
