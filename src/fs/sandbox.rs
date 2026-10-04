use std::path::{Component, Path, PathBuf};
use path_clean::PathClean;
use serde::{Deserialize, Serialize};

use crate::error::TentacleError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub modified: Option<u64>,
    pub permissions: u32,
}

#[derive(Debug, Clone)]
pub struct SandboxedFs {
    root: PathBuf,
}

impl SandboxedFs {
    pub fn new<P: Into<PathBuf>>(root: P) -> Result<Self, TentacleError> {
        let raw_root = root.into();
        std::fs::create_dir_all(&raw_root)?;
        let canonical_root = dunce::canonicalize(&raw_root).map_err(|e| {
            TentacleError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to canonicalize sandbox root {}: {}",
                    raw_root.display(),
                    e
                ),
            ))
        })?;

        Ok(Self {
            root: canonical_root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve_safe_path<P: AsRef<Path>>(&self, user_path: P) -> Result<PathBuf, TentacleError> {
        let p = user_path.as_ref();

        let normalized = if let Ok(stripped) = p.strip_prefix("/home/container") {
            stripped
        } else if let Ok(stripped) = p.strip_prefix("home/container") {
            stripped
        } else {
            p
        };

        let mut cleaned_relative = PathBuf::new();
        for component in normalized.components() {
            match component {
                Component::Normal(c) => cleaned_relative.push(c),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !cleaned_relative.pop() {
                        return Err(TentacleError::PathTraversal(format!(
                            "Path traversal attempt detected in: {}",
                            p.display()
                        )));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    // Strip leading root/prefix and treat as relative to sandbox root
                }
            }
        }

        let cleaned = cleaned_relative.clean();
        let target = self.root.join(cleaned);

        if target.exists() {
            let canonical = dunce::canonicalize(&target).map_err(|e| {
                TentacleError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to canonicalize {}: {}", target.display(), e),
                ))
            })?;

            if !canonical.starts_with(&self.root) {
                return Err(TentacleError::PathTraversal(format!(
                    "Resolved path {} escapes sandbox root {}",
                    canonical.display(),
                    self.root.display()
                )));
            }

            Ok(canonical)
        } else {
            let parent = target.parent().unwrap_or(&self.root);
            if parent.exists() {
                let canonical_parent = dunce::canonicalize(parent).map_err(|e| {
                    TentacleError::Io(std::io::Error::new(
                        e.kind(),
                        format!("Failed to canonicalize parent {}: {}", parent.display(), e),
                    ))
                })?;

                if !canonical_parent.starts_with(&self.root) {
                    return Err(TentacleError::PathTraversal(format!(
                        "Target parent {} escapes sandbox root {}",
                        canonical_parent.display(),
                        self.root.display()
                    )));
                }
            }

            let relative_to_root = target.strip_prefix(&self.root).map_err(|_| {
                TentacleError::PathTraversal(format!(
                    "Target {} outside sandbox root {}",
                    target.display(),
                    self.root.display()
                ))
            })?;

            Ok(self.root.join(relative_to_root))
        }
    }

    pub async fn list_dir<P: AsRef<Path>>(&self, subpath: P) -> Result<Vec<FileEntry>, TentacleError> {
        let safe_dir = self.resolve_safe_path(subpath)?;
        if !safe_dir.is_dir() {
            return Err(TentacleError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Not a directory: {}", safe_dir.display()),
            )));
        }

        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&safe_dir).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;
            let is_symlink = metadata.is_symlink();
            let is_dir = metadata.is_dir();
            let size = if is_dir { 0 } else { metadata.len() };

            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            #[cfg(unix)]
            let permissions = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode()
            };

            #[cfg(not(unix))]
            let permissions = if metadata.permissions().readonly() {
                0o444
            } else {
                0o666
            };

            let relative_path = path
                .strip_prefix(&self.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let name = entry.file_name().to_string_lossy().to_string();

            entries.push(FileEntry {
                name,
                path: relative_path,
                size,
                is_dir,
                is_symlink,
                modified,
                permissions,
            });
        }

        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(entries)
    }

    pub async fn read_file<P: AsRef<Path>>(&self, subpath: P) -> Result<Vec<u8>, TentacleError> {
        let safe_path = self.resolve_safe_path(subpath)?;
        let content = tokio::fs::read(&safe_path).await?;
        Ok(content)
    }

    pub async fn write_file<P: AsRef<Path>>(&self, subpath: P, content: &[u8]) -> Result<(), TentacleError> {
        let safe_path = self.resolve_safe_path(subpath)?;
        if let Some(parent) = safe_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&safe_path, content).await?;
        Ok(())
    }

    pub async fn create_dir<P: AsRef<Path>>(&self, subpath: P) -> Result<(), TentacleError> {
        let safe_path = self.resolve_safe_path(subpath)?;
        tokio::fs::create_dir_all(&safe_path).await?;
        Ok(())
    }

    pub async fn delete_file<P: AsRef<Path>>(&self, subpath: P) -> Result<(), TentacleError> {
        let safe_path = self.resolve_safe_path(subpath)?;
        if safe_path == self.root {
            return Err(TentacleError::PathTraversal(
                "Cannot delete the sandbox root".to_string(),
            ));
        }

        if safe_path.is_dir() {
            tokio::fs::remove_dir_all(&safe_path).await?;
        } else {
            tokio::fs::remove_file(&safe_path).await?;
        }
        Ok(())
    }

    pub async fn rename<P: AsRef<Path>, Q: AsRef<Path>>(&self, from: P, to: Q) -> Result<(), TentacleError> {
        let safe_from = self.resolve_safe_path(from)?;
        let safe_to = self.resolve_safe_path(to)?;

        if safe_from == self.root || safe_to == self.root {
            return Err(TentacleError::PathTraversal(
                "Cannot move to or from sandbox root".to_string(),
            ));
        }

        if let Some(parent) = safe_to.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::rename(&safe_from, &safe_to).await?;
        Ok(())
    }

    pub async fn chmod<P: AsRef<Path>>(&self, subpath: P, mode: u32) -> Result<(), TentacleError> {
        let safe_path = self.resolve_safe_path(subpath)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(mode);
            tokio::fs::set_permissions(&safe_path, permissions).await?;
        }

        #[cfg(not(unix))]
        {
            let _ = (safe_path, mode);
        }

        Ok(())
    }
}
