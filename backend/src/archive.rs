use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::{
    config::Config,
    error::AppError,
    models::{ArchiveRecord, Session},
};

pub fn archive_session(config: &Config, session: &Session) -> Result<ArchiveRecord, AppError> {
    let source_path = Path::new(&session.path);
    if !source_path.is_file() {
        return Err(AppError::BadRequest(format!(
            "session file '{}' does not exist",
            session.path
        )));
    }

    fs::create_dir_all(&config.archive_dir)?;

    let source_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session.txt");
    let archive_name = format!("{}-{}", session.id, safe_file_component(source_name));
    let archive_path = config.archive_dir.join(archive_name);
    let checksum = copy_with_sha256(source_path, &archive_path)?;

    Ok(ArchiveRecord {
        session_id: session.id.clone(),
        source_path: session.path.clone(),
        archive_provider: "local".to_string(),
        archive_uri: format!("file://{}", archive_path.to_string_lossy()),
        archived_at: Utc::now(),
        checksum: Some(checksum),
    })
}

fn copy_with_sha256(source_path: &Path, archive_path: &Path) -> Result<String, AppError> {
    let mut source = File::open(source_path)?;
    let mut archive = File::create(archive_path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }

        hasher.update(&buffer[..read]);
        archive.write_all(&buffer[..read])?;
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn safe_file_component(name: &str) -> String {
    let safe = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if safe.is_empty() {
        "session.txt".to_string()
    } else {
        safe
    }
}
