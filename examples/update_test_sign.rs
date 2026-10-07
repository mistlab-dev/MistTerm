//! 自动更新端到端测试用的签名工具（**仅测试**，使用运行时生成的临时密钥）。
//!
//! ```text
//! cargo run --example update_test_sign -- keygen <out-dir>      # 生成 test.key / test.pub，打印 base64 公钥
//! cargo run --example update_test_sign -- sign <test.key> <file> <trusted-comment>   # 写出 <file>.minisig
//! ```
//!
//! 生成的签名格式与正式发布用的 `minisign` 命令行完全一致（预哈希 Ed25519）。
//! 正式签名密钥只在 Tian 自己的电脑上生成，绝不经过这个工具。

use std::io::Cursor;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") if args.len() == 2 => keygen(PathBuf::from(&args[1])),
        Some("sign") if args.len() == 4 => sign(PathBuf::from(&args[1]), PathBuf::from(&args[2]), &args[3]),
        _ => {
            eprintln!("usage: update_test_sign keygen <out-dir> | sign <test.key> <file> <trusted-comment>");
            std::process::exit(2);
        }
    }
}

fn keygen(dir: PathBuf) {
    std::fs::create_dir_all(&dir).expect("create out dir");
    let minisign::KeyPair { pk, sk } = minisign::KeyPair::generate_unencrypted_keypair().expect("keygen");
    let pk_box = pk.to_box().expect("pk box").into_string();
    let sk_box = sk
        .to_box(Some("MistTerm update TEST key - never trust in release builds"))
        .expect("sk box")
        .into_string();
    std::fs::write(dir.join("test.pub"), pk_box).expect("write test.pub");
    std::fs::write(dir.join("test.key"), sk_box).expect("write test.key");
    println!("{}", pk.to_base64());
}

fn sign(key: PathBuf, file: PathBuf, trusted_comment: &str) {
    let sk_box = minisign::SecretKeyBox::from_string(&std::fs::read_to_string(&key).expect("read key"))
        .expect("parse key");
    let sk = sk_box.into_unencrypted_secret_key().expect("unencrypted test key");
    let data = std::fs::read(&file).expect("read file");
    let sig = minisign::sign(
        None,
        &sk,
        Cursor::new(data),
        Some(trusted_comment),
        Some("signature from MistTerm update TEST key"),
    )
    .expect("sign");
    let mut out = file.clone().into_os_string();
    out.push(".minisig");
    std::fs::write(PathBuf::from(out), sig.into_string()).expect("write signature");
}
