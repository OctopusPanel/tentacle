use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode,
};
use russh_sftp::server::Handler;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::fs::{FileEntry, SandboxedFs};

pub struct SftpSessionHandler {
    pub fs: SandboxedFs,
    next_handle: AtomicU64,
    open_files: HashMap<String, TokioFile>,
    open_dirs: HashMap<String, Vec<FileEntry>>,
}

impl SftpSessionHandler {
    pub fn new(fs: SandboxedFs) -> Self {
        Self {
            fs,
            next_handle: AtomicU64::new(1),
            open_files: HashMap::new(),
            open_dirs: HashMap::new(),
        }
    }

    fn generate_handle(&self, prefix: &str) -> String {
        let id = self.next_handle.fetch_add(1, Ordering::SeqCst);
        format!("{}_{}", prefix, id)
    }

    fn ok_status(id: u32) -> Status {
        Status {
            id,
            status_code: StatusCode::Ok,
            error_message: String::new(),
            language_tag: "en-US".to_string(),
        }
    }
}

impl Handler for SftpSessionHandler {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> impl Future<Output = Result<Handle, Self::Error>> + Send {
        async move {
            let safe_path = self.fs.resolve_safe_path(&filename).map_err(|_| StatusCode::PermissionDenied)?;

            let mut open_options = tokio::fs::OpenOptions::new();
            if pflags.contains(OpenFlags::READ) {
                open_options.read(true);
            }
            if pflags.contains(OpenFlags::WRITE) {
                open_options.write(true);
            }
            if pflags.contains(OpenFlags::APPEND) {
                open_options.append(true);
            }
            if pflags.contains(OpenFlags::CREATE) {
                open_options.create(true);
            }
            if pflags.contains(OpenFlags::TRUNCATE) {
                open_options.truncate(true);
            }

            if let Some(parent) = safe_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            let file = open_options.open(&safe_path).await.map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
                std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
                _ => StatusCode::Failure,
            })?;

            let handle = self.generate_handle("file");
            self.open_files.insert(handle.clone(), file);
            Ok(Handle { id, handle })
        }
    }

    fn close(
        &mut self,
        id: u32,
        handle: String,
    ) -> impl Future<Output = Result<Status, Self::Error>> + Send {
        async move {
            self.open_files.remove(&handle);
            self.open_dirs.remove(&handle);
            Ok(Self::ok_status(id))
        }
    }

    fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> impl Future<Output = Result<Data, Self::Error>> + Send {
        async move {
            let file = self.open_files.get_mut(&handle).ok_or(StatusCode::NoSuchFile)?;

            file.seek(SeekFrom::Start(offset))
                .await
                .map_err(|_| StatusCode::Failure)?;

            let mut buf = vec![0u8; len as usize];
            let bytes_read = file.read(&mut buf).await.map_err(|_| StatusCode::Failure)?;

            if bytes_read == 0 {
                return Err(StatusCode::Eof);
            }

            buf.truncate(bytes_read);
            Ok(Data { id, data: buf })
        }
    }

    fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> impl Future<Output = Result<Status, Self::Error>> + Send {
        async move {
            let file = self.open_files.get_mut(&handle).ok_or(StatusCode::NoSuchFile)?;

            file.seek(SeekFrom::Start(offset))
                .await
                .map_err(|_| StatusCode::Failure)?;

            file.write_all(&data).await.map_err(|_| StatusCode::Failure)?;
            Ok(Self::ok_status(id))
        }
    }

    fn opendir(
        &mut self,
        id: u32,
        path: String,
    ) -> impl Future<Output = Result<Handle, Self::Error>> + Send {
        async move {
            let safe_subpath = if path == "/" || path.is_empty() {
                PathBuf::from(".")
            } else {
                PathBuf::from(path.trim_start_matches('/'))
            };

            let entries = self
                .fs
                .list_dir(&safe_subpath)
                .await
                .map_err(|_| StatusCode::NoSuchFile)?;

            let handle = self.generate_handle("dir");
            self.open_dirs.insert(handle.clone(), entries);
            Ok(Handle { id, handle })
        }
    }

    fn readdir(
        &mut self,
        id: u32,
        handle: String,
    ) -> impl Future<Output = Result<Name, Self::Error>> + Send {
        async move {
            let entries = self.open_dirs.get_mut(&handle).ok_or(StatusCode::NoSuchFile)?;
            if entries.is_empty() {
                return Err(StatusCode::Eof);
            }

            let chunk: Vec<FileEntry> = entries.drain(..entries.len().min(64)).collect();
            let files = chunk
                .into_iter()
                .map(|e| {
                    let attrs = FileAttributes {
                        size: Some(e.size),
                        permissions: Some(e.permissions),
                        mtime: e.modified.map(|t| t as u32),
                        ..Default::default()
                    };
                    File::new(e.name, attrs)
                })
                .collect();

            Ok(Name { id, files })
        }
    }

    fn remove(
        &mut self,
        id: u32,
        filename: String,
    ) -> impl Future<Output = Result<Status, Self::Error>> + Send {
        async move {
            self.fs
                .delete_file(&filename)
                .await
                .map_err(|_| StatusCode::NoSuchFile)?;
            Ok(Self::ok_status(id))
        }
    }

    fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> impl Future<Output = Result<Status, Self::Error>> + Send {
        async move {
            self.fs
                .create_dir(&path)
                .await
                .map_err(|_| StatusCode::Failure)?;
            Ok(Self::ok_status(id))
        }
    }

    fn rmdir(
        &mut self,
        id: u32,
        path: String,
    ) -> impl Future<Output = Result<Status, Self::Error>> + Send {
        async move {
            self.fs
                .delete_file(&path)
                .await
                .map_err(|_| StatusCode::NoSuchFile)?;
            Ok(Self::ok_status(id))
        }
    }

    fn realpath(
        &mut self,
        id: u32,
        path: String,
    ) -> impl Future<Output = Result<Name, Self::Error>> + Send {
        async move {
            let safe_path = self.fs.resolve_safe_path(&path).map_err(|_| StatusCode::NoSuchFile)?;
            let rel_str = safe_path
                .strip_prefix(self.fs.root())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| ".".to_string());

            let final_path = if rel_str.is_empty() || rel_str == "." {
                "/".to_string()
            } else {
                format!("/{}", rel_str)
            };

            Ok(Name {
                id,
                files: vec![File::dummy(final_path)],
            })
        }
    }

    fn stat(
        &mut self,
        id: u32,
        path: String,
    ) -> impl Future<Output = Result<Attrs, Self::Error>> + Send {
        async move {
            let safe_path = self.fs.resolve_safe_path(&path).map_err(|_| StatusCode::NoSuchFile)?;
            let metadata = tokio::fs::metadata(&safe_path)
                .await
                .map_err(|_| StatusCode::NoSuchFile)?;

            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as u32);

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

            let attrs = FileAttributes {
                size: Some(metadata.len()),
                permissions: Some(permissions),
                mtime: modified,
                ..Default::default()
            };

            Ok(Attrs { id, attrs })
        }
    }

    fn lstat(
        &mut self,
        id: u32,
        path: String,
    ) -> impl Future<Output = Result<Attrs, Self::Error>> + Send {
        self.stat(id, path)
    }

    fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> impl Future<Output = Result<Status, Self::Error>> + Send {
        async move {
            self.fs
                .rename(&oldpath, &newpath)
                .await
                .map_err(|_| StatusCode::Failure)?;
            Ok(Self::ok_status(id))
        }
    }
}
