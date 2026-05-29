//! upload_policy 单元测试

use mistterm::core::upload_policy::*;

#[test]
fn format_bytes_short_bytes() {
    assert_eq!(format_bytes_short(0), "0 B");
    assert_eq!(format_bytes_short(100), "100 B");
    assert_eq!(format_bytes_short(1023), "1023 B");
}

#[test]
fn format_bytes_short_kilobytes() {
    assert_eq!(format_bytes_short(1024), "1.0 KB");
    assert_eq!(format_bytes_short(2048), "2.0 KB");
    assert_eq!(format_bytes_short(10240), "10.0 KB");
    assert_eq!(format_bytes_short(1536), "1.5 KB");
}

#[test]
fn format_bytes_short_megabytes() {
    assert_eq!(format_bytes_short(1024 * 1024), "1.0 MB");
    assert_eq!(format_bytes_short(2 * 1024 * 1024), "2.0 MB");
    assert_eq!(format_bytes_short(10 * 1024 * 1024), "10.0 MB");
    assert_eq!(format_bytes_short(1536 * 1024), "1.5 MB");
}

#[test]
fn format_bytes_short_large_values() {
    assert_eq!(format_bytes_short(1024 * 1024 * 1024), "1024.0 MB");
    assert_eq!(format_bytes_short(2 * 1024 * 1024 * 1024), "2048.0 MB");
}

#[test]
fn decide_upload_dispatch_no_active_tab() {
    let path = std::path::Path::new("/tmp/test.txt");
    let result = decide_upload_dispatch(path, false);
    assert_eq!(result, UploadDispatch::NoActiveTab);
}

#[test]
fn decide_upload_dispatch_small_file() {
    let path = std::path::Path::new("/nonexistent_small_file.txt");
    let result = decide_upload_dispatch(path, true);
    match result {
        UploadDispatch::ScpDirect { size_bytes } => {
            assert_eq!(size_bytes, 0);
        }
        _ => panic!("Expected ScpDirect for nonexistent file"),
    }
}

#[test]
fn decide_upload_dispatch_large_file_threshold() {
    let threshold = LARGE_UPLOAD_THRESHOLD_BYTES;
    assert_eq!(threshold, 10 * 1024 * 1024);

    let just_under = threshold - 1;
    let path_str = format!("/tmp/test_{}.txt", just_under);
    let path = std::path::Path::new(&path_str);

    if path.exists() {
        let result = decide_upload_dispatch(path, true);
        match result {
            UploadDispatch::ScpDirect { .. } => {}
            _ => panic!("Expected ScpDirect for file just under threshold"),
        }
    }
}

#[test]
fn upload_dispatch_debug() {
    let dispatch = UploadDispatch::ScpDirect { size_bytes: 1234 };
    let debug_str = format!("{:?}", dispatch);
    assert!(debug_str.contains("ScpDirect"));

    let dispatch2 = UploadDispatch::PromptLargeFile { size_bytes: 12345678 };
    let debug_str2 = format!("{:?}", dispatch2);
    assert!(debug_str2.contains("PromptLargeFile"));

    let dispatch3 = UploadDispatch::NoActiveTab;
    let debug_str3 = format!("{:?}", dispatch3);
    assert!(debug_str3.contains("NoActiveTab"));
}

#[test]
fn upload_dispatch_clone() {
    let dispatch = UploadDispatch::ScpDirect { size_bytes: 999 };
    let cloned = dispatch.clone();
    assert_eq!(dispatch, cloned);

    let dispatch2 = UploadDispatch::PromptLargeFile { size_bytes: 123456 };
    let cloned2 = dispatch2.clone();
    assert_eq!(dispatch2, cloned2);
}

#[test]
fn upload_dispatch_partial_eq() {
    assert_eq!(
        UploadDispatch::ScpDirect { size_bytes: 100 },
        UploadDispatch::ScpDirect { size_bytes: 100 }
    );
    assert_ne!(
        UploadDispatch::ScpDirect { size_bytes: 100 },
        UploadDispatch::ScpDirect { size_bytes: 200 }
    );
    assert_ne!(
        UploadDispatch::ScpDirect { size_bytes: 100 },
        UploadDispatch::PromptLargeFile { size_bytes: 100 }
    );
}