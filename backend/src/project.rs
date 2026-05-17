use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::models::ProjectIdentity;

pub fn project_identity_for_path(project_path: Option<&str>) -> ProjectIdentity {
    let normalized = project_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string);
    let git_identity = normalized
        .as_deref()
        .and_then(|path| git_project_identity(Path::new(path)));
    let root_path = git_identity
        .as_ref()
        .map(|identity| identity.root_path.clone())
        .or_else(|| normalized.clone());
    let path_label = root_path
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "unknown-project".to_string());
    let git_remote_hash = git_identity
        .as_ref()
        .map(|identity| identity.remote_hash.clone());
    let git_branch = git_identity
        .as_ref()
        .and_then(|identity| identity.branch.clone());
    let project_id = git_remote_hash
        .as_deref()
        .map(|hash| format!("project_git_{hash}"))
        .or_else(|| {
            normalized
                .as_deref()
                .map(|path| format!("project_{}", short_hash(path)))
        })
        .unwrap_or_else(|| "project_unknown".to_string());

    ProjectIdentity {
        project_id,
        root_path,
        path_label,
        git_remote_hash,
        git_branch,
    }
}

struct GitProjectIdentity {
    root_path: String,
    remote_hash: String,
    branch: Option<String>,
}

fn git_project_identity(path: &Path) -> Option<GitProjectIdentity> {
    let root = find_git_root(path)?;
    let git_dir = git_dir_for_root(&root)?;
    let common_dir = git_common_dir(&git_dir);
    let remote = read_origin_remote(&common_dir)?;
    let normalized_remote = normalize_git_remote_url(&remote);
    let root_path = root
        .canonicalize()
        .unwrap_or(root)
        .to_string_lossy()
        .to_string();

    Some(GitProjectIdentity {
        root_path,
        remote_hash: short_hash(&normalized_remote),
        branch: read_git_branch(&git_dir),
    })
}

fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_file() {
        path.parent()?.to_path_buf()
    } else {
        path.to_path_buf()
    };

    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn git_dir_for_root(root: &Path) -> Option<PathBuf> {
    let git_path = root.join(".git");
    if git_path.is_dir() {
        return Some(git_path);
    }

    let content = std::fs::read_to_string(&git_path).ok()?;
    let git_dir = content.trim().strip_prefix("gitdir:")?.trim();
    let path = PathBuf::from(git_dir);
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn git_common_dir(git_dir: &Path) -> PathBuf {
    let common_dir_path = git_dir.join("commondir");
    let Ok(common_dir) = std::fs::read_to_string(common_dir_path) else {
        return git_dir.to_path_buf();
    };
    let path = PathBuf::from(common_dir.trim());
    if path.is_absolute() {
        path
    } else {
        git_dir.join(path)
    }
}

fn read_origin_remote(git_dir: &Path) -> Option<String> {
    let config = std::fs::read_to_string(git_dir.join("config")).ok()?;
    let mut in_origin = false;

    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_origin = line == r#"[remote "origin"]"#;
            continue;
        }
        if in_origin {
            if let Some(url) = line.strip_prefix("url") {
                return url
                    .trim_start()
                    .strip_prefix('=')
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
                    .map(str::to_string);
            }
        }
    }

    None
}

fn read_git_branch(git_dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: refs/heads/") {
        return Some(reference.to_string());
    }
    if head.len() >= 12 && head.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Some(head.chars().take(12).collect());
    }
    None
}

fn normalize_git_remote_url(remote: &str) -> String {
    let remote = remote.trim().trim_end_matches('/').trim_end_matches(".git");

    if let Some(rest) = remote.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!(
                "{}/{}",
                host.to_ascii_lowercase(),
                path.trim_start_matches('/')
            )
            .to_ascii_lowercase();
        }
    }

    if let Some((_, rest)) = remote.split_once("://") {
        let rest = rest
            .split_once('@')
            .map_or(rest, |(_, without_user)| without_user);
        if let Some((host, path)) = rest.split_once('/') {
            return format!(
                "{}/{}",
                host.to_ascii_lowercase(),
                path.trim_start_matches('/')
            )
            .to_ascii_lowercase();
        }
    }

    remote.to_ascii_lowercase()
}

fn short_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    format!("{digest:x}").chars().take(16).collect()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn project_identity_uses_git_remote_hash_across_checkout_paths() {
        let temp_dir = unique_temp_dir("git-identity");
        let alice_repo = temp_dir.join("alice").join("repo");
        let bob_repo = temp_dir.join("bob").join("repo");
        let alice_subdir = alice_repo.join("backend").join("src");
        let bob_subdir = bob_repo.join("frontend").join("src");
        fs::create_dir_all(alice_repo.join(".git")).expect("create alice git dir");
        fs::create_dir_all(bob_repo.join(".git")).expect("create bob git dir");
        fs::create_dir_all(&alice_subdir).expect("create alice subdir");
        fs::create_dir_all(&bob_subdir).expect("create bob subdir");
        fs::write(
            alice_repo.join(".git").join("config"),
            r#"[remote "origin"]
    url = git@github.com:Example/Repo.git
"#,
        )
        .expect("write alice config");
        fs::write(
            bob_repo.join(".git").join("config"),
            r#"[remote "origin"]
    url = https://github.com/example/repo.git
"#,
        )
        .expect("write bob config");
        fs::write(
            alice_repo.join(".git").join("HEAD"),
            "ref: refs/heads/main\n",
        )
        .expect("write alice head");
        fs::write(
            bob_repo.join(".git").join("HEAD"),
            "ref: refs/heads/feature/collab\n",
        )
        .expect("write bob head");

        let alice_identity =
            project_identity_for_path(Some(alice_subdir.to_string_lossy().as_ref()));
        let bob_identity = project_identity_for_path(Some(bob_subdir.to_string_lossy().as_ref()));

        assert_eq!(alice_identity.project_id, bob_identity.project_id);
        assert!(alice_identity.project_id.starts_with("project_git_"));
        assert_eq!(alice_identity.git_remote_hash, bob_identity.git_remote_hash);
        assert_eq!(alice_identity.git_branch.as_deref(), Some("main"));
        assert_eq!(bob_identity.git_branch.as_deref(), Some("feature/collab"));
        let alice_repo = alice_repo.canonicalize().expect("canonical alice repo");
        assert_eq!(
            alice_identity.root_path.as_deref(),
            Some(alice_repo.to_string_lossy().as_ref())
        );
        assert_eq!(alice_identity.path_label, "repo");

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn project_identity_falls_back_to_path_without_git_remote() {
        let temp_dir = unique_temp_dir("path-identity");
        let project_dir = temp_dir.join("plain-project");
        fs::create_dir_all(&project_dir).expect("create project dir");

        let identity = project_identity_for_path(Some(project_dir.to_string_lossy().as_ref()));

        assert!(identity.project_id.starts_with("project_"));
        assert!(!identity.project_id.starts_with("project_git_"));
        assert_eq!(
            identity.root_path.as_deref(),
            Some(project_dir.to_string_lossy().as_ref())
        );
        assert_eq!(identity.path_label, "plain-project");
        assert_eq!(identity.git_remote_hash, None);
        assert_eq!(identity.git_branch, None);

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn project_identity_handles_unknown_path() {
        let identity = project_identity_for_path(None);

        assert_eq!(identity.project_id, "project_unknown");
        assert_eq!(identity.root_path, None);
        assert_eq!(identity.path_label, "unknown-project");
        assert_eq!(identity.git_remote_hash, None);
        assert_eq!(identity.git_branch, None);
    }

    #[test]
    fn project_identity_reads_worktree_common_remote_and_head() {
        let temp_dir = unique_temp_dir("worktree-identity");
        let repo = temp_dir.join("repo");
        let worktree = temp_dir.join("repo-worktree");
        let common_git = repo.join(".git");
        let worktree_git = common_git.join("worktrees").join("repo-worktree");
        fs::create_dir_all(&worktree).expect("create worktree");
        fs::create_dir_all(&worktree_git).expect("create worktree git dir");
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", worktree_git.to_string_lossy()),
        )
        .expect("write git file");
        fs::write(worktree_git.join("commondir"), "../..\n").expect("write commondir");
        fs::write(
            common_git.join("config"),
            r#"[remote "origin"]
    url = https://user@github.com/Example/Repo.git/
"#,
        )
        .expect("write common config");
        fs::write(worktree_git.join("HEAD"), "0123456789abcdef\n").expect("write head");

        let identity = project_identity_for_path(Some(worktree.to_string_lossy().as_ref()));

        assert!(identity.project_id.starts_with("project_git_"));
        assert!(identity.git_remote_hash.is_some());
        assert_eq!(identity.git_branch.as_deref(), Some("0123456789ab"));
        assert_eq!(identity.path_label, "repo-worktree");

        fs::remove_dir_all(temp_dir).ok();
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "csm-project-{prefix}-{}-{stamp}",
            std::process::id()
        ))
    }
}
