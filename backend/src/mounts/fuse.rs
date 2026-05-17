use std::{
    collections::HashMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime},
};

use fuser::{
    BackgroundSession, FUSE_ROOT_ID, FileAttr, FileType, Filesystem, MountOption, ReplyAttr,
    ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, Request,
    TimeOrNow,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::runtime::Runtime;

use crate::{
    error::AppError,
    mounts::{
        models::MountRecord,
        mysql::LiveMysqlConnector,
        router::{self, MountCache, MountContext},
    },
};

const TTL: Duration = Duration::from_secs(1);
const ROOT_PATH: &str = "";

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

pub fn ensure_mount_point(path: &Path) -> Result<(), AppError> {
    fs::create_dir_all(path)?;
    Ok(())
}

pub fn gitignore_hint(project_root: &Path) -> Option<String> {
    let expected = ".traceway/mounts/";
    let gitignore = project_root.join(".gitignore");
    let content = fs::read_to_string(gitignore).unwrap_or_default();
    if content
        .lines()
        .map(str::trim)
        .any(|line| line == expected || line == ".traceway/" || line == ".traceway/mounts")
    {
        None
    } else {
        Some(format!(
            "Add `{expected}` to .gitignore before committing project files."
        ))
    }
}

pub fn start_readonly_mount(
    context: MountContext,
    dsn: String,
    store_path: PathBuf,
    cache: Arc<MountCache>,
) -> Result<FuseStartReport, AppError> {
    let mount_point = PathBuf::from(&context.mount.mount_point);
    ensure_mount_point(&mount_point)?;
    let project_root = mount_point
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent);
    let gitignore_hint = project_root.and_then(gitignore_hint);

    let key = context.mount.key();
    if active_mounts()
        .lock()
        .expect("active mount registry lock poisoned")
        .contains_key(&key)
    {
        return Ok(FuseStartReport {
            mount_point: context.mount.mount_point,
            fuse_available: true,
            message: "FUSE mount is already running.".to_string(),
            gitignore_hint,
        });
    }

    let fs = MysqlContextFuse::new(context.clone(), dsn, store_path, cache)?;
    let options = vec![
        MountOption::RO,
        MountOption::NoDev,
        MountOption::NoExec,
        MountOption::NoSuid,
        MountOption::AutoUnmount,
        MountOption::FSName(format!(
            "traceway-{}-{}",
            context.mount.project_id, context.mount.mount_id
        )),
    ];
    let hint_text = gitignore_hint
        .as_deref()
        .map(|hint| format!(" {hint}"))
        .unwrap_or_default();
    let session = fuser::spawn_mount2(fs, &mount_point, &options).map_err(|error| {
        AppError::BadRequest(format!(
            "FUSE mounting failed at '{}'. Install/authorize macFUSE or verify mount permissions. {}{}",
            context.mount.mount_point, error, hint_text
        ))
    })?;

    active_mounts()
        .lock()
        .expect("active mount registry lock poisoned")
        .insert(key, session);

    Ok(FuseStartReport {
        mount_point: context.mount.mount_point,
        fuse_available: true,
        message: "FUSE read-only mount started.".to_string(),
        gitignore_hint,
    })
}

pub fn stop_readonly_mount(record: &MountRecord) -> FuseStartReport {
    let stopped = active_mounts()
        .lock()
        .expect("active mount registry lock poisoned")
        .remove(&record.key())
        .is_some();

    FuseStartReport {
        mount_point: record.mount_point.clone(),
        fuse_available: true,
        message: if stopped {
            "FUSE mount stopped.".to_string()
        } else {
            "No FUSE mount was running for this mount.".to_string()
        },
        gitignore_hint: None,
    }
}

pub fn readonly_write_error() -> AppError {
    AppError::BadRequest("Project context mounts are read-only".to_string())
}

fn active_mounts() -> &'static Mutex<HashMap<String, BackgroundSession>> {
    static ACTIVE_MOUNTS: OnceLock<Mutex<HashMap<String, BackgroundSession>>> = OnceLock::new();
    ACTIVE_MOUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct MysqlContextFuse {
    context: MountContext,
    connector: LiveMysqlConnector,
    cache: Arc<MountCache>,
    store_path: PathBuf,
    runtime: Runtime,
    nodes: NodeMap,
}

impl MysqlContextFuse {
    fn new(
        context: MountContext,
        dsn: String,
        store_path: PathBuf,
        cache: Arc<MountCache>,
    ) -> Result<Self, AppError> {
        Ok(Self {
            context,
            connector: LiveMysqlConnector::new(dsn),
            cache,
            store_path,
            runtime: Runtime::new().map_err(|error| AppError::External(error.to_string()))?,
            nodes: NodeMap::default(),
        })
    }

    fn path_for_inode(&self, ino: u64) -> Option<String> {
        self.nodes.path_for_inode(ino)
    }

    fn inode_for_path(&mut self, path: &str) -> u64 {
        self.nodes.inode_for_path(
            &self.context.mount.project_id,
            &self.context.mount.mount_id,
            path,
        )
    }

    fn child_path(&self, parent: u64, name: &OsStr) -> Option<String> {
        let name = name.to_str()?;
        if name.is_empty() || name == "." || name == ".." {
            return None;
        }
        let parent_path = self.path_for_inode(parent)?;
        if parent_path.is_empty() {
            Some(name.to_string())
        } else {
            Some(format!("{parent_path}/{name}"))
        }
    }

    fn classify(&mut self, path: &str) -> Result<NodeKind, AppError> {
        if path == ROOT_PATH {
            return Ok(NodeKind::Directory);
        }
        if known_file_path(path) || router::parse_lookup_path(path).is_some() {
            return Ok(NodeKind::File {
                size: self.file_size(path).unwrap_or(0),
            });
        }

        match self.runtime.block_on(router::readdir_virtual(
            &self.context,
            &self.connector,
            path,
        )) {
            Ok(_) => Ok(NodeKind::Directory),
            Err(AppError::NotFound(_)) => self.file_size(path).map(|size| NodeKind::File { size }),
            Err(error) => Err(error),
        }
    }

    fn file_size(&self, path: &str) -> Result<u64, AppError> {
        self.runtime
            .block_on(router::read_virtual_file(
                &self.context,
                &self.connector,
                &self.cache,
                path,
            ))
            .map(|bytes| bytes.len() as u64)
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, AppError> {
        self.runtime.block_on(router::read_virtual_file_audited(
            &self.context,
            &self.connector,
            &self.cache,
            &self.store_path,
            path,
        ))
    }

    fn attr_for_path(&mut self, path: &str, kind: NodeKind) -> FileAttr {
        let ino = self.inode_for_path(path);
        let now = SystemTime::now();
        let (file_type, perm, nlink, size) = match kind {
            NodeKind::Directory => (FileType::Directory, 0o555, 2, 0),
            NodeKind::File { size } => (FileType::RegularFile, 0o444, 1, size),
        };

        FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: file_type,
            perm,
            nlink,
            uid: unsafe { libc::geteuid() },
            gid: unsafe { libc::getegid() },
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }

    fn errno_for(error: AppError) -> i32 {
        match error {
            AppError::NotFound(_) => libc::ENOENT,
            AppError::BadRequest(_) => libc::EACCES,
            _ => libc::EIO,
        }
    }
}

impl Filesystem for MysqlContextFuse {
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let Some(path) = self.child_path(parent, name) else {
            reply.error(libc::ENOENT);
            return;
        };
        match self.classify(&path) {
            Ok(kind) => {
                let attr = self.attr_for_path(&path, kind);
                reply.entry(&TTL, &attr, 0);
            }
            Err(error) => reply.error(Self::errno_for(error)),
        }
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let Some(path) = self.path_for_inode(ino) else {
            reply.error(libc::ENOENT);
            return;
        };
        match self.classify(&path) {
            Ok(kind) => {
                let attr = self.attr_for_path(&path, kind);
                reply.attr(&TTL, &attr);
            }
            Err(error) => reply.error(Self::errno_for(error)),
        }
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        if flags & libc::O_ACCMODE != libc::O_RDONLY {
            reply.error(libc::EROFS);
            return;
        }
        let Some(path) = self.path_for_inode(ino) else {
            reply.error(libc::ENOENT);
            return;
        };
        match self.classify(&path) {
            Ok(NodeKind::File { .. }) => reply.opened(0, 0),
            Ok(NodeKind::Directory) => reply.error(libc::EISDIR),
            Err(error) => reply.error(Self::errno_for(error)),
        }
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }
        let Some(path) = self.path_for_inode(ino) else {
            reply.error(libc::ENOENT);
            return;
        };
        match self.read_file(&path) {
            Ok(bytes) => {
                let offset = offset as usize;
                if offset >= bytes.len() {
                    reply.data(&[]);
                    return;
                }
                let end = bytes.len().min(offset + size as usize);
                reply.data(&bytes[offset..end]);
            }
            Err(error) => reply.error(Self::errno_for(error)),
        }
    }

    fn opendir(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        if flags & libc::O_ACCMODE != libc::O_RDONLY {
            reply.error(libc::EROFS);
            return;
        }
        let Some(path) = self.path_for_inode(ino) else {
            reply.error(libc::ENOENT);
            return;
        };
        match self.classify(&path) {
            Ok(NodeKind::Directory) => reply.opened(0, 0),
            Ok(NodeKind::File { .. }) => reply.error(libc::ENOTDIR),
            Err(error) => reply.error(Self::errno_for(error)),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let Some(path) = self.path_for_inode(ino) else {
            reply.error(libc::ENOENT);
            return;
        };
        let entries = match self.runtime.block_on(router::readdir_virtual(
            &self.context,
            &self.connector,
            &path,
        )) {
            Ok(entries) => entries,
            Err(error) => {
                reply.error(Self::errno_for(error));
                return;
            }
        };

        let mut all_entries = vec![
            (FUSE_ROOT_ID, FileType::Directory, ".".to_string()),
            (
                parent_inode(&self.nodes, &path),
                FileType::Directory,
                "..".to_string(),
            ),
        ];
        for entry in entries {
            let child_path = join_virtual_path(&path, &entry);
            let kind = if known_file_path(&child_path) {
                FileType::RegularFile
            } else {
                FileType::Directory
            };
            let ino = self.inode_for_path(&child_path);
            all_entries.push((ino, kind, entry));
        }

        for (index, (ino, kind, name)) in all_entries.into_iter().enumerate().skip(offset as usize)
        {
            if reply.add(ino, (index + 1) as i64, kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn setattr(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        reply.error(libc::EROFS);
    }

    fn mknod(
        &mut self,
        _req: &Request<'_>,
        _parent: u64,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        reply.error(libc::EROFS);
    }

    fn mkdir(
        &mut self,
        _req: &Request<'_>,
        _parent: u64,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        reply.error(libc::EROFS);
    }

    fn unlink(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(libc::EROFS);
    }

    fn rmdir(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(libc::EROFS);
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        _parent: u64,
        _name: &OsStr,
        _newparent: u64,
        _newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        reply.error(libc::EROFS);
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _offset: i64,
        _data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        reply.error(libc::EROFS);
    }

    fn create(
        &mut self,
        _req: &Request<'_>,
        _parent: u64,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        reply.error(libc::EROFS);
    }
}

#[derive(Debug, Clone, Copy)]
enum NodeKind {
    Directory,
    File { size: u64 },
}

struct NodeMap {
    path_by_inode: HashMap<u64, String>,
    inode_by_path: HashMap<String, u64>,
}

impl Default for NodeMap {
    fn default() -> Self {
        let mut path_by_inode = HashMap::new();
        let mut inode_by_path = HashMap::new();
        path_by_inode.insert(FUSE_ROOT_ID, ROOT_PATH.to_string());
        inode_by_path.insert(ROOT_PATH.to_string(), FUSE_ROOT_ID);
        Self {
            path_by_inode,
            inode_by_path,
        }
    }
}

impl NodeMap {
    fn path_for_inode(&self, ino: u64) -> Option<String> {
        self.path_by_inode.get(&ino).cloned()
    }

    fn inode_for_path(&mut self, project_id: &str, mount_id: &str, path: &str) -> u64 {
        if let Some(ino) = self.inode_by_path.get(path) {
            return *ino;
        }

        let mut ino = stable_inode(project_id, mount_id, path);
        while ino == FUSE_ROOT_ID
            || self
                .path_by_inode
                .get(&ino)
                .is_some_and(|existing| existing != path)
        {
            ino = ino.wrapping_add(1).max(2);
        }
        self.path_by_inode.insert(ino, path.to_string());
        self.inode_by_path.insert(path.to_string(), ino);
        ino
    }
}

fn stable_inode(project_id: &str, mount_id: &str, path: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(mount_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(path.as_bytes());
    let digest = hasher.finalize();
    u64::from_be_bytes(digest[..8].try_into().expect("sha256 digest has 32 bytes")).max(2)
}

fn parent_inode(nodes: &NodeMap, path: &str) -> u64 {
    if path.is_empty() {
        return FUSE_ROOT_ID;
    }
    path.rsplit_once('/')
        .and_then(|(parent, _)| nodes.inode_by_path.get(parent).copied())
        .unwrap_or(FUSE_ROOT_ID)
}

fn join_virtual_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

fn known_file_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    matches!(
        parts.as_slice(),
        ["README.md"]
            | ["connection.json"]
            | ["health.json"]
            | ["queries", "README.md"]
            | ["schemas", _, "schema.json"]
            | ["schemas", _, "tables", _, "schema.sql"]
            | ["schemas", _, "tables", _, "columns.json"]
            | ["schemas", _, "tables", _, "indexes.sql"]
            | ["schemas", _, "tables", _, "foreign_keys.json"]
            | ["schemas", _, "tables", _, "inferred_relations.json"]
            | ["schemas", _, "tables", _, "lookup_manifest.json"]
            | ["schemas", _, "tables", _, "count.txt"]
            | ["schemas", _, "tables", _, "sample.jsonl"]
            | ["schemas", _, "tables", _, "lookup", "README.md"]
            | ["schemas", _, "tables", _, "stats", "status_counts.json"]
            | ["schemas", _, "tables", _, "stats", "null_counts.json"]
            | [
                "schemas",
                _,
                "tables",
                _,
                "stats",
                "top_values",
                "README.md"
            ]
    ) || router::parse_lookup_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn gitignore_hint_is_absent_when_traceway_mounts_are_ignored() {
        let root = unique_temp_dir("gitignore");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(root.join(".gitignore"), ".traceway/mounts/\n").expect("write gitignore");

        assert!(gitignore_hint(&root).is_none());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn project_mount_point_uses_traceway_mounts_directory() {
        let root = PathBuf::from("/tmp/project");

        assert_eq!(
            project_mount_point(&root, "mysql-main"),
            PathBuf::from("/tmp/project/.traceway/mounts/mysql-main")
        );
    }

    #[test]
    fn known_file_path_includes_addressable_lookup_values() {
        assert!(known_file_path(
            "schemas/app/tables/users/lookup/by-primary/id/123.json"
        ));
        assert!(!known_file_path(
            "schemas/app/tables/users/lookup/by-primary/id"
        ));
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("csm-mount-{prefix}-{}-{stamp}", std::process::id()))
    }
}
