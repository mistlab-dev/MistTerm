//! minisign 验签、可信注释核对、SHA-256 计算。

use std::io::Read;
use std::path::Path;

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

use super::error::UpdateError;
use super::manifest::Manifest;

/// 一组可信公钥（任一把验证通过即可）。
#[derive(Clone)]
pub struct TrustedKeys {
    keys: Vec<PublicKey>,
}

impl std::fmt::Debug for TrustedKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TrustedKeys({} keys)", self.keys.len())
    }
}

impl TrustedKeys {
    /// 本构建内置的公钥（见 `keys.rs`）。
    pub fn embedded() -> Self {
        let lines = super::keys::embedded_pubkeys();
        Self::from_base64(lines.iter().map(String::as_str))
    }

    /// 从 base64 公钥构造；格式不对的会被丢弃并记日志。
    pub fn from_base64<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        let keys = lines
            .into_iter()
            .filter_map(|l| match PublicKey::from_base64(l.trim()) {
                Ok(k) => Some(k),
                Err(e) => {
                    log::warn!("updater: ignoring malformed public key: {e}");
                    None
                }
            })
            .collect();
        Self { keys }
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// 只要有一把公钥能验证就通过；只接受预哈希签名（minisign 0.8+ 的默认格式）。
    pub fn verify(&self, data: &[u8], signature_text: &str) -> Result<Signature, UpdateError> {
        if self.keys.is_empty() {
            return Err(UpdateError::NoTrustedKeys);
        }
        let sig = Signature::decode(signature_text.trim()).map_err(|_| UpdateError::BadSignature)?;
        for key in &self.keys {
            if key.verify(data, &sig, false).is_ok() {
                return Ok(sig);
            }
        }
        Err(UpdateError::BadSignature)
    }
}

/// 先验签、再解析：签名通过后才解析 JSON，并核对可信注释与清单内容一致。
pub fn verify_manifest(
    manifest_bytes: &[u8],
    signature_bytes: &[u8],
    keys: &TrustedKeys,
    expected_channel: &str,
    allow_loopback_http: bool,
) -> Result<Manifest, UpdateError> {
    let sig_text = std::str::from_utf8(signature_bytes).map_err(|_| UpdateError::BadSignature)?;
    let sig = keys.verify(manifest_bytes, sig_text)?;
    let manifest = Manifest::parse_and_validate(manifest_bytes, expected_channel, allow_loopback_http)?;
    if sig.trusted_comment().trim() != manifest.expected_trusted_comment() {
        return Err(UpdateError::InvalidManifest(format!(
            "trusted comment {:?} does not match manifest",
            sig.trusted_comment()
        )));
    }
    Ok(manifest)
}

/// 计算文件 SHA-256（小写十六进制）。
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

pub fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// 常量时间无关紧要（SHA-256 不是机密），但统一大小写后比较。
pub fn sha256_matches(expected: &str, actual: &str) -> bool {
    expected.trim().eq_ignore_ascii_case(actual.trim())
}

#[cfg(test)]
pub(crate) mod test_signing {
    //! 测试用：运行时生成临时 minisign 密钥并签名（dev-dependency `minisign`）。
    use std::io::Cursor;

    pub struct TestSigner {
        pub pk_base64: String,
        sk: minisign::SecretKey,
    }

    impl TestSigner {
        pub fn new() -> Self {
            let minisign::KeyPair { pk, sk } =
                minisign::KeyPair::generate_unencrypted_keypair().expect("keygen");
            Self {
                pk_base64: pk.to_base64(),
                sk,
            }
        }

        pub fn sign(&self, data: &[u8], trusted_comment: &str) -> String {
            minisign::sign(
                None,
                &self.sk,
                Cursor::new(data),
                Some(trusted_comment),
                Some("signature from MistTerm test key"),
            )
            .expect("sign")
            .into_string()
        }

        pub fn keys(&self) -> super::TrustedKeys {
            super::TrustedKeys::from_base64([self.pk_base64.as_str()])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_signing::TestSigner;
    use super::*;
    use crate::core::updater::manifest::tests::sample_json;

    const TC: &str = "mistterm stable 1.2.0 2026-10-05T12:00:00Z";

    fn sample() -> String {
        sample_json("1.2.0", "2026-10-05T12:00:00Z")
    }

    #[test]
    fn valid_signature_passes() {
        let s = TestSigner::new();
        let json = sample();
        let sig = s.sign(json.as_bytes(), TC);
        let m = verify_manifest(json.as_bytes(), sig.as_bytes(), &s.keys(), "stable", false).unwrap();
        assert_eq!(m.version, "1.2.0");
    }

    #[test]
    fn one_byte_tamper_fails() {
        let s = TestSigner::new();
        let json = sample();
        let sig = s.sign(json.as_bytes(), TC);
        let tampered = json.replace("\"size\": 123", "\"size\": 124");
        assert!(matches!(
            verify_manifest(tampered.as_bytes(), sig.as_bytes(), &s.keys(), "stable", false),
            Err(UpdateError::BadSignature)
        ));
    }

    #[test]
    fn wrong_key_fails() {
        let s = TestSigner::new();
        let other = TestSigner::new();
        let json = sample();
        let sig = s.sign(json.as_bytes(), TC);
        assert!(matches!(
            verify_manifest(json.as_bytes(), sig.as_bytes(), &other.keys(), "stable", false),
            Err(UpdateError::BadSignature)
        ));
    }

    #[test]
    fn second_key_is_accepted() {
        // 备用钥匙签名也要能通过（换钥场景）。
        let primary = TestSigner::new();
        let backup = TestSigner::new();
        let keys = TrustedKeys::from_base64([primary.pk_base64.as_str(), backup.pk_base64.as_str()]);
        let json = sample();
        let sig = backup.sign(json.as_bytes(), TC);
        assert!(verify_manifest(json.as_bytes(), sig.as_bytes(), &keys, "stable", false).is_ok());
    }

    #[test]
    fn trusted_comment_mismatch_fails() {
        let s = TestSigner::new();
        let json = sample();
        for tc in [
            "mistterm stable 1.1.0 2026-10-05T12:00:00Z",
            "mistterm beta 1.2.0 2026-10-05T12:00:00Z",
            "mistterm stable 1.2.0 2026-10-04T12:00:00Z",
            "timestamp:1 file:latest.json",
        ] {
            let sig = s.sign(json.as_bytes(), tc);
            assert!(
                matches!(
                    verify_manifest(json.as_bytes(), sig.as_bytes(), &s.keys(), "stable", false),
                    Err(UpdateError::InvalidManifest(_))
                ),
                "{tc}"
            );
        }
    }

    #[test]
    fn html_page_and_garbage_signature_fail() {
        let s = TestSigner::new();
        let html = b"<!doctype html><html><head><title>MistLab</title></head></html>";
        assert!(matches!(
            verify_manifest(html, html, &s.keys(), "stable", false),
            Err(UpdateError::BadSignature)
        ));
        let json = sample();
        assert!(verify_manifest(json.as_bytes(), b"", &s.keys(), "stable", false).is_err());
        assert!(verify_manifest(json.as_bytes(), &[0xff, 0xfe], &s.keys(), "stable", false).is_err());
    }

    #[test]
    fn no_keys_means_no_trust() {
        let s = TestSigner::new();
        let json = sample();
        let sig = s.sign(json.as_bytes(), TC);
        let empty = TrustedKeys::from_base64(Vec::<&str>::new());
        assert!(matches!(
            verify_manifest(json.as_bytes(), sig.as_bytes(), &empty, "stable", false),
            Err(UpdateError::NoTrustedKeys)
        ));
    }

    #[test]
    fn accepts_minisign_cli_signature() {
        // 由 minisign 0.12 命令行（`echo pw | minisign -S -t ...`）生成，确认与 CI 用的工具格式一致。
        let pk = "RWTw6fTN7AeJMV2AcMgIoyLDXxzD430boD4J1d6V//064JvJxYyYyKBn";
        let sig = "untrusted comment: signature from minisign secret key\nRUTw6fTN7AeJMVQ3VKMsstRobCdeyEVWvvidMhvniHojJ5MU2i89BlmU16m84LUGQa5ulylpJ6GWHgOgJn5HMrI3v6yo6j26YgQ=\ntrusted comment: mistterm stable 1.2.0 2026-10-05T12:00:00Z\ndEbBkqUjuRmBbuZZRnSee5200hHhFX3ct+bv36O2RbrDlf1Ru90/tXtZ4kHmEyl44aMptk2+6JhH7/agTq7PCg==\n";
        let keys = TrustedKeys::from_base64([pk]);
        let got = keys.verify(b"hello\n", sig).unwrap();
        assert_eq!(got.trusted_comment(), TC);
        assert!(keys.verify(b"hello!\n", sig).is_err());
    }

    #[test]
    fn sha256_of_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"hello\n").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
        assert!(sha256_matches(
            "5891B5B522D5DF086D0FF0B110FBD9D21BB4FC7163AF34D08286A2E846F6BE03",
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        ));
    }
}
