//! SFTP 集成测试（需本地 sshd；无 sshd 时自动 skip）。
//!
//! 环境变量：`MISTTERM_TEST_SSH_HOST` / `PORT` / `USER` / `PASSWORD` / `SFTP_ROOT`
//! Windows 默认 `mistterm_test@127.0.0.1`，远端 `C:/Users/mistterm_test/mistterm_sftp`

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mistterm::ssh::{SftpClient, TransferResult, WriteMode};
use mistterm::test_support::ssh_local::{open_sftp, skip_without_sshd, ssh_remote_sftp_root};

fn sftp_root_path() -> PathBuf {
    PathBuf::from(ssh_remote_sftp_root())
}

#[test]
fn test_sftp_list_dir() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let sftp = open_sftp(&session).expect("Failed to open SFTP channel");

    let root = sftp_root_path();
    let entries = sftp.readdir(&root).expect("Failed to list SFTP root");
    println!("{} entries under {}", entries.len(), root.display());
    for (path, stat) in &entries {
        println!(
            "{}: {:?} {}B",
            path.display(),
            stat.is_dir(),
            stat.size.unwrap_or(0)
        );
    }
}

#[test]
fn test_sftp_upload_download() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let sftp = open_sftp(&session).expect("Failed to open SFTP channel");

    let test_dir = sftp_root_path().join("mistterm_sftp_test");
    let _ = sftp.mkdir(&test_dir, 0o755);

    let remote_file = test_dir.join("test.txt");
    let test_content = b"Hello, SFTP Integration Test!";

    {
        let mut file = sftp
            .create(&remote_file)
            .expect("Failed to create remote file");
        file.write_all(test_content)
            .expect("Failed to write content");
    }

    let downloaded = {
        let mut file = sftp
            .open(&remote_file)
            .expect("Failed to open remote file");
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).expect("Failed to read content");
        buf
    };

    assert_eq!(
        downloaded.as_slice(),
        test_content,
        "Downloaded content should match"
    );

    sftp.unlink(&remote_file).expect("Failed to delete file");
    sftp.rmdir(&test_dir).expect("Failed to remove test dir");
}

#[test]
fn test_sftp_mkdir_rmdir() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let sftp = open_sftp(&session).expect("Failed to open SFTP channel");

    let test_dir = sftp_root_path().join("mistterm_mkdir_test");

    sftp.mkdir(&test_dir, 0o755).expect("Failed to mkdir");

    let stat = sftp.stat(&test_dir).expect("Failed to stat dir");
    assert!(stat.is_dir(), "Should be a directory");

    sftp.rmdir(&test_dir).expect("Failed to rmdir");
    assert!(sftp.stat(&test_dir).is_err(), "Dir should be deleted");
}

#[test]
fn test_sftp_upload_ex_progress_and_download() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let client = SftpClient::new(&session).expect("SftpClient");
    let remote_dir = sftp_root_path().join("mistterm_xfer_ex");
    let _ = client.mkdir(&remote_dir);
    let remote = remote_dir.join("progress.bin");
    let local_up = std::env::temp_dir().join("mistterm_xfer_progress_up.bin");
    let local_down = std::env::temp_dir().join("mistterm_xfer_progress_down.bin");
    let payload = vec![0xABu8; 512 * 1024];
    std::fs::write(&local_up, &payload).unwrap();
    let _ = std::fs::remove_file(&local_down);

    let mut last = 0u64;
    let mut ticks = 0u32;
    let up = client
        .upload_ex(
            &local_up,
            &remote,
            WriteMode::Overwrite,
            None,
            &mut |p| {
                assert!(p.done >= last);
                last = p.done;
                ticks += 1;
            },
        )
        .expect("upload_ex");
    assert!(matches!(up, TransferResult::Completed { bytes } if bytes == payload.len() as u64));
    assert!(ticks >= 1, "expected progress callbacks");

    last = 0;
    ticks = 0;
    let down = client
        .download_ex(
            &remote,
            &local_down,
            WriteMode::Overwrite,
            None,
            &mut |p| {
                assert!(p.done >= last);
                last = p.done;
                ticks += 1;
            },
        )
        .expect("download_ex");
    assert!(matches!(down, TransferResult::Completed { bytes } if bytes == payload.len() as u64));
    assert_eq!(std::fs::read(&local_down).unwrap(), payload);

    let _ = client.remove(&remote);
    let _ = std::fs::remove_file(&local_up);
    let _ = std::fs::remove_file(&local_down);
}

#[test]
fn test_sftp_upload_cancel_then_resume() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let client = SftpClient::new(&session).expect("SftpClient");
    let remote_dir = sftp_root_path().join("mistterm_xfer_resume");
    let _ = client.mkdir(&remote_dir);
    let remote = remote_dir.join("resume.bin");
    let local = std::env::temp_dir().join("mistterm_xfer_resume_local.bin");
    let payload = vec![0x5Au8; 1024 * 1024];
    std::fs::write(&local, &payload).unwrap();
    let _ = client.remove(&remote);

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let mut saw = 0u64;
    let cancelled = client
        .upload_ex(
            &local,
            &remote,
            WriteMode::Overwrite,
            Some(&cancel),
            &mut |p| {
                saw = p.done;
                if p.done > 64 * 1024 {
                    cancel_flag.store(true, Ordering::Relaxed);
                }
            },
        )
        .expect("upload cancel");
    match cancelled {
        TransferResult::Cancelled { bytes_done } => {
            assert!(bytes_done > 0 && bytes_done < payload.len() as u64);
            assert_eq!(bytes_done, saw);
        }
        other => panic!("expected Cancelled, got {other:?}"),
    }

    let partial = client.remote_size(&remote).unwrap_or(0);
    assert!(partial > 0 && partial < payload.len() as u64);

    let resumed = client
        .upload_ex(
            &local,
            &remote,
            WriteMode::Resume,
            None,
            &mut |_| {},
        )
        .expect("upload resume");
    assert!(matches!(
        resumed,
        TransferResult::Completed { bytes } if bytes == payload.len() as u64
    ));
    assert_eq!(
        client.remote_size(&remote),
        Some(payload.len() as u64)
    );

    let skipped = client
        .upload_ex(
            &local,
            &remote,
            WriteMode::Resume,
            None,
            &mut |_| {},
        )
        .expect("upload skip complete");
    assert_eq!(skipped, TransferResult::Skipped);

    let _ = client.remove(&remote);
    let _ = std::fs::remove_file(&local);
}

#[test]
fn test_sftp_download_cancel_then_resume() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let client = SftpClient::new(&session).expect("SftpClient");
    let remote_dir = sftp_root_path().join("mistterm_xfer_dl_resume");
    let _ = client.mkdir(&remote_dir);
    let remote = remote_dir.join("dl_resume.bin");
    let local_src = std::env::temp_dir().join("mistterm_xfer_dl_src.bin");
    let local_dst = std::env::temp_dir().join("mistterm_xfer_dl_dst.bin");
    let payload = vec![0xCDu8; 768 * 1024];
    std::fs::write(&local_src, &payload).unwrap();
    let _ = std::fs::remove_file(&local_dst);
    client
        .upload_ex(
            &local_src,
            &remote,
            WriteMode::Overwrite,
            None,
            &mut |_| {},
        )
        .expect("seed remote");

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let cancelled = client
        .download_ex(
            &remote,
            &local_dst,
            WriteMode::Overwrite,
            Some(&cancel),
            &mut |p| {
                if p.done > 48 * 1024 {
                    cancel_flag.store(true, Ordering::Relaxed);
                }
            },
        )
        .expect("download cancel");
    assert!(matches!(cancelled, TransferResult::Cancelled { .. }));
    let partial = std::fs::metadata(&local_dst).map(|m| m.len()).unwrap_or(0);
    assert!(partial > 0 && partial < payload.len() as u64);

    let resumed = client
        .download_ex(
            &remote,
            &local_dst,
            WriteMode::Resume,
            None,
            &mut |_| {},
        )
        .expect("download resume");
    assert!(matches!(
        resumed,
        TransferResult::Completed { bytes } if bytes == payload.len() as u64
    ));
    assert_eq!(std::fs::read(&local_dst).unwrap(), payload);

    let _ = client.remove(&remote);
    let _ = std::fs::remove_file(&local_src);
    let _ = std::fs::remove_file(&local_dst);
}

#[test]
fn test_sftp_chmod_and_collect_tree() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let client = SftpClient::new(&session).expect("SftpClient");
    let root = sftp_root_path().join("mistterm_chmod_tree");
    let _ = client.remove(&root);
    client.mkdir_p(&root.join("sub/empty")).expect("mkdir_p");
    let file = root.join("sub/a.txt");
    let local = std::env::temp_dir().join("mistterm_chmod_seed.txt");
    std::fs::write(&local, b"chmod-tree").unwrap();
    client
        .upload_ex(&local, &file, WriteMode::Overwrite, None, &mut |_| {})
        .expect("seed");
    client.chmod(&file, 0o755).expect("chmod");
    let entry = client.stat(&file).expect("stat");
    // Windows OpenSSH 常忽略 Unix 权限位；Linux/macOS 上应能看到 x。
    #[cfg(not(windows))]
    assert!(
        entry.permissions.contains('x'),
        "expected exec bit, got {}",
        entry.permissions
    );
    #[cfg(windows)]
    {
        let _ = entry;
    }

    let tree = client.collect_tree(&root, None).expect("collect_tree");
    assert!(
        tree.iter().any(|(_, rel, is_dir)| !is_dir && rel.ends_with("a.txt")),
        "missing file in tree: {tree:?}"
    );
    assert!(
        tree.iter()
            .any(|(_, rel, is_dir)| *is_dir && rel.ends_with("empty")),
        "missing empty dir in tree: {tree:?}"
    );

    let _ = client.remove(&root);
    let _ = std::fs::remove_file(&local);
}
