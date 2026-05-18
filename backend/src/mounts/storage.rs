use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::Utc;

use crate::{
    error::AppError,
    mounts::models::{
        CredentialProfile, CredentialSecret, MountAuditRecord, MountKey, MountPolicy, MountRecord,
        MountStore, mount_key,
    },
};

static MOUNT_STORE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn mounts_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("mounts.json")
}

pub fn load_mount_store(path: &Path) -> Result<MountStore, AppError> {
    if !path.exists() {
        return Ok(MountStore::default());
    }

    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn with_mount_store<T>(
    path: &Path,
    mutate: impl FnOnce(&mut MountStore) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = MOUNT_STORE_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| AppError::External("mount store lock poisoned".to_string()))?;
    let mut store = load_mount_store(path)?;
    let result = mutate(&mut store)?;
    save_mount_store(path, &store)?;
    Ok(result)
}

pub fn save_mount_store(path: &Path, store: &MountStore) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let raw = serde_json::to_string_pretty(store)?;
    let temp_path = temp_mount_store_path(path);
    let write_result = (|| -> Result<(), AppError> {
        let mut file = File::create(&temp_path)?;
        file.write_all(raw.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result?;
    Ok(())
}

fn temp_mount_store_path(path: &Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mounts.json");
    path.with_file_name(format!("{file_name}.tmp-{pid}-{nanos}-{counter}"))
}

pub fn find_mount<'a>(
    store: &'a MountStore,
    project_id: &str,
    mount_id: &str,
) -> Option<&'a MountRecord> {
    let key = mount_key(project_id, mount_id);
    store.mounts.iter().find(|mount| mount.key() == key)
}

pub fn find_mount_mut<'a>(
    store: &'a mut MountStore,
    project_id: &str,
    mount_id: &str,
) -> Option<&'a mut MountRecord> {
    let key = mount_key(project_id, mount_id);
    store.mounts.iter_mut().find(|mount| mount.key() == key)
}

pub fn find_mount_by_mount_point<'a>(
    store: &'a MountStore,
    mount_point: &str,
) -> Option<&'a MountRecord> {
    store
        .mounts
        .iter()
        .find(|mount| mount.mount_point == mount_point)
}

pub fn find_policy<'a>(store: &'a MountStore, policy_id: &str) -> Option<&'a MountPolicy> {
    store
        .policies
        .iter()
        .find(|policy| policy.policy_id == policy_id)
}

pub fn find_policy_mut<'a>(
    store: &'a mut MountStore,
    policy_id: &str,
) -> Option<&'a mut MountPolicy> {
    store
        .policies
        .iter_mut()
        .find(|policy| policy.policy_id == policy_id)
}

pub fn find_credential_profile<'a>(
    store: &'a MountStore,
    profile_id: &str,
) -> Option<&'a CredentialProfile> {
    store
        .credential_profiles
        .iter()
        .find(|profile| profile.profile_id == profile_id)
}

pub fn find_credential_secret<'a>(
    store: &'a MountStore,
    profile_id: &str,
) -> Option<&'a CredentialSecret> {
    store
        .credential_secrets
        .iter()
        .find(|secret| secret.profile_id == profile_id)
}

pub fn upsert_mount(store: &mut MountStore, record: MountRecord) {
    let key = record.key();
    if let Some(existing) = store.mounts.iter_mut().find(|mount| mount.key() == key) {
        *existing = record;
    } else {
        store.mounts.push(record);
    }
}

pub fn upsert_policy(store: &mut MountStore, policy: MountPolicy) {
    if let Some(existing) = find_policy_mut(store, &policy.policy_id) {
        *existing = policy;
    } else {
        store.policies.push(policy);
    }
}

pub fn upsert_credential_profile(store: &mut MountStore, profile: CredentialProfile) {
    if let Some(existing) = store
        .credential_profiles
        .iter_mut()
        .find(|existing| existing.profile_id == profile.profile_id)
    {
        *existing = profile;
    } else {
        store.credential_profiles.push(profile);
    }
}

pub fn upsert_credential_secret(store: &mut MountStore, secret: CredentialSecret) {
    if let Some(existing) = store
        .credential_secrets
        .iter_mut()
        .find(|existing| existing.profile_id == secret.profile_id)
    {
        *existing = secret;
    } else {
        store.credential_secrets.push(secret);
    }
}

pub fn append_audit_event(store: &mut MountStore, mut event: MountAuditRecord) {
    event.timestamp = Utc::now();
    store.audit_events.push(event);
    const MAX_AUDIT_EVENTS: usize = 5000;
    if store.audit_events.len() > MAX_AUDIT_EVENTS {
        let overflow = store.audit_events.len() - MAX_AUDIT_EVENTS;
        store.audit_events.drain(0..overflow);
    }
}

pub fn existing_mount_keys(store: &MountStore) -> Vec<MountKey> {
    store.mounts.iter().map(MountRecord::key).collect()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        fs,
        path::PathBuf,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    use chrono::Utc;

    use crate::mounts::models::{ConnectorKind, MountStatus};

    use super::*;

    #[test]
    fn mount_records_are_keyed_by_project_and_mount_id() {
        let mut store = MountStore::default();
        let now = Utc::now();
        for project_id in ["project_a", "project_b"] {
            upsert_mount(
                &mut store,
                MountRecord {
                    project_id: project_id.to_string(),
                    mount_id: "mysql-main".to_string(),
                    connector_kind: ConnectorKind::Mysql,
                    display_name: format!("{project_id} MySQL"),
                    mount_point: format!("/tmp/{project_id}/.traceway/mounts/mysql-main"),
                    credential_profile_id: format!("cred_{project_id}"),
                    policy_id: format!("policy_{project_id}"),
                    status: MountStatus::Stopped,
                    last_health_check_at: None,
                    last_error: None,
                    created_at: now,
                    updated_at: now,
                },
            );
        }

        assert_eq!(existing_mount_keys(&store).len(), 2);
        assert_eq!(
            find_mount(&store, "project_a", "mysql-main")
                .expect("project A mount")
                .credential_profile_id,
            "cred_project_a"
        );
        assert_eq!(
            find_mount(&store, "project_b", "mysql-main")
                .expect("project B mount")
                .credential_profile_id,
            "cred_project_b"
        );
    }

    #[test]
    fn mount_point_lookup_finds_existing_registered_path() {
        let mut store = MountStore::default();
        let now = Utc::now();
        upsert_mount(
            &mut store,
            MountRecord {
                project_id: "project_a".to_string(),
                mount_id: "mysql-main".to_string(),
                connector_kind: ConnectorKind::Mysql,
                display_name: "Project A MySQL".to_string(),
                mount_point: "/tmp/project/.traceway/mounts/mysql-main".to_string(),
                credential_profile_id: "cred_project_a".to_string(),
                policy_id: "policy_project_a".to_string(),
                status: MountStatus::Stopped,
                last_health_check_at: None,
                last_error: None,
                created_at: now,
                updated_at: now,
            },
        );

        let existing =
            find_mount_by_mount_point(&store, "/tmp/project/.traceway/mounts/mysql-main")
                .expect("existing mountpoint");

        assert_eq!(existing.project_id, "project_a");
        assert_eq!(existing.mount_id, "mysql-main");
    }

    #[test]
    fn atomic_save_creates_parent_directory_and_loads_store() {
        let root = unique_temp_dir("atomic-save");
        let path = root.join("nested").join("mounts.json");
        let mut store = MountStore::default();
        let now = Utc::now();
        upsert_mount(
            &mut store,
            MountRecord {
                project_id: "project_a".to_string(),
                mount_id: "mysql-main".to_string(),
                connector_kind: ConnectorKind::Mysql,
                display_name: "Project A MySQL".to_string(),
                mount_point: "/tmp/project/.traceway/mounts/mysql-main".to_string(),
                credential_profile_id: "cred_project_a".to_string(),
                policy_id: "policy_project_a".to_string(),
                status: MountStatus::Stopped,
                last_health_check_at: None,
                last_error: None,
                created_at: now,
                updated_at: now,
            },
        );

        save_mount_store(&path, &store).expect("save store");
        let loaded = load_mount_store(&path).expect("load store");

        assert_eq!(loaded.mounts.len(), 1);
        assert_eq!(loaded.mounts[0].mount_id, "mysql-main");
        assert!(path.exists());
        assert!(
            fs::read_dir(path.parent().expect("parent"))
                .expect("read parent")
                .all(|entry| {
                    !entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .contains(".tmp-")
                })
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn with_mount_store_serializes_concurrent_audit_appends() {
        let root = unique_temp_dir("concurrent-audit");
        let path = root.join("mounts.json");
        let thread_count = 8;
        let events_per_thread = 10;
        let handles = (0..thread_count)
            .map(|thread_index| {
                let path = path.clone();
                thread::spawn(move || {
                    for event_index in 0..events_per_thread {
                        with_mount_store(&path, |store| {
                            append_audit_event(
                                store,
                                MountAuditRecord {
                                    timestamp: Utc::now(),
                                    project_id: "project_a".to_string(),
                                    mount_id: "mysql-main".to_string(),
                                    virtual_path: format!(
                                        "schemas/app/tables/users/{thread_index}-{event_index}.json"
                                    ),
                                    result: "ok".to_string(),
                                    bytes_returned: event_index,
                                    duration_ms: 1,
                                    error: None,
                                },
                            );
                            Ok(())
                        })
                        .expect("append audit event");
                    }
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().expect("join audit thread");
        }

        let loaded = load_mount_store(&path).expect("load store");
        let paths = loaded
            .audit_events
            .iter()
            .map(|event| event.virtual_path.clone())
            .collect::<HashSet<_>>();

        assert_eq!(loaded.audit_events.len(), thread_count * events_per_thread);
        assert_eq!(paths.len(), thread_count * events_per_thread);

        fs::remove_dir_all(root).ok();
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "codex-session-manager-{label}-{}-{nanos}",
            std::process::id()
        ))
    }
}
