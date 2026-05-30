//! SFTP integration tests
//!
//! Requires sshd running on localhost:22
//! Run with: cargo test --test sftp_integration_test -- --ignored --nocapture

use ssh2::Session;
use std::net::TcpStream;
use std::path::Path;

use std::io::{Read, Write};

fn connect_ssh() -> Session {
    let tcp = TcpStream::connect("127.0.0.1:22")
        .expect("Failed to connect to sshd");
    let mut sess = Session::new().expect("Failed to create SSH session");
    sess.set_tcp_stream(tcp);
    sess.handshake().expect("SSH handshake failed");
    sess.userauth_password("root", "mistterm123")
        .expect("SSH authentication failed");
    sess
}

#[test]
#[ignore]
fn test_sftp_list_dir() {
    let session = connect_ssh();
    let sftp = session.sftp().expect("Failed to open SFTP channel");
    
    // List /tmp directory
    let entries = sftp.readdir(Path::new("/tmp"))
        .expect("Failed to list /tmp");
    
    assert!(!entries.is_empty(), "/tmp should have entries");
    
    for (path, stat) in &entries {
        println!("{}: {:?} {}B", path.display(), stat.is_dir(), stat.size.unwrap_or(0));
    }
}

#[test]
#[ignore]
fn test_sftp_upload_download() {
    let session = connect_ssh();
    let sftp = session.sftp().expect("Failed to open SFTP channel");
    
    // Create test directory
    let test_dir = Path::new("/tmp/mistterm_sftp_test");
    sftp.mkdir(test_dir, 0o755).ok(); // Ignore already exists error

    // Write test file
    let remote_file = test_dir.join("test.txt");
    let test_content = b"Hello, SFTP Integration Test!";
    
    {
        let mut file = sftp.create(&remote_file)
            .expect("Failed to create remote file");
        file.write_all(test_content).expect("Failed to write content");
    }
    
    // Read back
    let downloaded = {
        let mut file = sftp.open(&remote_file).expect("Failed to open remote file");
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).expect("Failed to read content");
        buf
    };
    
    assert_eq!(downloaded.as_slice(), test_content, "Downloaded content should match");
    
    // Clean up
    sftp.unlink(&remote_file).expect("Failed to delete file");
    sftp.rmdir(test_dir).expect("Failed to remove test dir");
}

#[test]
#[ignore]
fn test_sftp_mkdir_rmdir() {
    let session = connect_ssh();
    let sftp = session.sftp().expect("Failed to open SFTP channel");
    
    let test_dir = Path::new("/tmp/mistterm_mkdir_test");
    
    // Create directory
    sftp.mkdir(test_dir, 0o755).expect("Failed to mkdir");

    // Verify it exists
    let stat = sftp.stat(test_dir).expect("Failed to stat dir");
    assert!(stat.is_dir(), "Should be a directory");

    // Delete
    sftp.rmdir(test_dir).expect("Failed to rmdir");

    // Verify it's deleted
    assert!(sftp.stat(test_dir).is_err(), "Dir should be deleted");
}
