//! SFTP 集成测试（需本地 sshd；无 sshd 时自动 skip）。
//!
//! 环境变量：`MISTTERM_TEST_SSH_HOST` / `PORT` / `USER` / `PASSWORD`
//! 默认 `127.0.0.1:22` / `root` / `mistterm123`

use std::io::{Read, Write};
use std::path::Path;

use mistterm::test_support::ssh_local::{open_sftp, skip_without_sshd};

#[test]
fn test_sftp_list_dir() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let sftp = open_sftp(&session).expect("Failed to open SFTP channel");

    let entries = sftp
        .readdir(Path::new("/tmp"))
        .expect("Failed to list /tmp");

    assert!(!entries.is_empty(), "/tmp should have entries");

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

    let test_dir = Path::new("/tmp/mistterm_sftp_test");
    sftp.mkdir(test_dir, 0o755).ok();

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
    sftp.rmdir(test_dir).expect("Failed to remove test dir");
}

#[test]
fn test_sftp_mkdir_rmdir() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let sftp = open_sftp(&session).expect("Failed to open SFTP channel");

    let test_dir = Path::new("/tmp/mistterm_mkdir_test");

    sftp.mkdir(test_dir, 0o755).expect("Failed to mkdir");

    let stat = sftp.stat(test_dir).expect("Failed to stat dir");
    assert!(stat.is_dir(), "Should be a directory");

    sftp.rmdir(test_dir).expect("Failed to rmdir");
    assert!(sftp.stat(test_dir).is_err(), "Dir should be deleted");
}
