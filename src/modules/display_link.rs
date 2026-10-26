//! 终端输出可点击文本（display-link）：路径 / 错误码 / IP / URL。
//!
//! 规则仍是 `assets/display-link-patterns.json`。扫描只经 [`MistModule::on_output_line`]，
//! 且只认 [`LineOrigin::Viewport`]：视口文本已经过 VTE，字节区间才能对上格子。
//! 原始 PTY 行（[`LineOrigin::Pty`]）不产出装饰，避免和 output-trigger 的「一行一次」副作用搅在一起。
//! 右键「新开连接」经 [`MistModule::contribute_context_menu`] 贡献，Host 持有菜单生命周期。
//!
//! 规则解析失败或文件缺失 = 引擎为空，输出渲染零影响。

use serde::Deserialize;

use crate::capability::{
    LineOrigin, MenuCtx, MenuItem, MistModule, ModuleAction, ModuleId, OutputLineCtx,
};

/// 稳定模块 id（设置项 / `MISTTERM_DISABLE_MODULES`）。
pub const MODULE_ID: ModuleId = "display-link";

/// 与 JSON `action` 字段一致，Host 按这些字符串分发点击。
pub const ACTION_SFTP_OPEN: &str = "sftp-open";
pub const ACTION_RUNBOOK_RECALL: &str = "runbook-recall";
pub const ACTION_COPY_OR_CONNECT: &str = "copy-or-connect";
pub const ACTION_BROWSER_OPEN: &str = "browser-open";

/// 右键「新开连接到此 IP」。只在命中 IP（`copy-or-connect`）时贡献。
pub const ACTION_NEW_CONNECTION: &str = "display-link.new-connection";


/// 系统二进制目录前缀（#140）：`ps`/CMD 列里的这些路径是「程序本身」，
/// 不是用户可操作的文件/目录，画链接是噪声。
/// 只拦「整个路径落在系统目录下」的绝对路径；`/etc` `/var` `/opt` 下的配置/数据不拦。
const SYSTEM_BIN_PREFIXES: [&str; 8] = [
    "/usr/bin/", "/usr/sbin/", "/usr/local/bin/", "/usr/local/sbin/",
    "/bin/", "/sbin/", "/lib/", "/lib64/",
];

/// `text` 是否为系统二进制路径（或恰为目录名本身，如 `/usr/sbin`）。
/// 仅作用于 path 类命中；相对路径（`./` `~/`）不受影响。
fn is_system_binary_path(text: &str) -> bool {
    if !text.starts_with('/') {
        return false;
    }
    if SYSTEM_BIN_PREFIXES.iter().any(|p| text.starts_with(p)) {
        return true;
    }
    // 目录名本身（无尾随段，如 `/usr/sbin`、`/bin`）也不链。
    matches!(
        text,
        "/usr/bin" | "/usr/sbin" | "/usr/local/bin" | "/usr/local/sbin" | "/bin" | "/sbin"
            | "/lib" | "/lib64"
    )
}

/// 从输出行中识别出的可点击片段。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayLink {
    start: usize,
    end: usize,
    rule_id: String,
    action: DisplayLinkAction,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayLinkAction {
    SftpOpen,
    RunbookRecall,
    Copy,
    BrowserOpen,
}

impl DisplayLinkAction {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            ACTION_SFTP_OPEN => Some(Self::SftpOpen),
            ACTION_RUNBOOK_RECALL => Some(Self::RunbookRecall),
            ACTION_COPY_OR_CONNECT => Some(Self::Copy),
            ACTION_BROWSER_OPEN => Some(Self::BrowserOpen),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SftpOpen => ACTION_SFTP_OPEN,
            Self::RunbookRecall => ACTION_RUNBOOK_RECALL,
            Self::Copy => ACTION_COPY_OR_CONNECT,
            Self::BrowserOpen => ACTION_BROWSER_OPEN,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PatternFile {
    #[serde(default)]
    #[allow(dead_code)]
    version: String,
    patterns: Vec<RawPattern>,
}

#[derive(Debug, Deserialize)]
struct RawPattern {
    id: String,
    pattern: String,
    /// path / errno / ip / url
    #[serde(default)]
    #[allow(dead_code)]
    category: String,
    action: String,
    /// 同一行内多个命中时的优先级：小者先占区间（URL > errno > ip > path）
    #[serde(default)]
    priority: u8,
}

struct CompiledPattern {
    id: String,
    action: DisplayLinkAction,
    /// path / errno / ip / url（#140：path 类命中做系统路径过滤）
    category: String,
    priority: u8,
    regex: regex::Regex,
}

/// 规则引擎：加载一次，之后只做匹配。
/// 上限保护：单行最多返回 16 个链接，防止病态输出拖垮渲染。
struct DisplayLinkEngine {
    patterns: Vec<CompiledPattern>,
    max_hits_per_line: usize,
}

impl DisplayLinkEngine {
    fn new() -> Self {
        const JSON: &str = include_str!("../../assets/display-link-patterns.json");
        Self::from_json(JSON)
    }

    /// 解析失败返回空引擎（功能关闭）。
    fn from_json(json: &str) -> Self {
        let (patterns, max_hits_per_line) = Self::parse(json).unwrap_or_else(|| (Vec::new(), 16));
        Self {
            patterns,
            max_hits_per_line,
        }
    }

    fn parse(json: &str) -> Option<(Vec<CompiledPattern>, usize)> {
        let file: PatternFile = serde_json::from_str(json).ok()?;
        let mut patterns: Vec<CompiledPattern> = file
            .patterns
            .into_iter()
            .filter_map(|p| {
                let action = DisplayLinkAction::from_str(&p.action)?;
                let regex = regex::Regex::new(&p.pattern).ok()?;
                Some(CompiledPattern {
                    id: p.id,
                    action,
                    category: p.category,
                    priority: p.priority,
                    regex,
                })
            })
            .collect();
        patterns.sort_by_key(|p| p.priority);
        Some((patterns, 16))
    }

    fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// 对一行输出做匹配，返回互不重叠、按出现位置排序的链接。
    ///
    /// 区间裁决：同一区间被多条规则命中时，取 priority 最小（最优先）的；
    /// 已占用区间不再接受新链接（先到先得，按 priority 顺序扫描）。
    /// 系统二进制路径（`/usr/sbin/sshd` 等）不产链接（#140）。
    fn scan(&self, line: &str) -> Vec<DisplayLink> {
        if self.patterns.is_empty() || line.is_empty() || line.len() > 8192 {
            return Vec::new();
        }
        let mut occupied: Vec<(usize, usize)> = Vec::new();
        let mut hits: Vec<DisplayLink> = Vec::new();
        for p in &self.patterns {
            for m in p.regex.captures_iter(line) {
                let (start, end, text) = match m.get(1).or_else(|| m.get(0)) {
                    Some(mm) => (mm.start(), mm.end(), mm.as_str().to_string()),
                    None => continue,
                };
                if end <= start {
                    continue;
                }
                if p.category == "path" && is_system_binary_path(&text) {
                    continue;
                }
                if occupied.iter().any(|(s, e)| start < *e && end > *s) {
                    continue;
                }
                occupied.push((start, end));
                hits.push(DisplayLink {
                    start,
                    end,
                    rule_id: p.id.clone(),
                    action: p.action,
                    text,
                });
                if hits.len() >= self.max_hits_per_line {
                    hits.sort_by_key(|h| h.start);
                    return hits;
                }
            }
        }
        hits.sort_by_key(|h| h.start);
        hits
    }
}

/// SFTP 列表目标目录：去掉末尾斜杠；若末段像文件名（含 `.`）则缩到父目录。
/// 例：`/var/log/nginx/error.log` → `/var/log/nginx`；`/var/log` → `/var/log`。
pub fn sftp_list_dir(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed == "." {
        return "/".into();
    }
    let p = std::path::Path::new(trimmed);
    let file_like = p
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains('.') && n != "." && n != "..");
    let dir = if file_like {
        match p.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => std::path::Path::new("/"),
        }
    } else {
        p
    };
    let mut out = dir.to_string_lossy().replace('\\', "/");
    if out.is_empty() {
        out = "/".into();
    }
    out
}

/// display-link 内置模块。规则在构造时编译一次；禁用后 registry 不再调用钩子。
pub struct DisplayLinkModule {
    engine: DisplayLinkEngine,
}

impl DisplayLinkModule {
    pub fn new() -> Self {
        Self {
            engine: DisplayLinkEngine::new(),
        }
    }
}

impl Default for DisplayLinkModule {
    fn default() -> Self {
        Self::new()
    }
}

impl MistModule for DisplayLinkModule {
    fn id(&self) -> ModuleId {
        MODULE_ID
    }

    fn name_zh(&self) -> &str {
        "输出可点击链接"
    }

    fn name_en(&self) -> &str {
        "Display links"
    }

    fn on_output_line(&self, ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
        // 通知类模块认 Pty；装饰必须对齐视口格子，重扫同一屏不能当成新输出。
        if ctx.origin != LineOrigin::Viewport {
            return Vec::new();
        }
        self.engine
            .scan(ctx.line)
            .into_iter()
            .map(|hit| ModuleAction::DecorateLink {
                start: hit.start,
                end: hit.end,
                action: hit.action.as_str().to_string(),
                text: hit.text,
            })
            .collect()
    }

    fn contribute_context_menu(&self, ctx: &MenuCtx<'_>) -> Vec<MenuItem> {
        // IP 右键才出现「新开连接」。URL / 路径 / 错误码 / 空白处不贡献，避免菜单语义变化。
        if ctx.link_action != Some(ACTION_COPY_OR_CONNECT) {
            return Vec::new();
        }
        vec![MenuItem {
            action: ACTION_NEW_CONNECTION.to_string(),
            label_zh: "新开连接到此 IP".to_string(),
            label_en: "New connection to this IP".to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::ModuleRegistry;

    fn engine() -> DisplayLinkEngine {
        DisplayLinkEngine::new()
    }

    #[test]
    fn builtin_json_parses() {
        let e = engine();
        assert!(!e.is_empty(), "builtin rules must load");
        assert!(e.patterns.len() >= 4);
    }

    #[test]
    fn errno_hit() {
        let e = engine();
        let hits = e.scan("curl: (7) Failed to connect: ECONNREFUSED 127.0.0.1:6379");
        let errno = hits
            .iter()
            .find(|h| h.rule_id == "DL-ERR-001")
            .expect("errno hit");
        assert_eq!(errno.text, "ECONNREFUSED");
        assert_eq!(errno.action, DisplayLinkAction::RunbookRecall);
    }

    #[test]
    fn ipv4_hit_and_version_not_mismatched() {
        let e = engine();
        let hits = e.scan("connect to 192.168.1.10 ok; version 1.2.3 shipped");
        let ip = hits
            .iter()
            .find(|h| h.rule_id == "DL-IPV4-001")
            .expect("ip hit");
        assert_eq!(ip.text, "192.168.1.10");
        assert!(hits.iter().all(|h| h.text != "1.2.3"));
    }

    #[test]
    fn url_hit() {
        let e = engine();
        let hits = e.scan("see https://docs.example.com/guide for details");
        assert!(hits
            .iter()
            .any(|h| h.action == DisplayLinkAction::BrowserOpen
                && h.text.starts_with("https://docs.example.com/guide")));
    }

    #[test]
    fn path_hit() {
        let e = engine();
        let hits = e.scan("failed to open /var/log/nginx/error.log");
        let p = hits
            .iter()
            .find(|h| h.action == DisplayLinkAction::SftpOpen);
        assert!(p.is_some(), "path hit expected, got {:?}", hits);
    }

    #[test]
    fn sftp_list_dir_shrinks_file_to_parent() {
        assert_eq!(sftp_list_dir("/var/log/nginx/error.log"), "/var/log/nginx");
        assert_eq!(sftp_list_dir("/var/log/nginx/"), "/var/log/nginx");
        assert_eq!(sftp_list_dir("/var/log"), "/var/log");
        assert_eq!(sftp_list_dir("/"), "/");
        assert_eq!(sftp_list_dir(""), "/");
    }

    #[test]
    fn overlaps_priority_url_beats_ip() {
        let e = engine();
        let hits = e.scan("at http://10.0.0.7:8080/health");
        let url = hits
            .iter()
            .find(|h| h.action == DisplayLinkAction::BrowserOpen);
        assert!(url.is_some(), "url should win overlap");
        let ip_inside = hits
            .iter()
            .find(|h| h.action == DisplayLinkAction::Copy && h.text == "10.0.0.7");
        assert!(ip_inside.is_none(), "overlapped IP must yield to URL");
    }

    #[test]
    fn max_hits_cap() {
        let e = engine();
        let line = "a EACCES b EACCES c EACCES d EACCES e EACCES f EACCES g EACCES h EACCES
        i EACCES j EACCES k EACCES l EACCES m EACCES n EACCES o EACCES p EACCES q EACCES r EACCES s EACCES t EACCES";
        let hits = e.scan(line);
        assert!(hits.len() <= 16, "cap at 16, got {}", hits.len());
    }

    #[test]
    fn long_line_short_circuit() {
        let e = engine();
        let line = "x".repeat(9000);
        assert!(e.scan(&line).is_empty());
    }

    #[test]
    fn empty_engine_no_hits() {
        let e = DisplayLinkEngine::from_json("{}");
        assert!(e.is_empty());
        assert!(e.scan("EACCES https://example.com").is_empty());
    }

    #[test]
    fn viewport_line_becomes_decorate_link_pty_line_does_not() {
        let m = DisplayLinkModule::new();
        let line = "curl: (7) Failed to connect: ECONNREFUSED 127.0.0.1:6379";
        assert!(m.on_output_line(&OutputLineCtx::pty(line)).is_empty());
        let actions = m.on_output_line(&OutputLineCtx::viewport(line));
        let errno = actions.iter().find_map(|a| match a {
            ModuleAction::DecorateLink { action, text, .. }
                if action == ACTION_RUNBOOK_RECALL && text == "ECONNREFUSED" =>
            {
                Some(())
            }
            _ => None,
        });
        assert!(
            errno.is_some(),
            "viewport scan must decorate errno, got {actions:?}"
        );
        assert!(actions.iter().any(|a| matches!(
            a,
            ModuleAction::DecorateLink { action, text, .. }
                if action == ACTION_COPY_OR_CONNECT && text == "127.0.0.1"
        )));
    }

    #[test]
    fn context_menu_only_for_ip_link() {
        let m = DisplayLinkModule::new();
        let ip = m.contribute_context_menu(&MenuCtx {
            link_action: Some(ACTION_COPY_OR_CONNECT),
            link_text: Some("192.168.1.10"),
        });
        assert_eq!(ip.len(), 1);
        assert_eq!(ip[0].action, ACTION_NEW_CONNECTION);
        assert_eq!(ip[0].label_zh, "新开连接到此 IP");
        assert_eq!(ip[0].label_en, "New connection to this IP");

        assert!(m
            .contribute_context_menu(&MenuCtx {
                link_action: Some(ACTION_BROWSER_OPEN),
                link_text: Some("https://example.com"),
            })
            .is_empty());
        assert!(m.contribute_context_menu(&MenuCtx::plain()).is_empty());
    }

    #[test]
    fn disabled_module_is_absent_from_registry() {
        let reg = ModuleRegistry::with_modules(crate::modules::builtin_modules(std::sync::Arc::new(
            std::sync::Mutex::new(crate::core::CmdAuditEngine::new()),
        )));
        assert!(reg.is_enabled(MODULE_ID));
        let line = "ECONNREFUSED 10.0.0.8 https://example.com/a";
        assert!(!reg
            .on_output_line_for(MODULE_ID, &OutputLineCtx::viewport(line))
            .is_empty());

        reg.set_enabled(MODULE_ID, false);
        assert!(reg
            .on_output_line_for(MODULE_ID, &OutputLineCtx::viewport(line))
            .is_empty());
        assert!(reg
            .on_output_line(&OutputLineCtx::viewport(line))
            .is_empty());
        assert!(reg
            .context_menu_items(&MenuCtx {
                link_action: Some(ACTION_COPY_OR_CONNECT),
                link_text: Some("10.0.0.8"),
            })
            .is_empty());
    }

    // --- #140 系统二进制路径不产链接 ---

    #[test]
    fn system_binary_paths_are_not_linked() {
        let engine = DisplayLinkEngine::new();
        // ps 输出典型行：CMD 列的系统二进制路径不画下划线（#140）。
        let line = "/usr/sbin/sshd  /usr/sbin/crond  /usr/bin/python";
        assert!(engine.scan(line).is_empty());

        // 目录名本身也不链。
        assert!(engine.scan("/usr/sbin /bin /lib64").is_empty());

        // 系统目录下的配置/数据（/etc /var /opt）不受影响，仍可点。
        let cfg = engine.scan("see /etc/nginx/nginx.conf and /var/log/app/error.log");
        assert!(cfg.iter().any(|l| l.text == "/etc/nginx/nginx.conf"));
        assert!(cfg.iter().any(|l| l.text == "/var/log/app/error.log"));

        // 用户/项目路径不受影响。
        let user = engine.scan("edit /home/tian/app/config.toml and /opt/svc/run.sh");
        assert!(user.iter().any(|l| l.text == "/home/tian/app/config.toml"));
        assert!(user.iter().any(|l| l.text == "/opt/svc/run.sh"));

        // 相对路径与 ~ 路径不在过滤范围（deny-list 只拦绝对路径）。
        let rel = engine.scan("run ~/bin/tool and /usr/local/bin/node");
        assert!(rel.iter().any(|l| l.text == "~/bin/tool"));
        assert!(!rel.iter().any(|l| l.text == "/usr/local/bin/node"));
    }

    #[test]
    fn is_system_binary_path_unit() {
        assert!(is_system_binary_path("/usr/sbin/sshd"));
        assert!(is_system_binary_path("/usr/bin/python3.11"));
        assert!(is_system_binary_path("/bin/bash"));
        assert!(is_system_binary_path("/usr/local/bin/node"));
        assert!(is_system_binary_path("/usr/sbin"));
        assert!(!is_system_binary_path("/etc/nginx/nginx.conf"));
        assert!(!is_system_binary_path("/var/log/app"));
        assert!(!is_system_binary_path("/home/tian/app"));
        assert!(!is_system_binary_path("./sbin/tool"));
        assert!(!is_system_binary_path("./usr/local/tool"));
        assert!(!is_system_binary_path("~/bin/tool"));
        assert!(!is_system_binary_path("~/bin/tool"));
        assert!(!is_system_binary_path("/usr/share/doc")); // /usr/ 前缀不拦，拦的是 /usr/bin|sbin
    }



}
