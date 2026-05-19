#[cfg(feature = "fuse-mounts")]
pub mod fuse;
#[cfg(not(feature = "fuse-mounts"))]
pub mod fuse {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };

    use serde::{Deserialize, Serialize};

    use crate::{
        error::AppError,
        mounts::{
            models::MountRecord,
            router::{MountCache, MountContext},
        },
    };

    #[derive(Debug, Clone, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FuseStartReport {
        pub mount_point: String,
        pub fuse_available: bool,
        pub message: String,
        pub gitignore_hint: Option<String>,
    }

    pub fn project_mount_point(project_root: &Path, mount_id: &str) -> PathBuf {
        project_root.join(".traceway").join("mounts").join(mount_id)
    }

    pub fn start_readonly_mount(
        context: MountContext,
        _dsn: String,
        _store_path: PathBuf,
        _cache: Arc<MountCache>,
    ) -> Result<FuseStartReport, AppError> {
        Err(AppError::BadRequest(format!(
            "FUSE mounts are not enabled in this build. Rebuild with the `fuse-mounts` feature or install a build that includes macFUSE support before starting '{}'.",
            context.mount.mount_point
        )))
    }

    pub fn stop_readonly_mount(record: &MountRecord) -> FuseStartReport {
        FuseStartReport {
            mount_point: record.mount_point.clone(),
            fuse_available: false,
            message: "FUSE mounts are not enabled in this build.".to_string(),
            gitignore_hint: None,
        }
    }
}
pub mod models;
pub mod mysql;
pub mod policy;
pub mod router;
pub mod storage;

pub use models::{MountKey, mount_key};
