use std::{fs, path::Path};

use crate::{
    error::AppError,
    models::{CollaborationStore, MetadataFile},
};

pub fn load_metadata(path: &Path) -> Result<MetadataFile, AppError> {
    if !path.exists() {
        return Ok(MetadataFile::default());
    }

    let raw = fs::read_to_string(path)?;
    let metadata = serde_json::from_str(&raw)?;
    Ok(metadata)
}

pub fn save_metadata(path: &Path, metadata: &MetadataFile) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = serde_json::to_string_pretty(metadata)?;
    fs::write(path, raw)?;
    Ok(())
}

pub fn load_collaboration_store(path: &Path) -> Result<CollaborationStore, AppError> {
    if !path.exists() {
        return Ok(CollaborationStore::default());
    }

    let raw = fs::read_to_string(path)?;
    let store = serde_json::from_str(&raw)?;
    Ok(store)
}

pub fn save_collaboration_store(
    path: &Path,
    collaboration: &CollaborationStore,
) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = serde_json::to_string_pretty(collaboration)?;
    fs::write(path, raw)?;
    Ok(())
}
