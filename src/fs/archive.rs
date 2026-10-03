use std::io::{Read, Write};
use std::path::Path;
use async_compression::tokio::bufread::{GzipDecoder, ZstdDecoder};
use async_compression::tokio::write::{GzipEncoder, ZstdEncoder};
use futures_util::StreamExt;
use tokio::fs::File;
use tokio::io::BufReader;
use tokio_tar::{Archive as AsyncTarArchive, Builder as AsyncTarBuilder};

use crate::error::TentacleError;
use crate::fs::sandbox::SandboxedFs;

pub enum ArchiveFormat {
    TarGz,
    TarZst,
    Tar,
    Zip,
}

impl ArchiveFormat {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Option<Self> {
        let path_str = path.as_ref().to_string_lossy().to_lowercase();
        if path_str.ends_with(".tar.gz") || path_str.ends_with(".tgz") {
            Some(Self::TarGz)
        } else if path_str.ends_with(".tar.zst") || path_str.ends_with(".tzst") {
            Some(Self::TarZst)
        } else if path_str.ends_with(".zip") {
            Some(Self::Zip)
        } else if path_str.ends_with(".tar") {
            Some(Self::Tar)
        } else {
            None
        }
    }
}

pub struct ArchiveEngine;

impl ArchiveEngine {
    pub async fn create_tar_gz<P: AsRef<Path>, Q: AsRef<Path>>(
        source_dir: P,
        dest_archive: Q,
    ) -> Result<(), TentacleError> {
        let dest_file = File::create(dest_archive).await?;
        let encoder = GzipEncoder::new(dest_file);
        let mut builder = AsyncTarBuilder::new(encoder);
        builder
            .append_dir_all(".", source_dir.as_ref())
            .await
            .map_err(|e| TentacleError::Archive(format!("Failed to build tar.gz: {}", e)))?;

        let mut encoder = builder
            .into_inner()
            .await
            .map_err(|e| TentacleError::Archive(format!("Failed to finalize tar builder: {}", e)))?;

        tokio::io::AsyncWriteExt::shutdown(&mut encoder).await?;
        Ok(())
    }

    pub async fn create_tar_zst<P: AsRef<Path>, Q: AsRef<Path>>(
        source_dir: P,
        dest_archive: Q,
    ) -> Result<(), TentacleError> {
        let dest_file = File::create(dest_archive).await?;
        let encoder = ZstdEncoder::new(dest_file);
        let mut builder = AsyncTarBuilder::new(encoder);
        builder
            .append_dir_all(".", source_dir.as_ref())
            .await
            .map_err(|e| TentacleError::Archive(format!("Failed to build tar.zst: {}", e)))?;

        let mut encoder = builder
            .into_inner()
            .await
            .map_err(|e| TentacleError::Archive(format!("Failed to finalize tar builder: {}", e)))?;

        tokio::io::AsyncWriteExt::shutdown(&mut encoder).await?;
        Ok(())
    }

    pub async fn create_zip<P: AsRef<Path>, Q: AsRef<Path>>(
        source_dir: P,
        dest_archive: Q,
    ) -> Result<(), TentacleError> {
        let src = source_dir.as_ref().to_path_buf();
        let dst = dest_archive.as_ref().to_path_buf();

        tokio::task::spawn_blocking(move || -> Result<(), TentacleError> {
            let file = std::fs::File::create(&dst)?;
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            let mut stack = vec![src.clone()];
            while let Some(current) = stack.pop() {
                if let Ok(entries) = std::fs::read_dir(&current) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let rel_path = path.strip_prefix(&src).map_err(|e| {
                            TentacleError::Archive(format!("Failed to strip path prefix: {}", e))
                        })?;
                        let name = rel_path.to_string_lossy().replace('\\', "/");

                        if path.is_dir() {
                            zip.add_directory(&name, options).map_err(|e| {
                                TentacleError::Archive(format!("Failed to add zip dir: {}", e))
                            })?;
                            stack.push(path);
                        } else if path.is_file() {
                            zip.start_file(&name, options).map_err(|e| {
                                TentacleError::Archive(format!("Failed to start zip file: {}", e))
                            })?;
                            let mut f = std::fs::File::open(&path)?;
                            let mut buf = Vec::new();
                            f.read_to_end(&mut buf)?;
                            zip.write_all(&buf)?;
                        }
                    }
                }
            }

            zip.finish()
                .map_err(|e| TentacleError::Archive(format!("Failed to finish zip: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| TentacleError::Internal(format!("Zip task failed: {}", e)))??;

        Ok(())
    }

    pub async fn extract_archive<P: AsRef<Path>>(
        archive_path: P,
        sandbox: &SandboxedFs,
    ) -> Result<(), TentacleError> {
        let path = archive_path.as_ref();
        let format = ArchiveFormat::from_path(path).ok_or_else(|| {
            TentacleError::Archive(format!(
                "Unsupported archive format for file: {}",
                path.display()
            ))
        })?;

        match format {
            ArchiveFormat::TarGz => {
                let file = File::open(path).await?;
                let buf_reader = BufReader::new(file);
                let decoder = GzipDecoder::new(buf_reader);
                let mut archive = AsyncTarArchive::new(decoder);
                Self::extract_async_tar(&mut archive, sandbox).await?;
            }
            ArchiveFormat::TarZst => {
                let file = File::open(path).await?;
                let buf_reader = BufReader::new(file);
                let decoder = ZstdDecoder::new(buf_reader);
                let mut archive = AsyncTarArchive::new(decoder);
                Self::extract_async_tar(&mut archive, sandbox).await?;
            }
            ArchiveFormat::Tar => {
                let file = File::open(path).await?;
                let buf_reader = BufReader::new(file);
                let mut archive = AsyncTarArchive::new(buf_reader);
                Self::extract_async_tar(&mut archive, sandbox).await?;
            }
            ArchiveFormat::Zip => {
                Self::extract_zip(path, sandbox).await?;
            }
        }

        Ok(())
    }

    async fn extract_async_tar<R: tokio::io::AsyncRead + Unpin + Send>(
        archive: &mut AsyncTarArchive<R>,
        sandbox: &SandboxedFs,
    ) -> Result<(), TentacleError> {
        let mut entries = archive
            .entries()
            .map_err(|e| TentacleError::Archive(format!("Tar read entries error: {}", e)))?;

        while let Some(entry_result) = entries.next().await {
            let mut entry = entry_result
                .map_err(|e| TentacleError::Archive(format!("Tar entry read error: {}", e)))?;

            let raw_path = entry
                .path()
                .map_err(|e| TentacleError::Archive(format!("Tar entry path error: {}", e)))?
                .to_path_buf();

            let safe_dest = sandbox.resolve_safe_path(&raw_path)?;

            if entry.header().entry_type().is_dir() {
                tokio::fs::create_dir_all(&safe_dest).await?;
            } else {
                if let Some(parent) = safe_dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                entry
                    .unpack(&safe_dest)
                    .await
                    .map_err(|e| TentacleError::Archive(format!("Tar entry unpack error: {}", e)))?;
            }
        }

        Ok(())
    }

    async fn extract_zip<P: AsRef<Path>>(
        archive_path: P,
        sandbox: &SandboxedFs,
    ) -> Result<(), TentacleError> {
        let path = archive_path.as_ref().to_path_buf();
        let sandbox_clone = sandbox.clone();

        tokio::task::spawn_blocking(move || -> Result<(), TentacleError> {
            let file = std::fs::File::open(&path)?;
            let mut archive = zip::ZipArchive::new(file)
                .map_err(|e| TentacleError::Archive(format!("Failed to open zip: {}", e)))?;

            for i in 0..archive.len() {
                let mut file = archive
                    .by_index(i)
                    .map_err(|e| TentacleError::Archive(format!("Zip read entry error: {}", e)))?;

                let enclosed_path = match file.enclosed_name() {
                    Some(path) => path.to_owned(),
                    None => {
                        return Err(TentacleError::PathTraversal(format!(
                            "Zip entry attempts path traversal: {}",
                            file.name()
                        )))
                    }
                };

                let safe_path = sandbox_clone.resolve_safe_path(&enclosed_path)?;

                if file.is_dir() {
                    std::fs::create_dir_all(&safe_path)?;
                } else {
                    if let Some(p) = safe_path.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    let mut outfile = std::fs::File::create(&safe_path)?;
                    std::io::copy(&mut file, &mut outfile)?;
                }
            }

            Ok(())
        })
        .await
        .map_err(|e| TentacleError::Internal(format!("Zip task failed: {}", e)))??;

        Ok(())
    }
}
