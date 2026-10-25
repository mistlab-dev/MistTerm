//! 本机等宽字体枚举（macOS / Windows / Linux，尽力而为）。
//!
//! Linux / macOS 优先问 fontconfig（`fc-list :spacing=100`）。`fc-list` 最多等
//! [`FC_LIST_TIMEOUT`]；超时、失败或没有 fontconfig 时扫常见字体目录，读 name /
//! OS/2 / post 表判断等宽。结果按家族名去重，优先 Regular、非斜体、独立的 .ttf/.otf。
//!
//! 扫描在后台线程跑。界面用 [`system_mono_fonts_if_ready`]，不要在 UI 线程调用
//! [`list_system_mono_fonts`]（那条会等到扫完）。进程内只扫一次，新装字体要重启才出现。
//!
//! 序列化身份是家族名（见偏好里的 `terminal_font_family`），路径只在加载时解析。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

/// `fc-list` 卡住时放弃并改扫目录。界面不走这条阻塞路径。
const FC_LIST_TIMEOUT: Duration = Duration::from_secs(3);
/// name 表记录数上限，和 num_tables / TTC / name 字节上限同一思路。
const MAX_NAME_RECORDS: usize = 512;

/// 已安装的一款等宽字体。`index` 是字体集里的面索引（给 egui `FontData::index`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemMonoFont {
    pub family: String,
    pub path: PathBuf,
    pub index: u32,
}

/// 家族名比较：忽略首尾空白和 ASCII 大小写。`Cascadia Mono` 不等于 `Cascadia Mono PL`。
pub fn family_eq(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

static CACHE: OnceLock<Vec<SystemMonoFont>> = OnceLock::new();
static SCAN_STARTED: AtomicBool = AtomicBool::new(false);

/// 在后台开始枚举。已经在扫、或已经扫完，则直接返回。
pub fn start_system_mono_font_scan() {
    if CACHE.get().is_some() {
        return;
    }
    if SCAN_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("mono-font-scan".into())
        .spawn(|| {
            let _ = list_system_mono_fonts();
        })
        .is_ok();
    if !spawned {
        SCAN_STARTED.store(false, Ordering::Release);
    }
}

/// 枚举完成则返回缓存（可以是空列表）。否则 `None`，并确保后台扫描已启动。
pub fn system_mono_fonts_if_ready() -> Option<&'static [SystemMonoFont]> {
    start_system_mono_font_scan();
    CACHE.get().map(Vec::as_slice)
}

pub fn system_mono_fonts_ready() -> bool {
    CACHE.get().is_some()
}

/// 本机等宽家族，按显示名排序。进程内只扫一次。
///
/// 会阻塞到扫描结束（包括正在进行的后台扫描）。界面用 [`system_mono_fonts_if_ready`]。
pub fn list_system_mono_fonts() -> &'static [SystemMonoFont] {
    CACHE.get_or_init(scan_system_mono_fonts)
}

/// 按家族名找首选文件。找不到返回 `None`（调用方再回退系统默认）。
pub fn resolve_mono_family(family: &str) -> Option<&'static SystemMonoFont> {
    let family = family.trim();
    if family.is_empty() {
        return None;
    }
    list_system_mono_fonts()
        .iter()
        .find(|f| family_eq(&f.family, family))
}

fn scan_system_mono_fonts() -> Vec<SystemMonoFont> {
    let started = std::time::Instant::now();
    let fonts = try_fontconfig().unwrap_or_else(scan_font_directories);
    log::info!(
        "enumerated {} monospace families in {:?}",
        fonts.len(),
        started.elapsed()
    );
    fonts
}

fn try_fontconfig() -> Option<Vec<SystemMonoFont>> {
    let mut cmd = std::process::Command::new("fc-list");
    cmd.args([
        ":spacing=100",
        "-f",
        "%{file}\t%{family}\t%{style}\t%{weight}\t%{index}\n",
    ]);
    let output = match command_output_with_timeout(&mut cmd, FC_LIST_TIMEOUT) {
        Ok(output) => output,
        Err(CmdOutputError::TimedOut) => {
            log::warn!(
                "fc-list exceeded {:?}; scanning font directories",
                FC_LIST_TIMEOUT
            );
            return None;
        }
        Err(CmdOutputError::Unavailable) => return None,
    };
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if !text.contains('\t') {
        return None;
    }
    let fonts: Vec<_> = fonts_from_fc_list(&text)
        .into_iter()
        .filter(|f| f.path.is_file())
        .collect();
    if fonts.is_empty() {
        None
    } else {
        Some(fonts)
    }
}

#[derive(Debug)]
enum CmdOutputError {
    TimedOut,
    Unavailable,
}

/// 跑完一条命令并读 stdout。超过 `timeout` 就杀掉进程，避免 `Command::output` 一直等。
fn command_output_with_timeout(
    cmd: &mut std::process::Command,
    timeout: Duration,
) -> Result<std::process::Output, CmdOutputError> {
    use std::process::{Output, Stdio};
    use std::sync::mpsc;
    use std::time::Instant;

    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| CmdOutputError::Unavailable)?;
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            stop_child(&mut child);
            return Err(CmdOutputError::Unavailable);
        }
    };
    let (tx, rx) = mpsc::channel();
    if std::thread::Builder::new()
        .name("cmd-stdout".into())
        .spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            let _ = tx.send(buf);
        })
        .is_err()
    {
        stop_child(&mut child);
        return Err(CmdOutputError::Unavailable);
    }
    let started = Instant::now();
    let stdout_buf = loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(buf) => break buf,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if started.elapsed() >= timeout {
                    stop_child(&mut child);
                    return Err(CmdOutputError::TimedOut);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stop_child(&mut child);
                return Err(CmdOutputError::Unavailable);
            }
        }
    };
    // stdout 已读完但进程还活着时，也不能无限等 wait。
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                stop_child(&mut child);
                return Err(CmdOutputError::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                stop_child(&mut child);
                return Err(CmdOutputError::Unavailable);
            }
        }
    };
    Ok(Output {
        status,
        stdout: stdout_buf,
        stderr: Vec::new(),
    })
}

fn stop_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

struct Candidate {
    family: String,
    path: PathBuf,
    index: u32,
    rank: i32,
}

fn fonts_from_fc_list(stdout: &str) -> Vec<SystemMonoFont> {
    let mut best: HashMap<String, Candidate> = HashMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let Some(file) = parts.next() else { continue };
        let Some(family_field) = parts.next() else {
            continue;
        };
        let style = parts.next().unwrap_or("");
        let weight = parts.next().unwrap_or("80");
        let index = parts.next().unwrap_or("0");
        let family = primary_family(family_field);
        if reject_family(&family) || !is_scalable_font_path(Path::new(file)) {
            continue;
        }
        let css = fc_weight_to_css(parse_fc_weight(weight));
        let italic = style_is_italic(style);
        let rank = face_rank(
            css,
            italic,
            style_is_regular(style),
            is_static_file(Path::new(file)),
        );
        let cand = Candidate {
            family,
            path: PathBuf::from(file),
            index: parse_fc_face_index(index),
            rank,
        };
        consider_candidate(&mut best, cand);
    }
    finish(best)
}

fn primary_family(field: &str) -> String {
    field
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_string()
}

/// fontconfig 的 `%{index}` 低 16 位是面索引，高 16 位是可变字体实例。
/// egui 只接受面索引，实例位要丢掉（否则会指到不存在的面）。
fn parse_fc_face_index(raw: &str) -> u32 {
    raw.trim().parse::<u32>().unwrap_or(0) & 0xFFFF
}

fn parse_fc_weight(raw: &str) -> i32 {
    let s = raw.trim();
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let nums: Vec<i32> = inner
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        if nums.len() >= 2 {
            let (lo, hi) = (nums[0].min(nums[1]), nums[0].max(nums[1]));
            if (lo..=hi).contains(&80) {
                return 80;
            }
            return if (80 - lo).abs() <= (80 - hi).abs() {
                lo
            } else {
                hi
            };
        }
    }
    s.parse().unwrap_or(80)
}

/// fontconfig 字重（regular = 80，bold = 200）→ CSS 字重（regular = 400）。
fn fc_weight_to_css(w: i32) -> i32 {
    if w <= 20 {
        100
    } else if w <= 40 {
        200
    } else if w <= 55 {
        300
    } else if w <= 90 {
        400
    } else if w <= 140 {
        500
    } else if w <= 180 {
        600
    } else if w <= 210 {
        700
    } else {
        800
    }
}

fn style_is_italic(style: &str) -> bool {
    let s = style.to_lowercase();
    s.contains("italic") || s.contains("oblique") || s.contains("slanted")
}

fn style_is_regular(style: &str) -> bool {
    if style_is_italic(style) {
        return false;
    }
    style
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .any(|t| {
            matches!(
                t.to_ascii_lowercase().as_str(),
                "regular" | "normal" | "book" | "roman" | "upright"
            )
        })
}

fn face_rank(css_weight: i32, italic: bool, regular_style: bool, static_file: bool) -> i32 {
    let mut score = 0;
    if !italic {
        score += 20_000;
    }
    let w = if css_weight <= 0 { 400 } else { css_weight };
    score -= (w - 400).abs().min(2_000);
    if regular_style && !italic {
        score += 100;
    }
    if static_file {
        score += 10;
    }
    score
}

fn is_static_file(path: &Path) -> bool {
    matches!(extension_lower(path).as_deref(), Some("ttf") | Some("otf"))
}

fn is_scalable_font_path(path: &Path) -> bool {
    matches!(
        extension_lower(path).as_deref(),
        Some("ttf") | Some("otf") | Some("ttc") | Some("otc")
    )
}

fn extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

fn reject_family(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 80 || n.starts_with('.') {
        return true;
    }
    let lower = n.to_lowercase();
    lower.contains("emoji")
        || lower.contains("lastresort")
        || lower.contains("last resort")
        || lower.contains("webdings")
        || lower.contains("wingdings")
        || lower.contains("zapf ding")
        || lower == "symbol"
}

fn consider_candidate(best: &mut HashMap<String, Candidate>, cand: Candidate) {
    let key = cand.family.to_lowercase();
    match best.get(&key) {
        Some(old) if cand.rank > old.rank || (cand.rank == old.rank && cand.path < old.path) => {
            best.insert(key, cand);
        }
        Some(_) => {}
        None => {
            best.insert(key, cand);
        }
    }
}

fn finish(best: HashMap<String, Candidate>) -> Vec<SystemMonoFont> {
    let mut fonts: Vec<_> = best
        .into_values()
        .map(|c| SystemMonoFont {
            family: c.family,
            path: c.path,
            index: c.index,
        })
        .collect();
    fonts.sort_by(|a, b| {
        a.family
            .to_lowercase()
            .cmp(&b.family.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    fonts
}

fn scan_font_directories() -> Vec<SystemMonoFont> {
    let mut best = HashMap::new();
    let mut seen = 0u32;
    for dir in font_directories() {
        walk_fonts(&dir, 0, &mut seen, &mut best);
        if seen > 4_000 {
            log::warn!("stopped monospace scan after {seen} font files");
            break;
        }
    }
    finish(best)
}

fn font_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
        dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.extend(
            [
                "/System/Library/Fonts",
                "/System/Library/Fonts/Supplemental",
                "/Library/Fonts",
                "/Network/Library/Fonts",
            ]
            .into_iter()
            .map(PathBuf::from),
        );
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs.extend(
            ["/usr/share/fonts", "/usr/local/share/fonts"]
                .into_iter()
                .map(PathBuf::from),
        );
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            dirs.push(PathBuf::from(xdg).join("fonts"));
        }
        if let Ok(dirs_env) = std::env::var("XDG_DATA_DIRS") {
            for d in dirs_env.split(':') {
                if !d.is_empty() {
                    dirs.push(PathBuf::from(d).join("fonts"));
                }
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let home = PathBuf::from(home);
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }
    dirs
}

fn walk_fonts(dir: &Path, depth: u8, seen: &mut u32, best: &mut HashMap<String, Candidate>) {
    if depth > 6 || *seen > 4_000 {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for ent in rd.flatten() {
        if *seen > 4_000 {
            return;
        }
        let path = ent.path();
        let ft = match ent.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() && !ft.is_symlink() {
            walk_fonts(&path, depth + 1, seen, best);
        } else if is_scalable_font_path(&path) {
            *seen += 1;
            for face in inspect_font_file(&path) {
                if !face.monospace || reject_family(&face.family) {
                    continue;
                }
                let italic = face.italic || style_is_italic(&face.subfamily);
                let rank = face_rank(
                    i32::from(face.weight),
                    italic,
                    style_is_regular(&face.subfamily),
                    is_static_file(&path),
                );
                consider_candidate(
                    best,
                    Candidate {
                        family: face.family,
                        path: path.clone(),
                        index: face.index,
                        rank,
                    },
                );
            }
        }
    }
}

#[derive(Debug)]
struct ParsedFace {
    family: String,
    subfamily: String,
    weight: u16,
    italic: bool,
    monospace: bool,
    index: u32,
}

fn inspect_font_file(path: &Path) -> Vec<ParsedFace> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut head = [0u8; 12];
    if file.read_exact(&mut head).is_err() {
        return Vec::new();
    }
    if &head[0..4] == b"ttcf" {
        return inspect_collection(&mut file);
    }
    if !is_sfnt_version(&head[0..4]) {
        return Vec::new();
    }
    inspect_sfnt_at(&mut file, 0, 0).into_iter().collect()
}

fn inspect_collection(file: &mut File) -> Vec<ParsedFace> {
    let n = read_u32_at(file, 8).unwrap_or(0).min(32);
    let mut out = Vec::new();
    for i in 0..n {
        let Some(off) = read_u32_at(file, 12 + u64::from(i) * 4) else {
            continue;
        };
        if let Some(face) = inspect_sfnt_at(file, u64::from(off), i) {
            out.push(face);
        }
    }
    out
}

fn inspect_sfnt_at(file: &mut File, origin: u64, index: u32) -> Option<ParsedFace> {
    let num_tables = read_u16_at(file, origin + 4)?;
    if num_tables == 0 || num_tables > 128 {
        return None;
    }
    let dir_len = usize::from(num_tables) * 16;
    let mut dir = vec![0u8; dir_len];
    read_exact_at(file, origin + 12, &mut dir)?;
    let mut name: Option<(u64, u32)> = None;
    let mut os2: Option<(u64, u32)> = None;
    let mut post: Option<(u64, u32)> = None;
    for rec in dir.chunks_exact(16) {
        let tag = &rec[0..4];
        let off = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]);
        let len = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]);
        let slot = match tag {
            b"name" => &mut name,
            b"OS/2" => &mut os2,
            b"post" => &mut post,
            _ => continue,
        };
        *slot = Some((u64::from(off), len));
    }
    let (name_off, name_len) = name?;
    let name_cap = (name_len as usize).min(256 * 1024);
    let mut name_buf = vec![0u8; name_cap];
    read_exact_at(file, name_off, &mut name_buf)?;
    let names = parse_name_table(&name_buf)?;
    let family = names.family?;
    let subfamily = names.subfamily.unwrap_or_default();

    let mut weight = 400u16;
    let mut italic = style_is_italic(&subfamily);
    let mut panose_mono = false;
    if let Some((off, len)) = os2 {
        let n = (len as usize).clamp(0, 128);
        if n >= 8 {
            let mut buf = vec![0u8; n];
            if read_exact_at(file, off, &mut buf).is_some() {
                weight = u16::from_be_bytes([buf[4], buf[5]]);
                if weight == 0 || weight > 1000 {
                    weight = 400;
                }
                if buf.len() >= 42 && (buf[32] == 2 || buf[32] == 0) && buf[35] == 9 {
                    panose_mono = true;
                }
                if buf.len() >= 64 {
                    let fs = u16::from_be_bytes([buf[62], buf[63]]);
                    if fs & 1 != 0 {
                        italic = true;
                    }
                }
            }
        }
    }

    let mut fixed_pitch = false;
    if let Some((off, len)) = post {
        if len >= 16 {
            let mut buf = [0u8; 16];
            if read_exact_at(file, off, &mut buf).is_some() {
                let flag = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
                fixed_pitch = flag != 0;
            }
        }
    }

    Some(ParsedFace {
        family,
        subfamily,
        weight,
        italic,
        monospace: fixed_pitch || panose_mono,
        index,
    })
}

struct NamePick {
    family: Option<String>,
    subfamily: Option<String>,
}

fn name_record_count(data: &[u8]) -> usize {
    let count = u16::from_be_bytes([data[2], data[3]]) as usize;
    count.min(MAX_NAME_RECORDS)
}

fn parse_name_table(data: &[u8]) -> Option<NamePick> {
    if data.len() < 6 {
        return None;
    }
    let count = name_record_count(data);
    let string_offset = u16::from_be_bytes([data[4], data[5]]) as usize;
    if data.len() < 6 + count * 12 {
        return None;
    }
    let mut best_family: Option<(u8, String)> = None;
    let mut best_typo: Option<(u8, String)> = None;
    let mut best_sub: Option<(u8, String)> = None;
    let mut best_typo_sub: Option<(u8, String)> = None;
    for i in 0..count {
        let rec = &data[6 + i * 12..6 + (i + 1) * 12];
        let platform = u16::from_be_bytes([rec[0], rec[1]]);
        let encoding = u16::from_be_bytes([rec[2], rec[3]]);
        let language = u16::from_be_bytes([rec[4], rec[5]]);
        let name_id = u16::from_be_bytes([rec[6], rec[7]]);
        let length = u16::from_be_bytes([rec[8], rec[9]]) as usize;
        let offset = u16::from_be_bytes([rec[10], rec[11]]) as usize;
        if !matches!(name_id, 1 | 2 | 16 | 17) {
            continue;
        }
        let start = string_offset.checked_add(offset)?;
        let end = start.checked_add(length)?;
        let bytes = data.get(start..end)?;
        let Some(text) = decode_name(platform, encoding, bytes) else {
            continue;
        };
        let rank = name_lang_rank(platform, language);
        let slot = match name_id {
            16 => &mut best_typo,
            1 => &mut best_family,
            17 => &mut best_typo_sub,
            _ => &mut best_sub,
        };
        if slot.as_ref().map(|(r, _)| rank < *r).unwrap_or(true) {
            *slot = Some((rank, text));
        }
    }
    let family = best_typo.or(best_family).map(|(_, s)| s);
    let subfamily = best_typo_sub.or(best_sub).map(|(_, s)| s);
    Some(NamePick { family, subfamily })
}

fn name_lang_rank(platform: u16, language: u16) -> u8 {
    match (platform, language) {
        (3, 0x0409) => 0,
        (3, 0) => 1,
        (0, 0 | 0x0409) => 1,
        (1, 0) => 2,
        (3, _) => 3,
        _ => 4,
    }
}

fn decode_name(platform: u16, encoding: u16, bytes: &[u8]) -> Option<String> {
    let text = match platform {
        0 | 3 => decode_utf16_be(bytes)?,
        1 if encoding == 0 => bytes
            .iter()
            .map(|&b| if b < 128 { b as char } else { '\u{FFFD}' })
            .collect::<String>(),
        _ => return None,
    };
    let text = text.trim().trim_matches('\0').trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn decode_utf16_be(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

fn is_sfnt_version(tag: &[u8]) -> bool {
    matches!(tag, b"\x00\x01\x00\x00" | b"OTTO" | b"true" | b"typ1")
}

fn read_exact_at(file: &mut File, offset: u64, buf: &mut [u8]) -> Option<()> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    file.read_exact(buf).ok()
}

fn read_u16_at(file: &mut File, offset: u64) -> Option<u16> {
    let mut buf = [0u8; 2];
    read_exact_at(file, offset, &mut buf)?;
    Some(u16::from_be_bytes(buf))
}

fn read_u32_at(file: &mut File, offset: u64) -> Option<u32> {
    let mut buf = [0u8; 4];
    read_exact_at(file, offset, &mut buf)?;
    Some(u32::from_be_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_eq_is_case_insensitive_but_not_a_prefix() {
        assert!(family_eq("JetBrains Mono", "jetbrains mono"));
        assert!(!family_eq("Cascadia Mono", "Cascadia Mono PL"));
        assert!(!family_eq("Cascadia Mono", "Cascadia Code"));
    }

    #[test]
    fn fc_face_index_drops_variable_instance_bits() {
        assert_eq!(parse_fc_face_index("0"), 0);
        assert_eq!(parse_fc_face_index("196608"), 0);
        assert_eq!(parse_fc_face_index("131073"), 1);
    }

    #[test]
    fn fc_weight_range_that_includes_regular_scores_as_regular() {
        assert_eq!(parse_fc_weight("[40 200]"), 80);
        assert_eq!(parse_fc_weight("200"), 200);
        assert_eq!(fc_weight_to_css(80), 400);
        assert_eq!(fc_weight_to_css(200), 700);
    }

    #[test]
    fn fc_list_prefers_regular_file_over_bold_and_italic() {
        let sample = "\
/usr/share/fonts/JetBrainsMono-Bold.ttf\tJetBrains Mono\tBold\t200\t0
/usr/share/fonts/JetBrainsMono-Italic.ttf\tJetBrains Mono\tItalic\t80\t0
/usr/share/fonts/macos/JetBrainsMono-Regular.ttf\tJetBrains Mono\tRegular\t80\t0
/usr/share/fonts/JetBrainsMono-Regular.ttf\tJetBrains Mono,JetBrains Mono\tRegular\t80\t0
/usr/share/fonts/DejaVuSansMono-Oblique.ttf\tDejaVu Sans Mono\tOblique\t80\t0
/usr/share/fonts/DejaVuSansMono.ttf\tDejaVu Sans Mono\tBook\t80\t0
/usr/share/fonts/DejaVuSansMono-Bold.ttf\tDejaVu Sans Mono\tBold\t200\t0
/usr/share/fonts/CascadiaMono.ttf\tCascadia Mono\t\t[40 200]\t0
/usr/share/fonts/CascadiaMono.ttf\tCascadia Mono\tBold\t200\t393216
/usr/share/fonts/CascadiaMono.ttf\tCascadia Mono\tRegular\t80\t262144
/usr/share/fonts/CascadiaMonoPL.ttf\tCascadia Mono PL\tRegular\t80\t0
/usr/share/fonts/Emoji.ttf\tNoto Color Emoji\tRegular\t80\t0
/usr/share/fonts/bitmap.pcf\tFixed\tRegular\t80\t0
";
        let fonts = fonts_from_fc_list(sample);
        let jb = fonts.iter().find(|f| f.family == "JetBrains Mono").unwrap();
        assert_eq!(
            jb.path,
            PathBuf::from("/usr/share/fonts/JetBrainsMono-Regular.ttf")
        );
        assert_eq!(jb.index, 0);
        let dejavu = fonts
            .iter()
            .find(|f| f.family == "DejaVu Sans Mono")
            .unwrap();
        assert_eq!(
            dejavu.path,
            PathBuf::from("/usr/share/fonts/DejaVuSansMono.ttf")
        );
        let cascadia = fonts.iter().find(|f| f.family == "Cascadia Mono").unwrap();
        assert_eq!(
            cascadia.path,
            PathBuf::from("/usr/share/fonts/CascadiaMono.ttf")
        );
        assert_eq!(
            cascadia.index, 0,
            "variable instance bits must not become the face index"
        );
        assert!(fonts.iter().any(|f| f.family == "Cascadia Mono PL"));
        assert!(fonts.iter().all(|f| f.family != "Noto Color Emoji"));
        assert!(fonts.iter().all(|f| f.path.extension().unwrap() != "pcf"));
    }

    #[test]
    fn synthetic_mono_face_is_detected_and_proportional_is_not() {
        let mono = build_sfnt("Test Mono", "Regular", true, 400);
        let prop = build_sfnt("Test Sans", "Regular", false, 400);
        let dir = tempfile::tempdir().unwrap();
        let mono_path = dir.path().join("TestMono-Regular.ttf");
        let prop_path = dir.path().join("TestSans-Regular.ttf");
        std::fs::write(&mono_path, &mono).unwrap();
        std::fs::write(&prop_path, &prop).unwrap();

        let mono_faces = inspect_font_file(&mono_path);
        assert_eq!(mono_faces.len(), 1);
        assert!(mono_faces[0].monospace);
        assert_eq!(mono_faces[0].family, "Test Mono");
        assert_eq!(mono_faces[0].weight, 400);

        let prop_faces = inspect_font_file(&prop_path);
        assert_eq!(prop_faces.len(), 1);
        assert!(!prop_faces[0].monospace);
    }

    #[test]
    fn dejavu_files_match_monospace_bit_when_present() {
        let mono = Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf");
        let sans = Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
        if !mono.is_file() || !sans.is_file() {
            return;
        }
        let mono_faces = inspect_font_file(mono);
        assert!(
            mono_faces
                .iter()
                .any(|f| f.monospace && f.family.contains("Mono")),
            "{mono_faces:?}"
        );
        let sans_faces = inspect_font_file(sans);
        assert!(sans_faces.iter().all(|f| !f.monospace), "{sans_faces:?}");
    }

    #[test]
    fn live_scan_prefers_dejavu_book_when_fontconfig_lists_it() {
        let marker = Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf");
        if !marker.is_file() {
            return;
        }
        let fonts = list_system_mono_fonts();
        let dejavu = fonts.iter().find(|f| f.family == "DejaVu Sans Mono");
        let Some(dejavu) = dejavu else {
            return;
        };
        let name = dejavu
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase();
        assert!(
            name.contains("dejavusansmono") && !name.contains("bold") && !name.contains("oblique"),
            "{name}"
        );
    }

    #[test]
    fn system_mono_fonts_if_ready_does_not_block() {
        let started = std::time::Instant::now();
        let _ = system_mono_fonts_if_ready();
        assert!(
            started.elapsed() < Duration::from_millis(800),
            "enumeration blocked the caller for {:?}",
            started.elapsed()
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !system_mono_fonts_ready() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(system_mono_fonts_ready());
    }

    #[test]
    fn command_output_returns_stdout_before_the_timeout() {
        let mut cmd = echo_command("mistterm-fc");
        let output = command_output_with_timeout(&mut cmd, Duration::from_secs(3)).unwrap();
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("mistterm-fc"), "{text}");
    }

    #[test]
    fn command_output_kills_a_hung_process() {
        let mut cmd = hung_command();
        let started = std::time::Instant::now();
        let err = command_output_with_timeout(&mut cmd, Duration::from_millis(400)).unwrap_err();
        assert!(matches!(err, CmdOutputError::TimedOut));
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "hung command was not killed: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn name_table_count_is_capped_at_512() {
        assert_eq!(name_record_count(&count_header(0)), 0);
        assert_eq!(name_record_count(&count_header(3)), 3);
        assert_eq!(name_record_count(&count_header(512)), 512);
        assert_eq!(name_record_count(&count_header(4000)), 512);
        assert_eq!(name_record_count(&count_header(u16::MAX)), 512);

        let early = name_table_with_family_at(0, 3, "Early Mono");
        assert_eq!(
            parse_name_table(&early).unwrap().family.as_deref(),
            Some("Early Mono")
        );
        let late = name_table_with_family_at(512, 513, "Late Mono");
        assert!(parse_name_table(&late).unwrap().family.is_none());
    }

    fn count_header(count: u16) -> [u8; 6] {
        let mut data = [0u8; 6];
        data[2..4].copy_from_slice(&count.to_be_bytes());
        data
    }

    fn name_table_with_family_at(index: usize, total: usize, family: &str) -> Vec<u8> {
        let fam = utf16_be(family);
        let string_offset = 6 + total * 12;
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&(total as u16).to_be_bytes());
        out.extend_from_slice(&(string_offset as u16).to_be_bytes());
        let mut pool = Vec::new();
        for i in 0..total {
            let (name_id, bytes): (u16, &[u8]) = if i == index { (1, &fam) } else { (0, &[]) };
            let offset = pool.len() as u16;
            out.extend_from_slice(&3u16.to_be_bytes());
            out.extend_from_slice(&1u16.to_be_bytes());
            out.extend_from_slice(&0x0409u16.to_be_bytes());
            out.extend_from_slice(&name_id.to_be_bytes());
            out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            out.extend_from_slice(&offset.to_be_bytes());
            pool.extend_from_slice(bytes);
        }
        out.extend_from_slice(&pool);
        out
    }

    fn echo_command(text: &str) -> std::process::Command {
        #[cfg(unix)]
        {
            let mut cmd = std::process::Command::new("echo");
            cmd.arg(text);
            cmd
        }
        #[cfg(windows)]
        {
            let mut cmd = std::process::Command::new("cmd");
            cmd.args(["/C", "echo", text]);
            cmd
        }
    }

    fn hung_command() -> std::process::Command {
        #[cfg(unix)]
        {
            let mut cmd = std::process::Command::new("sleep");
            cmd.arg("30");
            cmd
        }
        #[cfg(windows)]
        {
            let mut cmd = std::process::Command::new("ping");
            cmd.args(["-n", "30", "127.0.0.1"]);
            cmd
        }
    }

    fn build_sfnt(family: &str, subfamily: &str, mono: bool, weight: u16) -> Vec<u8> {
        let name = name_table(family, subfamily);
        let os2 = os2_table(weight, mono);
        let post = post_table(mono);
        pack_sfnt(&[(b"OS/2", os2), (b"name", name), (b"post", post)])
    }

    fn pack_sfnt(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let n = tables.len() as u16;
        let mut out = Vec::new();
        out.extend_from_slice(&0x00010000u32.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        let dir_at = out.len();
        out.resize(dir_at + tables.len() * 16, 0);
        for (i, (tag, data)) in tables.iter().enumerate() {
            while out.len() % 4 != 0 {
                out.push(0);
            }
            let offset = out.len() as u32;
            let rec = dir_at + i * 16;
            out[rec..rec + 4].copy_from_slice((*tag).as_slice());
            out[rec + 8..rec + 12].copy_from_slice(&offset.to_be_bytes());
            out[rec + 12..rec + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
        }
        out
    }

    fn name_table(family: &str, subfamily: &str) -> Vec<u8> {
        let fam = utf16_be(family);
        let sub = utf16_be(subfamily);
        let records = [(1u16, fam.clone()), (2u16, sub), (16u16, fam)];
        let count = records.len() as u16;
        let string_offset = 6 + records.len() * 12;
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&(string_offset as u16).to_be_bytes());
        let mut pool = Vec::new();
        for (name_id, bytes) in records {
            let offset = pool.len() as u16;
            out.extend_from_slice(&3u16.to_be_bytes());
            out.extend_from_slice(&1u16.to_be_bytes());
            out.extend_from_slice(&0x0409u16.to_be_bytes());
            out.extend_from_slice(&name_id.to_be_bytes());
            out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            out.extend_from_slice(&offset.to_be_bytes());
            pool.extend_from_slice(&bytes);
        }
        out.extend_from_slice(&pool);
        out
    }

    fn utf16_be(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
    }

    fn os2_table(weight: u16, mono: bool) -> Vec<u8> {
        let mut buf = vec![0u8; 80];
        buf[4..6].copy_from_slice(&weight.to_be_bytes());
        buf[32] = 2;
        buf[35] = if mono { 9 } else { 3 };
        if weight == 400 {
            buf[62] = 0;
            buf[63] = 0x40;
        }
        buf
    }

    fn post_table(mono: bool) -> Vec<u8> {
        let mut buf = vec![0u8; 16];
        buf[0..4].copy_from_slice(&0x00030000u32.to_be_bytes());
        if mono {
            buf[12..16].copy_from_slice(&1u32.to_be_bytes());
        }
        buf
    }
}
