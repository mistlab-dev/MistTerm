//! 从 Xshell、FinalShell 导入会话（主机、分组、端口、用户名，能读出时连密码一起）。
//!
//! 格式依据（都是公开资料，见 `docs/manual/从Xshell和FinalShell导入.md`）：
//! - Xshell：每个会话一个 `.xsh`（INI 格式，Xshell 6/7 多为 UTF-16），分组就是 `Sessions` 下的子文件夹；
//!   「文件 → 导出」得到的 `.xts` 是 zip，里面是同样的 `.xsh` 加一个 `xts.zcf`（记着导出时的 Windows 账号）。
//!   密码：5.0 及以前用固定密钥；5.1 起和「原电脑的 Windows 账号名 + SID」绑定；设了主密码则只能用主密码解。
//! - FinalShell：数据目录下 `conn/**/<id>_connect_config.json` 是主机，`conn/**/folder.json` 是分组（`parent_id` 串成树）；
//!   密码是 DES 加密后 base64，密钥由密文前 8 字节推出来，任何电脑都能解。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use sha2::{Digest, Sha256};

use super::session::SessionConfig;

/// 来源软件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignSource {
    Xshell,
    FinalShell,
}

impl ForeignSource {
    pub fn label(self) -> &'static str {
        match self {
            ForeignSource::Xshell => "Xshell",
            ForeignSource::FinalShell => "FinalShell",
        }
    }
}

/// 一条待导入的会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignCandidate {
    pub source: ForeignSource,
    pub name: String,
    /// 分组；多级用「/」连接，例如 `生产/数据库`。
    pub group: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// 读出来的明文密码；`None` 表示没保存或读不出。
    pub password: Option<String>,
    /// 给用户看的提醒（例如「密码读不出来，导入后请重新输入」）。
    pub notes: Vec<String>,
    /// 不能导入的原因（例如 Telnet、远程桌面）。
    pub skip_reason: Option<String>,
    /// 防止重复导入的标记（存进 `SessionConfig::ssh_config_marker`）。
    pub marker: String,
}

impl ForeignCandidate {
    pub fn importable(&self) -> bool {
        self.skip_reason.is_none() && !self.host.trim().is_empty()
    }

    pub fn display_target(&self) -> String {
        let user = if self.username.is_empty() {
            String::new()
        } else {
            format!("{}@", self.username)
        };
        if self.port == 22 {
            format!("{user}{}", self.host)
        } else {
            format!("{user}{}:{}", self.host, self.port)
        }
    }
}

/// 解析结果（含不致命的提醒）。
#[derive(Debug, Clone, Default)]
pub struct ForeignParseResult {
    pub candidates: Vec<ForeignCandidate>,
    pub warnings: Vec<String>,
}

/// 解 Xshell 5.1+ 密码要用的「原电脑 Windows 账号」。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XshellAccount {
    pub user: String,
    pub sid: String,
}

/// 解析选项。
#[derive(Debug, Clone, Default)]
pub struct ForeignImportOptions {
    /// 额外尝试的 Windows 账号（Windows 上会自动加上当前账号；`.xts` 导出文件里自带的也会用）。
    pub xshell_accounts: Vec<XshellAccount>,
    /// Xshell 设置了主密码时，用户提供的主密码。
    pub xshell_master_password: Option<String>,
}

const NOTE_REENTER: &str = "密码没能读出来，导入后请在会话里重新输入";
const NOTE_NO_PASSWORD: &str = "原来没有保存密码，导入后请在会话里填上";

// ---------------------------------------------------------------- 加解密小工具

/// RC4（Xshell 用）。
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: [u8; 256] = [0; 256];
    for (i, v) in s.iter_mut().enumerate() {
        *v = i as u8;
    }
    let mut j: u8 = 0;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[i as usize]);
            s.swap(i as usize, j as usize);
            b ^ s[s[i as usize].wrapping_add(s[j as usize]) as usize]
        })
        .collect()
}

fn b64(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()
}

/// `.xts` 里 `xts.zcf` 字段：RC4(MD5("!X@s#c$e%l^l&")) + MD5 校验。
fn xts_decrypt_field(s: &str) -> Option<String> {
    let data = b64(s)?;
    if data.len() < 16 {
        return None;
    }
    let (ct, sum) = data.split_at(data.len() - 16);
    let key = md5::compute(b"!X@s#c$e%l^l&");
    let pt = rc4(&key.0, ct);
    if md5::compute(&pt).0 != sum {
        return None;
    }
    String::from_utf8(pt).ok()
}

/// 解 Xshell 会话里的 `Password=`。返回 `None` 表示密钥不对 / 读不出。
fn xshell_decrypt_password(version: &str, enc: &str, opts: &ForeignImportOptions, extra: &[XshellAccount]) -> Option<String> {
    let data = b64(enc)?;
    if data.is_empty() {
        return None;
    }
    let ver = parse_version(version);
    if ver < (5, 1) {
        let key = md5::compute(b"!X@s#h$e%l^l&");
        return String::from_utf8(rc4(&key.0, &data)).ok();
    }
    if data.len() <= 32 {
        return None;
    }
    let (ct, sum) = data.split_at(data.len() - 32);
    let try_key = |key_material: &[u8]| -> Option<String> {
        let key = Sha256::digest(key_material);
        let pt = rc4(&key, ct);
        if Sha256::digest(&pt).as_slice() == sum {
            String::from_utf8(pt).ok()
        } else {
            None
        }
    };
    if let Some(mp) = &opts.xshell_master_password {
        if let Some(p) = try_key(mp.as_bytes()) {
            return Some(p);
        }
    }
    let mut accounts: Vec<&XshellAccount> = extra.iter().collect();
    accounts.extend(opts.xshell_accounts.iter());
    for acc in accounts {
        // 不同版本的密钥拼法不同；有 SHA-256 校验，逐个试不会误判。
        let name_variants = string_byte_variants(&acc.user);
        let mut materials: Vec<Vec<u8>> = vec![acc.sid.as_bytes().to_vec()];
        for name in &name_variants {
            // 5.2 之后、7.0：用户名 + SID
            let mut m = name.clone();
            m.extend_from_slice(acc.sid.as_bytes());
            materials.push(m);
        }
        // 7.x：反转(反转(用户名) + SID) = 反转(SID) + 用户名
        let rev_sid: String = acc.sid.chars().rev().collect();
        for name in string_byte_variants(&acc.user) {
            let mut m = rev_sid.as_bytes().to_vec();
            m.extend_from_slice(&name);
            materials.push(m);
        }
        for m in materials {
            if let Some(p) = try_key(&m) {
                return Some(p);
            }
        }
    }
    None
}

/// 字符串可能的字节形式：UTF-8，以及中文 Windows 的 GBK。
fn string_byte_variants(s: &str) -> Vec<Vec<u8>> {
    let mut v = vec![s.as_bytes().to_vec()];
    let (gbk, _, _) = encoding_rs::GBK.encode(s);
    if gbk.as_ref() != s.as_bytes() {
        v.push(gbk.into_owned());
    }
    v
}

fn parse_version(v: &str) -> (u32, u32) {
    let mut it = v.trim().split('.');
    let major = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
    let minor = it
        .next()
        .and_then(|x| x.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok())
        .unwrap_or(0);
    (major, minor)
}

/// Java `java.util.Random`（FinalShell 推 DES 密钥时用）。
struct JavaRandom {
    seed: i64,
}

impl JavaRandom {
    const MULT: i64 = 0x5DEECE66D;
    const MASK: i64 = (1i64 << 48) - 1;

    fn new(seed: i64) -> Self {
        Self { seed: (seed ^ Self::MULT) & Self::MASK }
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(Self::MULT).wrapping_add(0xB)) & Self::MASK;
        ((self.seed as u64) >> (48 - bits)) as i64 as i32
    }

    fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        if bound & bound.wrapping_neg() == bound {
            return ((bound as i64).wrapping_mul(self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    fn next_long(&mut self) -> i64 {
        ((self.next(32) as i64) << 32).wrapping_add(self.next(32) as i64)
    }
}

/// FinalShell 由密文前 8 字节推出 DES 密钥（来自公开的解密工具，见文档）。
fn finalshell_des_key(head: &[u8; 8]) -> Option<[u8; 8]> {
    let h = |i: usize| head[i] as i8 as i64;
    let n127 = JavaRandom::new(h(5)).next_int_bound(127);
    if n127 == 0 {
        return None;
    }
    let ks = 3680984568597093857i64 / n127 as i64;
    let mut random = JavaRandom::new(ks);
    let t = head[0] as i8;
    for _ in 0..t.max(0) {
        random.next_long();
    }
    let n = random.next_long();
    let mut r2 = JavaRandom::new(n);
    let ld = [h(4), r2.next_long(), h(7), h(3), r2.next_long(), h(1), random.next_long(), h(2)];
    let mut buf = Vec::with_capacity(64);
    for v in ld {
        buf.extend_from_slice(&v.to_be_bytes());
    }
    let digest = md5::compute(&buf);
    let mut key = [0u8; 8];
    key.copy_from_slice(&digest.0[..8]);
    Some(key)
}

/// 解 FinalShell 的 `password` 字段。空串返回 `Some("")`。
pub fn finalshell_decrypt_password(enc: &str) -> Option<String> {
    use des::cipher::{BlockDecrypt, KeyInit};
    if enc.trim().is_empty() {
        return Some(String::new());
    }
    let buf = b64(enc)?;
    if buf.len() < 16 || (buf.len() - 8) % 8 != 0 {
        return None;
    }
    let mut head = [0u8; 8];
    head.copy_from_slice(&buf[..8]);
    let key = finalshell_des_key(&head)?;
    let cipher = des::Des::new_from_slice(&key).ok()?;
    let mut data = buf[8..].to_vec();
    for chunk in data.chunks_mut(8) {
        cipher.decrypt_block(des::cipher::generic_array::GenericArray::from_mut_slice(chunk));
    }
    // PKCS5 去填充
    let pad = *data.last()? as usize;
    if pad == 0 || pad > 8 || pad > data.len() || !data[data.len() - pad..].iter().all(|&b| b as usize == pad) {
        return None;
    }
    data.truncate(data.len() - pad);
    String::from_utf8(data).ok()
}

// ---------------------------------------------------------------- 文本与 INI

/// 读会话文件文本：认 UTF-16（带或不带 BOM）、UTF-8，最后按 GBK。
fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode_without_bom_handling(rest).0.into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE.decode_without_bom_handling(rest).0.into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    // 没有 BOM 的 UTF-16LE：ASCII 字符后面跟 0
    if bytes.len() >= 4 && bytes.len() % 2 == 0 && bytes[1] == 0 && bytes[3] == 0 {
        return encoding_rs::UTF_16LE.decode_without_bom_handling(bytes).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode_without_bom_handling(bytes).0.into_owned(),
    }
}

/// 极简 INI：`[段]` + `键=值`；键名不区分大小写。
fn parse_ini(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_uppercase();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.entry(section.clone())
                .or_default()
                .insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    out
}

fn ini_get<'a>(ini: &'a HashMap<String, HashMap<String, String>>, section: &str, key: &str) -> &'a str {
    ini.get(&section.to_ascii_uppercase())
        .and_then(|s| s.get(&key.to_ascii_lowercase()))
        .map(String::as_str)
        .unwrap_or("")
}

// ---------------------------------------------------------------- Xshell

/// 解析一个 `.xsh` 的内容。`group` 为分组（子文件夹路径），`name` 为会话名（文件名去掉扩展名）。
fn xshell_candidate(
    name: &str,
    group: &str,
    bytes: &[u8],
    opts: &ForeignImportOptions,
    extra_accounts: &[XshellAccount],
) -> ForeignCandidate {
    let ini = parse_ini(&decode_text(bytes));
    let version = ini_get(&ini, "SessionInfo", "Version");
    let host = ini_get(&ini, "CONNECTION", "Host").to_string();
    let port = ini_get(&ini, "CONNECTION", "Port").parse::<u16>().unwrap_or(22);
    let protocol = ini_get(&ini, "CONNECTION", "Protocol");
    let username = ini_get(&ini, "CONNECTION:AUTHENTICATION", "UserName").to_string();
    let enc_password = ini_get(&ini, "CONNECTION:AUTHENTICATION", "Password");
    let user_key = ini_get(&ini, "CONNECTION:AUTHENTICATION", "UserKey");
    let method = ini_get(&ini, "CONNECTION:AUTHENTICATION", "Method");

    let mut c = ForeignCandidate {
        source: ForeignSource::Xshell,
        name: name.to_string(),
        group: if group.is_empty() { "Xshell".into() } else { group.to_string() },
        host: host.clone(),
        port,
        username,
        password: None,
        notes: Vec::new(),
        skip_reason: None,
        marker: format!("xshell:{}|{}|{}|{}", group, name, host, port),
    };
    if !protocol.is_empty() && !protocol.eq_ignore_ascii_case("SSH") {
        c.skip_reason = Some(format!("{protocol} 会话，MistTerm 只支持 SSH"));
        return c;
    }
    if host.trim().is_empty() {
        c.skip_reason = Some("没有主机地址".into());
        return c;
    }
    if !enc_password.is_empty() {
        match xshell_decrypt_password(version, enc_password, opts, extra_accounts) {
            Some(p) => c.password = Some(p),
            None => c.notes.push(NOTE_REENTER.into()),
        }
    } else if !user_key.is_empty() || method == "1" {
        c.notes.push(format!("原来用 Xshell 里的私钥「{user_key}」登录，导入后请在会话里选择私钥文件"));
    } else {
        c.notes.push(NOTE_NO_PASSWORD.into());
    }
    c
}

/// 解析 Xshell：可以是 `Sessions` 文件夹、单个 `.xsh`，或「导出」得到的 `.xts`。
pub fn parse_xshell_path(path: &Path, opts: &ForeignImportOptions) -> std::io::Result<ForeignParseResult> {
    let mut result = ForeignParseResult::default();
    let accounts = local_windows_accounts();
    if path.is_dir() {
        let mut files = Vec::new();
        collect_files(path, &mut files, &|p| has_ext(p, "xsh"), 0);
        files.sort();
        for f in files {
            let rel = f.strip_prefix(path).unwrap_or(&f);
            let group = rel
                .parent()
                .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
                .unwrap_or_default();
            let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match std::fs::read(&f) {
                Ok(bytes) => result.candidates.push(xshell_candidate(&name, &group, &bytes, opts, &accounts)),
                Err(e) => result.warnings.push(format!("{}：读不了（{e}）", f.display())),
            }
        }
    } else if has_ext(path, "xts") || has_ext(path, "zip") {
        parse_xts(path, opts, &accounts, &mut result)?;
    } else {
        let bytes = std::fs::read(path)?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        result.candidates.push(xshell_candidate(&name, "", &bytes, opts, &accounts));
    }
    finish_result(&mut result, ForeignSource::Xshell);
    Ok(result)
}

fn parse_xts(
    path: &Path,
    opts: &ForeignImportOptions,
    local_accounts: &[XshellAccount],
    result: &mut ForeignParseResult,
) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("不是有效的 .xts 文件：{e}")))?;
    let mut accounts: Vec<XshellAccount> = Vec::new();
    // xts.zcf 记着导出时的 Windows 账号名和 SID（RC4 + MD5 校验）。
    if let Ok(mut zcf) = zip.by_name("xts.zcf") {
        let mut bytes = Vec::new();
        zcf.read_to_end(&mut bytes)?;
        let ini = parse_ini(&decode_text(&bytes));
        let user = xts_decrypt_field(ini_get(&ini, "SessionInfo", "UN")).unwrap_or_default();
        let sid = xts_decrypt_field(ini_get(&ini, "SessionInfo", "SI")).unwrap_or_default();
        if !sid.is_empty() {
            accounts.push(XshellAccount { user, sid });
        }
    }
    accounts.extend(local_accounts.iter().cloned());
    for i in 0..zip.len() {
        let mut entry = match zip.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                result.warnings.push(format!("压缩包第 {} 项读不了：{e}", i + 1));
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let raw = entry.name_raw().to_vec();
        let full = match std::str::from_utf8(&raw) {
            Ok(s) => s.to_string(),
            Err(_) => encoding_rs::GBK.decode_without_bom_handling(&raw).0.into_owned(),
        }
        .replace('\\', "/");
        if !full.to_ascii_lowercase().ends_with(".xsh") {
            continue;
        }
        let mut parts: Vec<&str> = full.split('/').filter(|s| !s.is_empty()).collect();
        // 去掉开头的 Xshell/、Sessions/ 这类目录
        while parts.len() > 1 && (parts[0].eq_ignore_ascii_case("xshell") || parts[0].eq_ignore_ascii_case("sessions")) {
            parts.remove(0);
        }
        let file_name = parts.pop().unwrap_or_default();
        let name = file_name.rsplit_once('.').map(|(a, _)| a).unwrap_or(file_name).to_string();
        let group = parts.join("/");
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        result.candidates.push(xshell_candidate(&name, &group, &bytes, opts, &accounts));
    }
    Ok(())
}

/// Windows 上当前登录账号（名字 + SID），用于解本机 Xshell 5.1+ 保存的密码。
#[cfg(windows)]
fn local_windows_accounts() -> Vec<XshellAccount> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    // "DOMAIN\\user","S-1-5-21-..."
    let fields: Vec<String> = text.trim().split(',').map(|f| f.trim().trim_matches('"').to_string()).collect();
    if fields.len() < 2 || !fields[1].starts_with("S-1-") {
        return Vec::new();
    }
    let sid = fields[1].clone();
    let mut users: Vec<String> = Vec::new();
    if let Ok(u) = std::env::var("USERNAME") {
        users.push(u);
    }
    let from_whoami = fields[0].rsplit('\\').next().unwrap_or("").to_string();
    if !from_whoami.is_empty() && !users.iter().any(|u| u.eq_ignore_ascii_case(&from_whoami)) {
        users.push(from_whoami);
    }
    users.into_iter().map(|user| XshellAccount { user, sid: sid.clone() }).collect()
}

#[cfg(not(windows))]
fn local_windows_accounts() -> Vec<XshellAccount> {
    Vec::new()
}

// ---------------------------------------------------------------- FinalShell

/// FinalShell 会把历史版本放在 `backup/`、删掉的放在 `deleted/`，这些不能当成现有主机导入。
const FINALSHELL_IGNORED_DIRS: &[&str] = &["backup", "deleted", "temp", "blur_image_cache"];

/// 解析 FinalShell：可以是数据目录（里面有 `conn`）、`conn` 目录，或单个 `*_connect_config.json`。
pub fn parse_finalshell_path(path: &Path) -> std::io::Result<ForeignParseResult> {
    let mut result = ForeignParseResult::default();
    let mut files = Vec::new();
    if path.is_dir() {
        collect_files(
            path,
            &mut files,
            &|p| {
                let n = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                n == "folder.json" || n.ends_with("_connect_config.json")
            },
            0,
        );
        files.sort();
    } else {
        files.push(path.to_path_buf());
    }

    struct Folder {
        name: String,
        parent: String,
    }
    let mut folders: HashMap<String, Folder> = HashMap::new();
    let mut hosts: Vec<(PathBuf, serde_json::Map<String, serde_json::Value>)> = Vec::new();
    let mut seen_hosts = std::collections::HashSet::new();
    for f in files {
        let text = match std::fs::read(&f) {
            Ok(b) => decode_text(&b),
            Err(e) => {
                result.warnings.push(format!("{}：读不了（{e}）", f.display()));
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => {
                result.warnings.push(format!("{}：不是有效的 JSON，已跳过", f.display()));
                continue;
            }
        };
        let Some(obj) = value.as_object() else { continue };
        let id = json_str(obj, "id");
        if f.file_name().is_some_and(|n| n == "folder.json") {
            if !id.is_empty() {
                folders.insert(id.clone(), Folder { name: non_empty(json_str(obj, "name"), &id), parent: json_str(obj, "parent_id") });
            }
        } else if seen_hosts.insert(if id.is_empty() { f.display().to_string() } else { id }) {
            hosts.push((f.clone(), obj.clone()));
        }
    }

    let group_of = |parent: &str| -> String {
        let mut names = Vec::new();
        let mut cur = parent.to_string();
        let mut guard = 0;
        while !cur.is_empty() && cur != "root" && guard < 32 {
            match folders.get(&cur) {
                Some(f) => {
                    names.push(f.name.clone());
                    cur = f.parent.clone();
                }
                None => break,
            }
            guard += 1;
        }
        names.reverse();
        names.join("/")
    };

    let mut orphans = 0;
    for (f, obj) in hosts {
        let id = json_str(&obj, "id");
        let host = json_str(&obj, "host");
        let port = obj.get("port").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(22);
        let parent = json_str(&obj, "parent_id");
        let mut group = group_of(&parent);
        if group.is_empty() && !parent.is_empty() && parent != "root" {
            orphans += 1;
        }
        if group.is_empty() {
            group = "FinalShell".into();
        }
        let name = non_empty(json_str(&obj, "name"), &host);
        let mut c = ForeignCandidate {
            source: ForeignSource::FinalShell,
            name,
            group,
            host: host.clone(),
            port: u16::try_from(port).unwrap_or(22),
            username: json_str(&obj, "user_name"),
            password: None,
            notes: Vec::new(),
            skip_reason: None,
            marker: format!("finalshell:{}", if id.is_empty() { f.display().to_string() } else { id }),
        };
        let conn_type = obj.get("conection_type").and_then(|v| v.as_i64()).unwrap_or(100);
        if conn_type == 101 {
            c.skip_reason = Some("远程桌面会话，MistTerm 只支持 SSH".into());
        } else if conn_type != 100 {
            c.skip_reason = Some(format!("不认识的连接类型 {conn_type}，MistTerm 只支持 SSH"));
        } else if host.trim().is_empty() {
            c.skip_reason = Some("没有主机地址".into());
        }
        if c.skip_reason.is_none() {
            let auth = obj.get("authentication_type").and_then(|v| v.as_i64()).unwrap_or(1);
            let enc = json_str(&obj, "password");
            if auth == 2 {
                c.notes.push("原来用私钥登录，导入后请在会话里选择私钥文件".into());
            } else if enc.is_empty() {
                c.notes.push(NOTE_NO_PASSWORD.into());
            } else {
                match finalshell_decrypt_password(&enc) {
                    Some(p) if !p.is_empty() => c.password = Some(p),
                    _ => c.notes.push(NOTE_REENTER.into()),
                }
            }
            let proxy = json_str(&obj, "proxy_id");
            if !proxy.is_empty() && proxy != "0" {
                c.notes.push("原来配了代理，导入后需要自己重新设置".into());
            }
            let has_fwd = |k: &str| match obj.get(k) {
                Some(serde_json::Value::Array(a)) => !a.is_empty(),
                Some(serde_json::Value::Object(o)) => !o.is_empty(),
                _ => false,
            };
            if has_fwd("port_forwarding_list") || has_fwd("remote_port_forwarding") {
                c.notes.push("原来配了端口转发，导入后需要自己重新设置".into());
            }
        }
        result.candidates.push(c);
    }
    if orphans > 0 {
        result.warnings.push(format!(
            "{orphans} 台主机的分组信息不在所选位置，先放进「FinalShell」分组。想保留分组，请选择 FinalShell 的数据目录（里面有 conn 文件夹）。"
        ));
    }
    finish_result(&mut result, ForeignSource::FinalShell);
    Ok(result)
}

fn json_str(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> String {
    match obj.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn non_empty(s: String, fallback: &str) -> String {
    if s.trim().is_empty() {
        fallback.to_string()
    } else {
        s
    }
}

// ---------------------------------------------------------------- 公共

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>, want: &dyn Fn(&Path) -> bool, depth: usize) {
    if depth > 12 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            let n = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if FINALSHELL_IGNORED_DIRS.contains(&n.as_str()) {
                continue;
            }
            collect_files(&p, out, want, depth + 1);
        } else if ft.is_file() && want(&p) {
            out.push(p);
        }
    }
}

fn finish_result(result: &mut ForeignParseResult, source: ForeignSource) {
    if result.candidates.is_empty() {
        result.warnings.push(match source {
            ForeignSource::Xshell => "没有找到 Xshell 会话。请选择 Xshell 的 Sessions 文件夹，或用 Xshell「文件 → 导出」得到的 .xts 文件。".into(),
            ForeignSource::FinalShell => "没有找到 FinalShell 主机。请选择 FinalShell 的数据目录（里面有 conn 文件夹）。".into(),
        });
    }
    let unreadable = result.candidates.iter().filter(|c| c.importable() && c.notes.iter().any(|n| n == NOTE_REENTER)).count();
    if unreadable > 0 {
        result.warnings.push(match source {
            ForeignSource::Xshell => format!(
                "{unreadable} 个会话的密码读不出来（Xshell 5.1 起密码和原电脑的 Windows 账号绑定；设了主密码也读不出）。在原来那台 Windows 电脑上导入，或用 Xshell「文件 → 导出」的 .xts 文件，通常就能读出；否则导入后重新输入密码。"
            ),
            ForeignSource::FinalShell => format!("{unreadable} 台主机的密码读不出来，导入后请重新输入。"),
        });
    }
}

/// 自动判断是 Xshell 还是 FinalShell。
pub fn detect_source(path: &Path) -> Option<ForeignSource> {
    if path.is_file() {
        if has_ext(path, "xsh") || has_ext(path, "xts") {
            return Some(ForeignSource::Xshell);
        }
        let n = path.file_name()?.to_string_lossy().to_string();
        if n.ends_with("_connect_config.json") {
            return Some(ForeignSource::FinalShell);
        }
        return None;
    }
    let mut xsh = Vec::new();
    collect_files(path, &mut xsh, &|p| has_ext(p, "xsh"), 0);
    if !xsh.is_empty() {
        return Some(ForeignSource::Xshell);
    }
    let mut fs = Vec::new();
    collect_files(path, &mut fs, &|p| p.to_string_lossy().ends_with("_connect_config.json"), 0);
    if !fs.is_empty() {
        return Some(ForeignSource::FinalShell);
    }
    None
}

pub fn parse_path(source: ForeignSource, path: &Path, opts: &ForeignImportOptions) -> std::io::Result<ForeignParseResult> {
    match source {
        ForeignSource::Xshell => parse_xshell_path(path, opts),
        ForeignSource::FinalShell => parse_finalshell_path(path),
    }
}

/// 常见的默认位置（存在的才返回），用来给文件选择框一个起点。
pub fn default_locations(source: ForeignSource) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let home = crate::platform::home_dir();
    match source {
        ForeignSource::Xshell => {
            if let Some(docs) = dirs::document_dir() {
                for sub in ["NetSarang Computer/8/Xshell/Sessions", "NetSarang Computer/7/Xshell/Sessions", "NetSarang Computer/6/Xshell/Sessions", "NetSarang/Xshell/Sessions"] {
                    v.push(docs.join(sub));
                }
            }
        }
        ForeignSource::FinalShell => {
            if let Some(local) = dirs::data_local_dir() {
                v.push(local.join("finalshell"));
            }
            if let Some(h) = &home {
                v.push(h.join(".finalshell"));
                v.push(h.join("Library/FinalShell"));
            }
        }
    }
    v.retain(|p| p.is_dir());
    v
}

/// 是否已经导入过（按标记）。
pub fn is_already_imported(c: &ForeignCandidate, existing: &[SessionConfig]) -> bool {
    existing.iter().any(|s| s.ssh_config_marker.as_deref() == Some(c.marker.as_str()))
}

/// 转成 MistTerm 会话（名称去重）。
pub fn candidate_to_session(c: &ForeignCandidate, existing_names: &[String]) -> SessionConfig {
    let mut name = if c.name.trim().is_empty() { c.host.clone() } else { c.name.clone() };
    let base = name.clone();
    let mut n = 2;
    while existing_names.iter().any(|x| x == &name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    let mut cfg = SessionConfig::default();
    cfg.name = name;
    cfg.group = c.group.clone();
    cfg.host = c.host.trim().to_string();
    cfg.port = c.port;
    cfg.username = c.username.clone();
    cfg.password = c.password.clone().unwrap_or_default();
    cfg.ssh_config_marker = Some(c.marker.clone());
    cfg.created_at = Some(chrono::Utc::now().timestamp());
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: &str = "S-1-5-21-917267712-1342860078-1792151419-512";

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/foreign_import")
    }

    fn by_name<'a>(r: &'a ForeignParseResult, name: &str) -> &'a ForeignCandidate {
        r.candidates.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("missing {name}: {:?}", r.candidates))
    }

    fn acc(user: &str) -> ForeignImportOptions {
        ForeignImportOptions {
            xshell_accounts: vec![XshellAccount { user: user.into(), sid: SID.into() }],
            xshell_master_password: None,
        }
    }

    #[test]
    fn xshell_public_vectors() {
        // HyperSine/how-does-Xmanager-encrypt-password 文档里的公开向量（明文 "This is a test"）
        let none = ForeignImportOptions::default();
        assert_eq!(xshell_decrypt_password("5.0", "/6KaTrKwm0cmhr0yAWQ=", &none, &[]).as_deref(), Some("This is a test"));
        let o = acc("Administrator");
        assert_eq!(
            xshell_decrypt_password("5.1", "hIMxIyQ3HbJsVIdbbunHvh7ZAvuN1NSJl8ZFL11+UJ+82+KAixa89O3OTAfRTg==", &o, &[]).as_deref(),
            Some("This is a test")
        );
        assert_eq!(
            xshell_decrypt_password("6.0", "zv21O1x43qRs3c5NckDHvh7ZAvuN1NSJl8ZFL11+UJ+82+KAixa89O3OTAfRTg==", &o, &[]).as_deref(),
            Some("This is a test")
        );
        // 账号不对：读不出，而不是读出乱码
        assert_eq!(
            xshell_decrypt_password("6.0", "zv21O1x43qRs3c5NckDHvh7ZAvuN1NSJl8ZFL11+UJ+82+KAixa89O3OTAfRTg==", &acc("someone"), &[]),
            None
        );
        assert_eq!(xshell_decrypt_password("6.0", "zv21O1x43qRs3c5NckDHvh7ZAvuN1NSJl8ZFL11+UJ+82+KAixa89O3OTAfRTg==", &none, &[]), None);
        // 7.x 拼法（JDArmy/SharpXDecrypt），中文用户名
        assert_eq!(
            xshell_decrypt_password("7.1", "b+6VKaKORR/PMz2Z0A4PpINmIsH+N3NNFMsEIB/nrqOiC9qSYseM5N3X", &acc("张三"), &[]).as_deref(),
            Some("P@ss w0rd!")
        );
    }

    #[test]
    fn finalshell_public_vectors() {
        // final2halo / finalshell2all 测试里的公开向量
        assert_eq!(finalshell_decrypt_password("UU8hWV51DmVNgmX/pUd0LlaEo53VTa6s").as_deref(), Some("beac3d85988e"));
        assert_eq!(finalshell_decrypt_password("OGNqLj1Le11Br3AIelAiPaDJpfhBzmEN").as_deref(), Some("beac3d85988e"));
        assert_eq!(finalshell_decrypt_password("AQIDBAUGBwjPeQvDruVvdza4kcHpV3CH").as_deref(), Some("vector-password"));
        // EstamelGG/FinalShellPassMassDecode 仓库里的样例配置
        assert_eq!(finalshell_decrypt_password("PQopUUVtS2b5w3tdM8/g9GNKnrJM/isB").as_deref(), Some("admin@123"));
        assert_eq!(finalshell_decrypt_password("cHwlSxdqEVQ0tTLhQT3C7LTclBSh7c7b91qumrW6xcM=").as_deref(), Some("RootH89h2s*(SH2w9h"));
        assert_eq!(finalshell_decrypt_password("BnEMYARWV0/5LJjIfTCw6uQeVxFSxgULAhEi95TVgCo=").as_deref(), Some("admin@2023SHhxD!@#"));
        assert_eq!(finalshell_decrypt_password("").as_deref(), Some(""));
        assert_eq!(finalshell_decrypt_password("not-base64%"), None);
        assert_eq!(finalshell_decrypt_password("c2hvcnQ="), None);
    }

    #[test]
    fn java_random_matches_jdk() {
        assert_eq!(JavaRandom::new(0).next_int(), -1155484576);
        assert_eq!(JavaRandom::new(0).next_long(), -4962768465676381896);
    }

    #[test]
    fn xshell_sessions_folder() {
        let r = parse_xshell_path(&fixtures().join("xshell/Sessions"), &acc("tian")).unwrap();
        assert_eq!(r.candidates.len(), 6, "{:?}", r.candidates);

        let web = by_name(&r, "web-01");
        assert_eq!((web.group.as_str(), web.host.as_str(), web.port, web.username.as_str()), ("生产", "10.0.0.11", 22, "root"));
        assert_eq!(web.password.as_deref(), Some("mist-test-pw"));

        let db = by_name(&r, "db-01");
        assert_eq!((db.group.as_str(), db.port, db.username.as_str()), ("生产/数据库", 2222, "dba"));
        assert_eq!(db.password.as_deref(), Some("This is a test"));

        // 账号是 Administrator 的会话，用 tian 解不开：提示重新输入
        let t = by_name(&r, "测试机");
        assert_eq!(t.group, "Xshell");
        assert_eq!(t.password, None);
        assert!(t.notes.iter().any(|n| n.contains("重新输入")));

        assert!(by_name(&r, "telnet-box").skip_reason.as_deref().unwrap().contains("TELNET"));
        let key = by_name(&r, "key-login");
        assert!(key.importable() && key.password.is_none());
        assert!(key.notes.iter().any(|n| n.contains("私钥")));
        assert!(r.warnings.iter().any(|w| w.contains("读不出来")));

        // 主密码
        let mut o = acc("tian");
        o.xshell_master_password = Some("my-master".into());
        let r2 = parse_xshell_path(&fixtures().join("xshell/Sessions"), &o).unwrap();
        assert_eq!(by_name(&r2, "主密码").password.as_deref(), Some("mp-secret"));
    }

    #[test]
    fn xshell_xts_export_carries_account() {
        // 不给任何账号：.xts 里的 xts.zcf 自带导出时的账号，密码能读出
        let r = parse_xshell_path(&fixtures().join("xshell/export.xts"), &ForeignImportOptions::default()).unwrap();
        assert_eq!(r.candidates.len(), 2, "{:?}", r);
        let web = by_name(&r, "web-01");
        assert_eq!(web.group, "生产");
        assert_eq!(web.password.as_deref(), Some("mist-test-pw"));
        let jump = by_name(&r, "跳板机");
        assert_eq!(jump.group, "Xshell");
        assert_eq!(jump.password.as_deref(), Some("jump-pw"));
    }

    #[test]
    fn finalshell_data_dir() {
        let r = parse_finalshell_path(&fixtures().join("finalshell")).unwrap();
        let names: Vec<&str> = r.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"deleted-host") && !names.contains(&"backup-host"), "{names:?}");
        assert_eq!(r.candidates.len(), 4, "{names:?}");

        let web = by_name(&r, "web-01");
        assert_eq!((web.group.as_str(), web.host.as_str(), web.port, web.username.as_str()), ("生产", "10.0.0.11", 22, "root"));
        assert_eq!(web.password.as_deref(), Some("mist-test-pw"));
        assert_eq!(web.marker, "finalshell:h1");

        let db = by_name(&r, "db-01");
        assert_eq!((db.group.as_str(), db.port), ("生产/数据库", 2222));
        assert_eq!(db.password.as_deref(), Some("数据库密码"));
        assert!(db.notes.iter().any(|n| n.contains("代理")));

        let key = by_name(&r, "key-only");
        assert_eq!(key.group, "FinalShell");
        assert!(key.notes.iter().any(|n| n.contains("私钥")));

        assert!(by_name(&r, "win-desktop").skip_reason.as_deref().unwrap().contains("远程桌面"));

        // 只选 conn 下的子文件夹：分组信息不全时给出提醒
        let r2 = parse_finalshell_path(&fixtures().join("finalshell/conn/f1/f2")).unwrap();
        assert_eq!(by_name(&r2, "db-01").group, "数据库");
        // 单个文件
        let r3 = parse_finalshell_path(&fixtures().join("finalshell/conn/f1/h1_connect_config.json")).unwrap();
        assert_eq!(by_name(&r3, "web-01").group, "FinalShell");
        assert!(r3.warnings.iter().any(|w| w.contains("数据目录")));
    }

    #[test]
    fn detect_and_convert() {
        assert_eq!(detect_source(&fixtures().join("xshell/Sessions")), Some(ForeignSource::Xshell));
        assert_eq!(detect_source(&fixtures().join("xshell/export.xts")), Some(ForeignSource::Xshell));
        assert_eq!(detect_source(&fixtures().join("finalshell")), Some(ForeignSource::FinalShell));

        let r = parse_finalshell_path(&fixtures().join("finalshell")).unwrap();
        let web = by_name(&r, "web-01");
        let s = candidate_to_session(web, &["web-01".to_string()]);
        assert_eq!(s.name, "web-01 (2)");
        assert_eq!(s.group, "生产");
        assert_eq!((s.host.as_str(), s.port, s.username.as_str(), s.password.as_str()), ("10.0.0.11", 22, "root", "mist-test-pw"));
        assert!(is_already_imported(web, &[s]));
    }

    #[test]
    fn text_decoding() {
        let utf16: Vec<u8> = "[CONNECTION]\r\nHost=中文.example\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let mut with_bom = vec![0xFF, 0xFE];
        with_bom.extend_from_slice(&utf16);
        assert!(decode_text(&with_bom).contains("Host=中文.example"));
        assert!(decode_text(&utf16).contains("Host=中文.example"));
        let (gbk, _, _) = encoding_rs::GBK.encode("Host=生产库");
        assert_eq!(decode_text(&gbk), "Host=生产库");
    }
}
