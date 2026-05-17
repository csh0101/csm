use std::{fs, path::Path};

use chrono::Utc;

use crate::{
    error::AppError,
    mounts::models::{
        CredentialProfile, CredentialSecret, MountAuditRecord, MountKey, MountPolicy, MountRecord,
        MountStore, mount_key,
    },
};

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

pub fn save_mount_store(path: &Path, store: &MountStore) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = serde_json::to_string_pretty(store)?;
    fs::write(path, raw)?;
    Ok(())
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
}
