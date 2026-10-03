use tentacle::fs::{ArchiveEngine, DiskQuota, SandboxedFs};

#[tokio::test]
async fn test_sandbox_path_resolution() {
    let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
    let sandbox = SandboxedFs::new(temp_dir.path()).expect("failed to create sandbox");

    // Root should resolve cleanly
    let root = sandbox.resolve_safe_path(".").unwrap();
    assert_eq!(root, dunce::canonicalize(temp_dir.path()).unwrap());

    // Clean relative path
    let file_path = sandbox.resolve_safe_path("config/server.properties").unwrap();
    assert!(file_path.starts_with(sandbox.root()));

    // Traversal attack
    let traversal = sandbox.resolve_safe_path("../../../etc/passwd");
    assert!(traversal.is_err());

    // Complex traversal with self dots
    let attack = sandbox.resolve_safe_path("foo/../../../../windows/system32");
    assert!(attack.is_err());
}

#[tokio::test]
async fn test_sandbox_file_crud() {
    let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
    let sandbox = SandboxedFs::new(temp_dir.path()).expect("failed to create sandbox");

    // Write file
    sandbox
        .write_file("sub/test.txt", b"Hello Octopus!")
        .await
        .unwrap();

    // Read file
    let content = sandbox.read_file("sub/test.txt").await.unwrap();
    assert_eq!(content, b"Hello Octopus!");

    // List dir
    let entries = sandbox.list_dir("sub").await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "test.txt");
    assert_eq!(entries[0].size, 14);
    assert!(!entries[0].is_dir);

    // Rename
    sandbox.rename("sub/test.txt", "sub/renamed.txt").await.unwrap();
    assert!(sandbox.read_file("sub/renamed.txt").await.is_ok());
    assert!(sandbox.read_file("sub/test.txt").await.is_err());

    // Delete
    sandbox.delete_file("sub/renamed.txt").await.unwrap();
    let entries_after = sandbox.list_dir("sub").await.unwrap();
    assert_eq!(entries_after.len(), 0);
}

#[tokio::test]
async fn test_disk_quota_and_archive() {
    let temp_src = tempfile::tempdir().expect("failed to create tempdir");
    let temp_dst = tempfile::tempdir().expect("failed to create tempdir");

    let src_sandbox = SandboxedFs::new(temp_src.path()).unwrap();
    let dst_sandbox = SandboxedFs::new(temp_dst.path()).unwrap();

    src_sandbox
        .write_file("sample.txt", b"Testing Archive Compression")
        .await
        .unwrap();
    src_sandbox
        .write_file("nested/data.json", b"{\"name\":\"tentacle\"}")
        .await
        .unwrap();

    // Check quota
    let usage = DiskQuota::calculate_usage(src_sandbox.root()).await.unwrap();
    assert!(usage > 0);

    let quota_check = DiskQuota::check_quota(src_sandbox.root(), Some(10), 5).await;
    assert!(quota_check.is_err()); // Quota exceeded

    // Tar.gz creation and extraction
    let tar_gz_path = temp_dst.path().join("backup.tar.gz");
    ArchiveEngine::create_tar_gz(src_sandbox.root(), &tar_gz_path)
        .await
        .unwrap();
    assert!(tar_gz_path.exists());

    ArchiveEngine::extract_archive(&tar_gz_path, &dst_sandbox)
        .await
        .unwrap();
    let extracted_content = dst_sandbox.read_file("sample.txt").await.unwrap();
    assert_eq!(extracted_content, b"Testing Archive Compression");
}
