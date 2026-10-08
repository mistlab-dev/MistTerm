//! 判断一条 shell 命令是不是「只读」（只查看、不改动主机）。
//!
//! 运维助手用它决定：只读命令可以在「这次排查期间同类不再问」后直接执行；
//! 改动类命令（删除、重启服务、写文件……）每条都要确认；看不懂的命令也要确认。
//!
//! 做法：自己切分命令（引号、注释、`;` `&&` `||` `|`、换行、`$(…)`、反引号、重定向），
//! 再逐个看每段命令的程序名和参数。宁可把只读命令当成「看不懂」去确认，也不能把改动当成只读。

use std::collections::BTreeSet;

/// 只读命令的类别（用于「同类只读命令不再问」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadOnlyKind {
    /// 看日志 / 看文件内容
    Logs,
    /// 看进程、服务、容器
    Processes,
    /// 看磁盘和目录
    Disk,
    /// 看网络和连通性（含 curl 健康检查）
    Network,
    /// 看系统状态（负载、内存、内核、时间……）
    System,
    /// 只有 echo / date 之类最简单的命令
    Basic,
}

impl ReadOnlyKind {
    pub fn label_zh(self) -> &'static str {
        match self {
            ReadOnlyKind::Logs => "看日志",
            ReadOnlyKind::Processes => "看进程和服务",
            ReadOnlyKind::Disk => "看磁盘和目录",
            ReadOnlyKind::Network => "看网络",
            ReadOnlyKind::System => "看系统状态",
            ReadOnlyKind::Basic => "简单查看",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            ReadOnlyKind::Logs => "logs",
            ReadOnlyKind::Processes => "processes & services",
            ReadOnlyKind::Disk => "disk & files",
            ReadOnlyKind::Network => "network",
            ReadOnlyKind::System => "system status",
            ReadOnlyKind::Basic => "basic info",
        }
    }
}

/// 分类结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandClass {
    /// 只读；`kinds` 不为空。
    ReadOnly { kinds: BTreeSet<ReadOnlyKind> },
    /// 会改动主机或远端服务；`reason` 给用户看。
    Mutating { reason: String },
    /// 看不懂（未知程序、脚本、复杂语法）：需要确认，但不算改动。
    Unknown { reason: String },
}

impl CommandClass {
    pub fn is_read_only(&self) -> bool {
        matches!(self, CommandClass::ReadOnly { .. })
    }
    pub fn is_mutating(&self) -> bool {
        matches!(self, CommandClass::Mutating { .. })
    }
    pub fn kinds(&self) -> Option<&BTreeSet<ReadOnlyKind>> {
        match self {
            CommandClass::ReadOnly { kinds } => Some(kinds),
            _ => None,
        }
    }
}

/// 一段简单命令：词（已去掉引号）+ 输出重定向目标。
#[derive(Debug, Default, Clone)]
struct Segment {
    words: Vec<String>,
    /// `(运算符, 目标)`，只记录会写东西的重定向。
    writes: Vec<(String, String)>,
}

struct Parsed {
    segments: Vec<Segment>,
    /// `$(…)`、反引号、`<(…)`、`>(…)` 里的命令，需要递归判断。
    subs: Vec<String>,
    /// 解析失败（引号没闭合等）。
    broken: Option<String>,
    /// 出现了 here-doc：后面的行不是命令，整体按看不懂处理。
    heredoc: bool,
}

const MAX_DEPTH: usize = 6;

/// 判断命令是否只读。
pub fn classify_command(command: &str) -> CommandClass {
    classify_depth(command, 0)
}

fn classify_depth(command: &str, depth: usize) -> CommandClass {
    if depth > MAX_DEPTH {
        return unknown("命令嵌套太深");
    }
    let parsed = parse(command);
    if let Some(why) = parsed.broken {
        return unknown(&why);
    }
    let mut kinds = BTreeSet::new();
    let mut first_unknown: Option<String> = None;
    for seg in &parsed.segments {
        for (op, target) in &seg.writes {
            if !is_harmless_redirect_target(target) {
                return mutating(&format!("会写入文件 {target}（{op}）"));
            }
        }
        match classify_segment(&seg.words, depth) {
            SegClass::Neutral => {}
            SegClass::Kind(k) => {
                kinds.insert(k);
            }
            SegClass::Kinds(ks) => kinds.extend(ks),
            SegClass::Mutating(r) => return mutating(&r),
            SegClass::Unknown(r) => {
                first_unknown.get_or_insert(r);
            }
        }
    }
    for sub in &parsed.subs {
        match classify_depth(sub, depth + 1) {
            CommandClass::ReadOnly { kinds: k } => kinds.extend(k),
            CommandClass::Mutating { reason } => return CommandClass::Mutating { reason },
            CommandClass::Unknown { reason } => {
                first_unknown.get_or_insert(reason);
            }
        }
    }
    if parsed.heredoc {
        first_unknown.get_or_insert("包含 here-doc（<<），无法逐行判断".into());
    }
    if let Some(r) = first_unknown {
        return CommandClass::Unknown { reason: r };
    }
    if kinds.is_empty() {
        if parsed.segments.iter().all(|s| s.words.is_empty()) && parsed.subs.is_empty() {
            return unknown("空命令");
        }
        kinds.insert(ReadOnlyKind::Basic);
    }
    CommandClass::ReadOnly { kinds }
}

fn mutating(r: &str) -> CommandClass {
    CommandClass::Mutating {
        reason: r.to_string(),
    }
}

fn unknown(r: &str) -> CommandClass {
    CommandClass::Unknown {
        reason: r.to_string(),
    }
}

fn is_harmless_redirect_target(t: &str) -> bool {
    matches!(
        t,
        "/dev/null"
            | "/dev/stdout"
            | "/dev/stderr"
            | "/dev/tty"
            | "-"
            | "1"
            | "2"
            | "&1"
            | "&2"
            | "&-"
    )
}

/// 运维助手「这次排查期间，同类只读命令不再问」：命令只读、且它涉及的每一类都已放行时才自动执行。
/// 改动命令、看不懂的命令永远返回 false（每次都要确认）。
pub fn readonly_auto_run_allowed(class: &CommandClass, trusted: &BTreeSet<ReadOnlyKind>) -> bool {
    match class {
        CommandClass::ReadOnly { kinds } => !kinds.is_empty() && kinds.is_subset(trusted),
        _ => false,
    }
}

// ---------------------------------------------------------------- 解析

struct Parser {
    chars: Vec<char>,
    i: usize,
    out: Parsed,
    seg: Segment,
    word: String,
    in_word: bool,
    /// 下一个词是重定向目标：(运算符, 是否写)
    pending_redirect: Option<(String, bool)>,
}

impl Parser {
    fn end_word(&mut self) {
        if self.in_word {
            let w = std::mem::take(&mut self.word);
            if let Some((op, is_write)) = self.pending_redirect.take() {
                if is_write {
                    self.seg.writes.push((op, w));
                }
            } else {
                self.seg.words.push(w);
            }
            self.in_word = false;
        }
    }

    fn end_segment(&mut self) -> Result<(), String> {
        self.end_word();
        if self.pending_redirect.is_some() {
            return Err("重定向后面缺少目标".into());
        }
        if !self.seg.words.is_empty() || !self.seg.writes.is_empty() {
            self.out.segments.push(std::mem::take(&mut self.seg));
        }
        Ok(())
    }

    fn push_sub(&mut self, inner: String) {
        self.out.subs.push(inner);
        self.word.push_str("$SUB");
        self.in_word = true;
    }

    fn run(&mut self) -> Result<(), String> {
        while self.i < self.chars.len() {
            let c = self.chars[self.i];
            match c {
                '\\' => {
                    if self.i + 1 < self.chars.len() {
                        if self.chars[self.i + 1] != '\n' {
                            self.word.push(self.chars[self.i + 1]);
                            self.in_word = true;
                        }
                        self.i += 2;
                    } else {
                        self.i += 1;
                    }
                }
                '\'' => {
                    self.in_word = true;
                    self.i += 1;
                    let start = self.i;
                    while self.i < self.chars.len() && self.chars[self.i] != '\'' {
                        self.i += 1;
                    }
                    if self.i >= self.chars.len() {
                        return Err("单引号没有闭合".into());
                    }
                    let s: String = self.chars[start..self.i].iter().collect();
                    self.word.push_str(&s);
                    self.i += 1;
                }
                '"' => {
                    self.in_word = true;
                    self.i += 1;
                    loop {
                        if self.i >= self.chars.len() {
                            return Err("双引号没有闭合".into());
                        }
                        match self.chars[self.i] {
                            '"' => {
                                self.i += 1;
                                break;
                            }
                            '\\' if self.i + 1 < self.chars.len() => {
                                self.word.push(self.chars[self.i + 1]);
                                self.i += 2;
                            }
                            '$' if self.chars.get(self.i + 1) == Some(&'(') => {
                                let (inner, next, arith) = read_dollar_paren(&self.chars, self.i)?;
                                self.i = next;
                                if arith {
                                    self.word.push('0');
                                } else {
                                    self.push_sub(inner);
                                }
                            }
                            '`' => {
                                let (inner, next) = read_backtick(&self.chars, self.i)?;
                                self.i = next;
                                self.push_sub(inner);
                            }
                            ch => {
                                self.word.push(ch);
                                self.i += 1;
                            }
                        }
                    }
                }
                '$' if self.chars.get(self.i + 1) == Some(&'(') => {
                    let (inner, next, arith) = read_dollar_paren(&self.chars, self.i)?;
                    self.i = next;
                    if arith {
                        self.word.push('0');
                        self.in_word = true;
                    } else {
                        self.push_sub(inner);
                    }
                }
                '`' => {
                    let (inner, next) = read_backtick(&self.chars, self.i)?;
                    self.i = next;
                    self.push_sub(inner);
                }
                '#' if !self.in_word => {
                    while self.i < self.chars.len() && self.chars[self.i] != '\n' {
                        self.i += 1;
                    }
                }
                ' ' | '\t' | '\r' => {
                    self.end_word();
                    self.i += 1;
                }
                '\n' | ';' => {
                    self.end_segment()?;
                    self.i += 1;
                    while self.chars.get(self.i) == Some(&';') {
                        self.i += 1;
                    }
                }
                '|' => {
                    self.end_segment()?;
                    self.i += 1;
                    if matches!(self.chars.get(self.i), Some('|') | Some('&')) {
                        self.i += 1;
                    }
                }
                '&' => {
                    if self.chars.get(self.i + 1) == Some(&'>') {
                        self.end_word();
                        let mut op = String::from("&>");
                        self.i += 2;
                        if self.chars.get(self.i) == Some(&'>') {
                            op.push('>');
                            self.i += 1;
                        }
                        self.pending_redirect = Some((op, true));
                    } else {
                        self.end_segment()?;
                        self.i += 1;
                        if self.chars.get(self.i) == Some(&'&') {
                            self.i += 1;
                        }
                    }
                }
                '(' if !self.in_word && self.chars.get(self.i + 1) == Some(&'(') => {
                    // 算术 ((i++))：跳过，不当命令
                    let (_, next) = read_paren(&self.chars, self.i)?;
                    self.i = next;
                }
                '(' | ')' | '{' | '}' if !self.in_word => {
                    if (c == '{' || c == '}')
                        && !matches!(
                            self.chars.get(self.i + 1),
                            None | Some(' ' | '\t' | '\n' | ';')
                        )
                    {
                        // `{a,b}` 之类的花括号展开，按普通字符
                        self.word.push(c);
                        self.in_word = true;
                        self.i += 1;
                        continue;
                    }
                    self.end_segment()?;
                    self.i += 1;
                }
                '<' | '>' => self.redirect(c)?,
                _ => {
                    self.word.push(c);
                    self.in_word = true;
                    self.i += 1;
                }
            }
        }
        self.end_segment()
    }

    fn redirect(&mut self, c: char) -> Result<(), String> {
        if self.chars.get(self.i + 1) == Some(&'(') {
            // 进程替换 <(…) / >(…)
            let (inner, next) = read_paren(&self.chars, self.i + 1)?;
            self.i = next;
            self.push_sub(inner);
            return Ok(());
        }
        // 词全是数字（文件描述符，如 2>）就并入运算符
        let mut op = String::new();
        if self.in_word && !self.word.is_empty() && self.word.chars().all(|ch| ch.is_ascii_digit())
        {
            op.push_str(&std::mem::take(&mut self.word));
            self.in_word = false;
        } else {
            self.end_word();
        }
        op.push(c);
        self.i += 1;
        while let Some(&n) = self.chars.get(self.i) {
            if n == '>' || n == '<' || n == '&' || n == '|' {
                op.push(n);
                self.i += 1;
            } else {
                break;
            }
        }
        if op.contains("<<") && !op.contains("<<<") {
            self.out.heredoc = true;
        }
        if op.ends_with('&') {
            // 2>&1 / >&2 / <&0：目标是文件描述符
            let mut t = String::new();
            while let Some(&n) = self.chars.get(self.i) {
                if n.is_ascii_digit() || n == '-' {
                    t.push(n);
                    self.i += 1;
                } else {
                    break;
                }
            }
            if !t.is_empty() {
                return Ok(());
            }
        }
        let is_write = op.contains('>');
        self.pending_redirect = Some((op, is_write));
        Ok(())
    }
}

fn parse(src: &str) -> Parsed {
    let mut p = Parser {
        chars: src.chars().collect(),
        i: 0,
        out: Parsed {
            segments: Vec::new(),
            subs: Vec::new(),
            broken: None,
            heredoc: false,
        },
        seg: Segment::default(),
        word: String::new(),
        in_word: false,
        pending_redirect: None,
    };
    if let Err(e) = p.run() {
        p.out.broken = Some(e);
    }
    p.out
}

/// `$(` 起始位置 → (内部文本, 结束后的位置, 是否算术 `$((…))`)
fn read_dollar_paren(chars: &[char], start: usize) -> Result<(String, usize, bool), String> {
    if chars.get(start + 2) == Some(&'(') {
        let mut depth = 0i32;
        let mut i = start + 1;
        while i < chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok((String::new(), i + 1, true));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        return Err("$(( 没有闭合".into());
    }
    let (inner, next) = read_paren(chars, start + 1)?;
    Ok((inner, next, false))
}

/// `(` 起始位置 → (括号内文本, 结束后的位置)。处理嵌套和引号。
fn read_paren(chars: &[char], open: usize) -> Result<(String, usize), String> {
    let mut depth = 0i32;
    let mut i = open;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' && q == '"' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else {
            match c {
                '\'' | '"' => quote = Some(c),
                '\\' => {
                    i += 2;
                    continue;
                }
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok((chars[open + 1..i].iter().collect(), i + 1));
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    Err("括号没有闭合".into())
}

fn read_backtick(chars: &[char], open: usize) -> Result<(String, usize), String> {
    let mut i = open + 1;
    let mut inner = String::new();
    while i < chars.len() {
        match chars[i] {
            '`' => return Ok((inner, i + 1)),
            '\\' if i + 1 < chars.len() => {
                inner.push(chars[i + 1]);
                i += 2;
            }
            c => {
                inner.push(c);
                i += 1;
            }
        }
    }
    Err("反引号没有闭合".into())
}

// ---------------------------------------------------------------- 逐段判断

enum SegClass {
    /// 不影响类别（控制结构、echo、过滤命令……）
    Neutral,
    Kind(ReadOnlyKind),
    Kinds(BTreeSet<ReadOnlyKind>),
    Mutating(String),
    Unknown(String),
}

use ReadOnlyKind::{Basic, Disk, Logs, Network, Processes, System};

fn from_class(c: CommandClass) -> SegClass {
    match c {
        CommandClass::ReadOnly { kinds } => SegClass::Kinds(kinds),
        CommandClass::Mutating { reason } => SegClass::Mutating(reason),
        CommandClass::Unknown { reason } => SegClass::Unknown(reason),
    }
}

fn mut_(r: impl Into<String>) -> SegClass {
    SegClass::Mutating(r.into())
}

fn unk(r: impl Into<String>) -> SegClass {
    SegClass::Unknown(r.into())
}

/// 条件成立就是改动，否则是某类只读。
fn ro_unless(bad: bool, kind: ReadOnlyKind, why: &str) -> SegClass {
    if bad {
        mut_(why)
    } else {
        SegClass::Kind(kind)
    }
}

fn is_assignment(w: &str) -> bool {
    match w.split_once('=') {
        Some((name, _)) => {
            name.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

fn basename(w: &str) -> &str {
    w.rsplit('/').next().unwrap_or(w)
}

/// 非选项参数（跳过 `takes_value` 里列出的选项的值）。
fn positionals<'a>(args: &'a [String], takes_value: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut end_of_opts = false;
    while i < args.len() {
        let a = args[i].as_str();
        if end_of_opts || !a.starts_with('-') || a == "-" {
            out.push(a);
        } else if a == "--" {
            end_of_opts = true;
        } else if takes_value.contains(&a) {
            i += 1;
        }
        i += 1;
    }
    out
}

/// 是否带某个选项（长选项也认 `--name=value`）。
fn has_flag(args: &[String], names: &[&str]) -> bool {
    args.iter().any(|a| {
        names.iter().any(|n| {
            a == n || (n.starts_with("--") && a.starts_with(n) && a[n.len()..].starts_with('='))
        })
    })
}

/// 短选项组合里是否有某个字母，如 `-nvL` 含 `L`。
fn has_short(args: &[String], ch: char) -> bool {
    args.iter()
        .any(|a| a.starts_with('-') && !a.starts_with("--") && a.len() > 1 && a[1..].contains(ch))
}

/// 跳过开头的选项（以及 `takes_value` 里选项的值），返回剩下的词。
fn skip_opts(args: &[String], takes_value: &[&str]) -> Vec<String> {
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            i += 1;
            break;
        }
        if !a.starts_with('-') {
            break;
        }
        i += if takes_value.contains(&a.as_str()) {
            2
        } else {
            1
        };
    }
    args[i.min(args.len())..].to_vec()
}

const SHELL_TERMINATORS: &[&str] = &["done", "fi", "esac", "}", ")", "then", "do", "else"];
const SHELL_PREFIXES: &[&str] = &[
    "!", "do", "then", "else", "elif", "if", "while", "until", "time", "{", "(",
];
/// 不改东西、也不算某类排查的命令：过滤、格式化、shell 内建。
const NEUTRAL: &[&str] = &[
    "echo",
    "printf",
    "true",
    "false",
    "test",
    "[",
    "[[",
    "]]",
    "sleep",
    "read",
    "cd",
    "pwd",
    "export",
    "set",
    "unset",
    "local",
    "declare",
    "typeset",
    "type",
    "which",
    "whereis",
    "hash",
    "seq",
    "expr",
    "exit",
    "return",
    "break",
    "continue",
    "shift",
    "wait",
    "jq",
    "column",
    "xxd",
    "hexdump",
    "od",
    "strings",
    "rev",
    "fold",
    "fmt",
    "paste",
    "join",
    "comm",
    "diff",
    "cmp",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "sort",
    "uniq",
    "wc",
    "cut",
    "tr",
    "nl",
    "md5sum",
    "sha1sum",
    "sha256sum",
    "sha512sum",
    "cksum",
    "numfmt",
    "bc",
    "tput",
    ":",
    "let",
    "history",
    "base64",
];
/// 一定会改动主机的命令。
const MUTATING: &[&str] = &[
    "rm",
    "rmdir",
    "mv",
    "cp",
    "mkdir",
    "touch",
    "chmod",
    "chown",
    "chgrp",
    "chattr",
    "setfacl",
    "ln",
    "install",
    "truncate",
    "shred",
    "dd",
    "mkfs",
    "fdisk",
    "parted",
    "sfdisk",
    "wipefs",
    "mkswap",
    "swapon",
    "swapoff",
    "umount",
    "kill",
    "pkill",
    "killall",
    "reboot",
    "shutdown",
    "poweroff",
    "halt",
    "init",
    "telinit",
    "useradd",
    "userdel",
    "usermod",
    "groupadd",
    "groupdel",
    "groupmod",
    "passwd",
    "chpasswd",
    "apt-get",
    "aptitude",
    "modprobe",
    "rmmod",
    "insmod",
    "renice",
    "ufw",
    "firewall-cmd",
    "setenforce",
    "update-alternatives",
    "logrotate",
    "ntpdate",
    "hwclock",
];

/// 直接算某类只读的命令（不看参数）。
const SIMPLE_RO: &[(&str, ReadOnlyKind)] = &[
    ("last", Logs),
    ("lastb", Logs),
    ("lastlog", Logs),
    ("aureport", Logs),
    ("ausearch", Logs),
    ("ps", Processes),
    ("pgrep", Processes),
    ("pidof", Processes),
    ("pstree", Processes),
    ("lsof", Processes),
    ("pmap", Processes),
    ("pwdx", Processes),
    ("df", Disk),
    ("du", Disk),
    ("ls", Disk),
    ("ll", Disk),
    ("dir", Disk),
    ("lsblk", Disk),
    ("blkid", Disk),
    ("findmnt", Disk),
    ("stat", Disk),
    ("file", Disk),
    ("tree", Disk),
    ("locate", Disk),
    ("getfacl", Disk),
    ("namei", Disk),
    ("pvs", Disk),
    ("vgs", Disk),
    ("lvs", Disk),
    ("pvdisplay", Disk),
    ("vgdisplay", Disk),
    ("lvdisplay", Disk),
    ("lsattr", Disk),
    ("netstat", Network),
    ("ping", Network),
    ("ping6", Network),
    ("traceroute", Network),
    ("traceroute6", Network),
    ("tracepath", Network),
    ("dig", Network),
    ("nslookup", Network),
    ("host", Network),
    ("getent", Network),
    ("whois", Network),
    ("iptables-save", Network),
    ("ip6tables-save", Network),
    ("uptime", System),
    ("free", System),
    ("uname", System),
    ("whoami", System),
    ("id", System),
    ("groups", System),
    ("w", System),
    ("who", System),
    ("vmstat", System),
    ("mpstat", System),
    ("iostat", System),
    ("sar", System),
    ("pidstat", System),
    ("nproc", System),
    ("lscpu", System),
    ("lsmem", System),
    ("lspci", System),
    ("lsusb", System),
    ("lsmod", System),
    ("modinfo", System),
    ("dmidecode", System),
    ("printenv", System),
    ("getconf", System),
    ("locale", System),
    ("arch", System),
    ("systemd-analyze", System),
    ("lsb_release", System),
    ("sestatus", System),
    ("getenforce", System),
    ("numastat", System),
    ("ipcs", System),
    ("ulimit", System),
    ("users", System),
    ("tty", System),
    ("cal", Basic),
    ("systemd-detect-virt", System),
    ("ntpq", System),
    ("dpkg-query", System),
];

fn classify_segment(words: &[String], depth: usize) -> SegClass {
    let mut start = 0;
    while let Some(first) = words.get(start) {
        if is_assignment(first) || SHELL_PREFIXES.contains(&first.as_str()) {
            start += 1;
        } else {
            break;
        }
    }
    let Some(prog_raw) = words.get(start) else {
        return SegClass::Neutral;
    };
    if SHELL_TERMINATORS.contains(&prog_raw.as_str()) {
        return SegClass::Neutral;
    }
    let prog = basename(prog_raw);
    let args = &words[start + 1..];

    if let Some((_, k)) = SIMPLE_RO.iter().find(|(p, _)| *p == prog) {
        return SegClass::Kind(*k);
    }
    if NEUTRAL.contains(&prog) {
        return SegClass::Neutral;
    }
    if MUTATING.contains(&prog) || prog.starts_with("mkfs.") {
        return mut_(format!("{prog} 会改动主机"));
    }
    match prog {
        // ---- 控制结构
        "for" | "select" => {
            if args.len() <= 1 || args.get(1).is_some_and(|a| a == "in") {
                SegClass::Neutral
            } else {
                unk("for 循环写法看不懂")
            }
        }
        "case" => unk("case 语句需要人工确认"),
        "function" => unk("定义函数需要人工确认"),
        _ if prog.ends_with("()") => unk("定义函数需要人工确认"),
        "eval" | "source" | "." | "exec" | "trap" => {
            unk(format!("{prog} 会执行别的内容，需要确认"))
        }

        // ---- 包装命令：看里面真正执行的命令
        "sudo" | "doas" => wrapped(
            args,
            &["-u", "-g", "-h", "-p", "-C", "-D", "-r", "-t", "-U"],
            depth,
            prog,
        ),
        "timeout" => {
            let rest = skip_opts(args, &["-s", "--signal", "-k", "--kill-after"]);
            if rest.is_empty() {
                unk("timeout 后面缺少命令")
            } else {
                classify_words(&rest[1..], depth)
            }
        }
        "nice" => wrapped(args, &["-n", "--adjustment"], depth, prog),
        "ionice" => wrapped(args, &["-c", "-n", "-p", "-P", "-u"], depth, prog),
        "stdbuf" => wrapped(args, &["-i", "-o", "-e"], depth, prog),
        "command" if args.first().is_some_and(|a| a == "-v" || a == "-V") => SegClass::Neutral,
        "command" | "builtin" => classify_words(args, depth),
        "env" => {
            let mut i = 0;
            while i < args.len() {
                let a = args[i].as_str();
                if matches!(a, "-u" | "--unset" | "-C" | "--chdir") {
                    i += 2;
                } else if a.starts_with('-') || is_assignment(a) {
                    i += 1;
                } else {
                    break;
                }
            }
            if i >= args.len() {
                SegClass::Kind(System)
            } else {
                classify_words(&args[i..], depth)
            }
        }
        "xargs" => {
            let rest = skip_opts(
                args,
                &[
                    "-n",
                    "-I",
                    "-i",
                    "-P",
                    "-d",
                    "-L",
                    "-l",
                    "-s",
                    "-a",
                    "-E",
                    "-e",
                    "--max-args",
                    "--max-procs",
                    "--delimiter",
                    "--arg-file",
                    "--replace",
                ],
            );
            classify_words(&rest, depth) // 没写命令时 xargs 执行 echo
        }
        "watch" => {
            let rest = skip_opts(args, &["-n", "--interval"]);
            if rest.is_empty() {
                unk("watch 后面缺少命令")
            } else {
                from_class(classify_depth(&rest.join(" "), depth + 1))
            }
        }
        "bash" | "sh" | "dash" | "zsh" | "ksh" => {
            let pos = args
                .iter()
                .position(|a| a.starts_with('-') && !a.starts_with("--") && a.contains('c'));
            match pos.and_then(|p| args.get(p + 1)) {
                Some(script) => from_class(classify_depth(script, depth + 1)),
                None => unk(format!("运行 {prog} 脚本，需要确认")),
            }
        }
        "tee" => {
            let files = positionals(args, &[]);
            if files.iter().all(|f| is_harmless_redirect_target(f)) {
                SegClass::Neutral
            } else {
                mut_(format!("tee 会写入文件 {}", files.join(" ")))
            }
        }

        // ---- 看日志 / 文件内容（不带文件名时只是管道里的过滤）
        "cat" | "tac" | "zcat" | "bzcat" | "xzcat" | "zless" | "less" | "more" => {
            file_reader(positionals(args, &[]).len())
        }
        "head" | "tail" => file_reader(
            positionals(
                args,
                &[
                    "-n",
                    "-c",
                    "--lines",
                    "--bytes",
                    "-s",
                    "--sleep-interval",
                    "--pid",
                    "--max-unchanged-stats",
                ],
            )
            .len(),
        ),
        "grep" | "egrep" | "fgrep" | "zgrep" | "zegrep" | "rg" | "ag" => {
            let tv = [
                "-e",
                "-f",
                "-m",
                "-A",
                "-B",
                "-C",
                "--regexp",
                "--file",
                "--max-count",
                "--context",
                "--after-context",
                "--before-context",
                "-g",
                "--glob",
                "-t",
                "--type",
                "-d",
                "-D",
            ];
            let pattern_given = has_flag(args, &["-e", "-f", "--regexp", "--file"]);
            let pos = positionals(args, &tv).len();
            let files = if pattern_given {
                pos
            } else {
                pos.saturating_sub(1)
            };
            let recursive =
                has_flag(args, &["--recursive"]) || has_short(args, 'r') || has_short(args, 'R');
            if files == 0 && !recursive && prog != "rg" && prog != "ag" {
                SegClass::Neutral
            } else {
                SegClass::Kind(Logs)
            }
        }
        "awk" | "gawk" | "mawk" | "nawk" => classify_awk(args),
        "sed" => classify_sed(args),
        "journalctl" => ro_unless(
            has_flag(
                args,
                &[
                    "--vacuum-size",
                    "--vacuum-time",
                    "--vacuum-files",
                    "--rotate",
                    "--flush",
                    "--sync",
                    "--relinquish-var",
                    "--smart-relinquish-var",
                    "--setup-keys",
                    "--update-catalog",
                ],
            ),
            Logs,
            "journalctl 这个参数会清理或改动日志",
        ),
        "dmesg" => ro_unless(
            has_flag(
                args,
                &[
                    "-c",
                    "-C",
                    "--clear",
                    "--read-clear",
                    "-D",
                    "--console-off",
                    "-E",
                    "--console-on",
                    "-n",
                    "--console-level",
                ],
            ),
            Logs,
            "dmesg 这个参数会清空或改动内核日志设置",
        ),

        // ---- 进程、服务、容器
        "fuser" => ro_unless(
            has_short(args, 'k') || has_flag(args, &["--kill"]),
            Processes,
            "fuser -k 会结束进程",
        ),
        "top" => {
            if has_short(args, 'b') {
                SegClass::Kind(Processes)
            } else {
                unk("top 要加 -b -n 1 才能非交互运行")
            }
        }
        "systemctl" => classify_systemctl(args),
        "service" => {
            let pos = positionals(args, &[]);
            ro_unless(
                !(has_flag(args, &["--status-all"]) || pos.get(1) == Some(&"status")),
                Processes,
                &format!("service {} 会改动服务状态", pos.join(" ")),
            )
        }
        "docker" | "podman" | "nerdctl" => classify_docker(args),
        "kubectl" | "oc" => classify_kubectl(args, prog),
        "crictl" => sub_allow(
            args,
            &[
                "ps", "pods", "images", "img", "logs", "inspect", "inspecti", "inspectp", "stats",
                "info", "version",
            ],
            Processes,
            prog,
        ),
        "supervisorctl" => sub_allow(
            args,
            &["status", "avail", "pid", "tail", "version"],
            Processes,
            prog,
        ),
        "pm2" => sub_allow(
            args,
            &[
                "list",
                "ls",
                "status",
                "jlist",
                "describe",
                "show",
                "logs",
                "info",
                "prettylist",
            ],
            Processes,
            prog,
        ),

        // ---- 磁盘和目录
        "mount" => ro_unless(
            !positionals(args, &["-t"]).is_empty()
                || has_flag(
                    args,
                    &["-a", "--all", "-o", "--bind", "--move", "--remount"],
                ),
            Disk,
            "mount 会挂载或改动文件系统",
        ),
        "find" => classify_find(args, depth),
        "smartctl" => ro_unless(
            has_flag(
                args,
                &[
                    "-s",
                    "--smart",
                    "-o",
                    "--offlineauto",
                    "-S",
                    "--saveauto",
                    "-t",
                    "--test",
                    "-X",
                    "--abort",
                ],
            ),
            Disk,
            "smartctl 这个参数会改磁盘设置或启动自检",
        ),

        // ---- 网络
        "ss" => ro_unless(
            has_short(args, 'K') || has_flag(args, &["--kill"]),
            Network,
            "ss -K 会断开连接",
        ),
        "ethtool" => ro_unless(
            args.iter().any(|a| {
                [
                    "-s",
                    "-K",
                    "-G",
                    "-A",
                    "-C",
                    "-L",
                    "--change",
                    "--offload",
                    "--set-ring",
                ]
                .contains(&a.as_str())
            }),
            Network,
            "ethtool 这个参数会改网卡设置",
        ),
        "mtr" => {
            if has_flag(
                args,
                &[
                    "-r",
                    "--report",
                    "-w",
                    "--report-wide",
                    "-j",
                    "--json",
                    "-C",
                    "--csv",
                ],
            ) {
                SegClass::Kind(Network)
            } else {
                unk("mtr 要加 --report 才能非交互运行")
            }
        }
        "ip" => ro_unless(
            args.iter().any(|a| {
                [
                    "add", "del", "delete", "set", "flush", "change", "replace", "append",
                    "prepend", "restore", "exec", "attach", "detach",
                ]
                .contains(&a.as_str())
            }),
            Network,
            "ip 这个子命令会改网络配置",
        ),
        "ifconfig" => ro_unless(
            positionals(args, &[]).len() > 1,
            Network,
            "ifconfig 带参数会改网卡配置",
        ),
        "route" => ro_unless(
            !positionals(args, &[]).is_empty(),
            Network,
            "route 带参数会改路由",
        ),
        "arp" => ro_unless(
            has_flag(args, &["-d", "-s", "-f", "--delete", "--set", "--file"]),
            Network,
            "arp 这个参数会改 ARP 表",
        ),
        "iptables" | "ip6tables" | "iptables-legacy" | "iptables-nft" => classify_iptables(args),
        "nft" => ro_unless(
            positionals(args, &[]).first() != Some(&"list") || has_flag(args, &["-f", "--file"]),
            Network,
            "nft 会改防火墙规则",
        ),
        "conntrack" => ro_unless(
            !has_flag(
                args,
                &[
                    "-L", "--dump", "-S", "--stats", "-C", "--count", "-G", "--get",
                ],
            ),
            Network,
            "conntrack 会改连接跟踪表",
        ),
        "nc" | "ncat" | "netcat" => {
            if has_short(args, 'z') {
                SegClass::Kind(Network)
            } else {
                unk("nc 只有加 -z（只探测端口）才算只读")
            }
        }
        "curl" => classify_curl(args),
        "wget" => classify_wget(args),
        "openssl" => {
            let sub = args.first().map(String::as_str).unwrap_or("");
            if [
                "s_client",
                "x509",
                "version",
                "verify",
                "ciphers",
                "crl",
                "asn1parse",
            ]
            .contains(&sub)
                && !has_flag(args, &["-out", "-keyout"])
            {
                SegClass::Kind(Network)
            } else {
                unk(format!("openssl {sub} 需要确认"))
            }
        }

        // ---- 系统状态
        "date" => ro_unless(
            has_flag(args, &["-s", "--set"])
                || positionals(
                    args,
                    &["-d", "--date", "-r", "--reference", "-f", "--file", "-I"],
                )
                .iter()
                .any(|p| !p.starts_with('+')),
            Basic,
            "date 带这个参数会改系统时间",
        ),
        "hostname" => ro_unless(
            !positionals(args, &[]).is_empty() || has_flag(args, &["-F", "--file", "-b", "--boot"]),
            System,
            "hostname 带参数会改主机名",
        ),
        "hostnamectl" | "timedatectl" | "localectl" | "loginctl" | "resolvectl" | "networkctl" => {
            let sub = positionals(
                args,
                &["-H", "--host", "-M", "--machine", "-p", "--property"],
            )
            .first()
            .copied()
            .unwrap_or("status");
            ro_unless(
                !(sub == "status"
                    || sub == "show"
                    || sub.starts_with("list")
                    || sub.ends_with("status")
                    || sub == "query"
                    || sub == "statistics"
                    || sub == "show-timesync"),
                System,
                &format!("{prog} {sub} 会改系统设置"),
            )
        }
        "sysctl" => ro_unless(
            has_flag(args, &["-w", "--write", "-p", "--load", "--system", "-f"])
                || args.iter().any(|a| !a.starts_with('-') && a.contains('=')),
            System,
            "sysctl 会改内核参数",
        ),
        "crontab" => ro_unless(
            !(has_flag(args, &["-l"])
                && !has_flag(args, &["-r", "-e", "-i"])
                && positionals(args, &["-u"]).is_empty()),
            System,
            "crontab 会改定时任务",
        ),
        "chage" => ro_unless(
            !has_flag(args, &["-l", "--list"]),
            System,
            "chage 会改账号密码策略",
        ),
        "chronyc" => sub_allow(
            args,
            &[
                "tracking",
                "sources",
                "sourcestats",
                "activity",
                "ntpdata",
                "clients",
                "serverstats",
            ],
            System,
            prog,
        ),
        "rpm" => ro_unless(
            !args.first().is_some_and(|a| {
                a.starts_with("-q") || a == "--query" || a.starts_with("-V") || a == "--verify"
            }),
            System,
            "rpm 会安装或删除软件包",
        ),
        "dpkg" => ro_unless(
            !has_flag(
                args,
                &[
                    "-l",
                    "--list",
                    "-L",
                    "--listfiles",
                    "-s",
                    "--status",
                    "-S",
                    "--search",
                    "--get-selections",
                    "-p",
                    "--print-avail",
                    "--audit",
                    "-C",
                ],
            ),
            System,
            "dpkg 会安装或删除软件包",
        ),
        "apt" | "apt-cache" | "yum" | "dnf" | "zypper" | "apk" => {
            let sub = positionals(args, &["-c", "-o", "--config-file", "--option"])
                .first()
                .copied()
                .unwrap_or("");
            const RO: &[&str] = &[
                "list",
                "show",
                "search",
                "info",
                "policy",
                "depends",
                "rdepends",
                "showpkg",
                "madison",
                "provides",
                "whatprovides",
                "repolist",
                "history",
                "check-update",
                "stats",
                "dump",
            ];
            ro_unless(
                !RO.contains(&sub),
                System,
                &format!("{prog} {sub} 会改动软件包"),
            )
        }
        "git" => {
            let sub = args.first().map(String::as_str).unwrap_or("");
            let rest = &args[1.min(args.len())..];
            let ok = match sub {
                "status" | "log" | "diff" | "show" | "rev-parse" | "describe" | "blame"
                | "ls-files" | "shortlog" => true,
                "remote" => rest.is_empty() || rest.iter().all(|a| a == "-v" || a == "--verbose"),
                "branch" | "tag" => {
                    positionals(rest, &[]).is_empty() || has_flag(rest, &["-l", "--list"])
                }
                "config" => has_flag(
                    rest,
                    &["--get", "--list", "-l", "--get-all", "--get-regexp"],
                ),
                _ => false,
            };
            if ok {
                SegClass::Kind(Disk)
            } else {
                unk(format!("git {sub} 需要确认"))
            }
        }

        _ => unk(format!("不认识 {prog}，需要确认")),
    }
}

fn file_reader(positional_count: usize) -> SegClass {
    if positional_count == 0 {
        SegClass::Neutral
    } else {
        SegClass::Kind(Logs)
    }
}

fn classify_words(words: &[String], depth: usize) -> SegClass {
    if words.is_empty() {
        return SegClass::Neutral;
    }
    if depth > MAX_DEPTH {
        return unk("命令嵌套太深");
    }
    classify_segment(words, depth + 1)
}

fn wrapped(args: &[String], takes_value: &[&str], depth: usize, prog: &str) -> SegClass {
    let rest = skip_opts(args, takes_value);
    if rest.is_empty() {
        return unk(format!("{prog} 后面缺少命令"));
    }
    classify_words(&rest, depth)
}

fn sub_allow(args: &[String], allowed: &[&str], kind: ReadOnlyKind, prog: &str) -> SegClass {
    let sub = positionals(args, &[]).first().copied().unwrap_or("");
    if allowed.contains(&sub)
        || (sub.is_empty() && has_flag(args, &["--version", "-v", "--help", "-h"]))
    {
        SegClass::Kind(kind)
    } else {
        mut_(format!("{prog} {sub} 可能改动状态"))
    }
}

fn classify_systemctl(args: &[String]) -> SegClass {
    let pos = positionals(
        args,
        &[
            "-H",
            "--host",
            "-M",
            "--machine",
            "-p",
            "--property",
            "-t",
            "--type",
            "--state",
            "-n",
            "--lines",
            "-o",
            "--output",
        ],
    );
    let sub = pos.first().copied().unwrap_or("list-units");
    const RO: &[&str] = &[
        "status",
        "show",
        "cat",
        "list-units",
        "list-unit-files",
        "list-sockets",
        "list-timers",
        "list-jobs",
        "list-dependencies",
        "list-machines",
        "is-active",
        "is-enabled",
        "is-failed",
        "is-system-running",
        "get-default",
        "help",
        "show-environment",
    ];
    if RO.contains(&sub) {
        SegClass::Kind(Processes)
    } else {
        mut_(format!("systemctl {sub} 会改动服务"))
    }
}

fn classify_docker(args: &[String]) -> SegClass {
    let pos = positionals(
        args,
        &[
            "-H",
            "--host",
            "--context",
            "-c",
            "--config",
            "-l",
            "--log-level",
            "-f",
            "--filter",
            "--format",
            "-n",
            "--tail",
            "--since",
            "--until",
        ],
    );
    let sub = pos.first().copied().unwrap_or("");
    let sub2 = pos.get(1).copied().unwrap_or("");
    let ok = match sub {
        "ps" | "logs" | "inspect" | "stats" | "top" | "images" | "info" | "version" | "port"
        | "diff" | "history" | "events" | "search" => true,
        "container" | "image" | "network" | "volume" | "node" | "service" | "stack" | "context"
        | "plugin" | "secret" | "config" => [
            "ls", "list", "ps", "inspect", "logs", "top", "stats", "port", "diff", "history",
        ]
        .contains(&sub2),
        "system" => ["df", "info", "events"].contains(&sub2),
        "compose" => [
            "ps", "logs", "config", "top", "images", "ls", "version", "port",
        ]
        .contains(&sub2),
        "" => has_flag(args, &["--version", "-v"]),
        _ => false,
    };
    if ok {
        SegClass::Kind(Processes)
    } else {
        mut_(format!(
            "docker {} 会改动容器或镜像",
            pos.iter().take(2).copied().collect::<Vec<_>>().join(" ")
        ))
    }
}

fn classify_kubectl(args: &[String], prog: &str) -> SegClass {
    let pos = positionals(
        args,
        &[
            "-n",
            "--namespace",
            "--context",
            "--kubeconfig",
            "-l",
            "--selector",
            "-o",
            "--output",
            "-c",
            "--container",
            "--tail",
            "--since",
            "-s",
            "--server",
            "--cluster",
            "--user",
            "--field-selector",
            "--sort-by",
        ],
    );
    let sub = pos.first().copied().unwrap_or("");
    let sub2 = pos.get(1).copied().unwrap_or("");
    let ok = match sub {
        "get" | "describe" | "logs" | "top" | "explain" | "version" | "cluster-info"
        | "api-resources" | "api-versions" | "events" => true,
        "config" => ["view", "get-contexts", "current-context", "get-clusters"].contains(&sub2),
        "auth" => sub2 == "can-i",
        "rollout" => ["status", "history"].contains(&sub2),
        _ => false,
    };
    if ok && !has_flag(args, &["-w", "--watch"]) {
        SegClass::Kind(Processes)
    } else if ok {
        unk(format!("{prog} --watch 会一直运行"))
    } else {
        mut_(format!("{prog} {sub} 会改动集群"))
    }
}

fn classify_iptables(args: &[String]) -> SegClass {
    // 只允许 -L/-S 查看，以及 -n -v -x -t <table> --line-numbers 这些显示选项
    let mut listing = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-t" || a == "--table" {
            i += 2;
            continue;
        }
        if a == "--line-numbers"
            || a == "--numeric"
            || a == "--verbose"
            || a == "--exact"
            || a == "--list"
            || a == "--list-rules"
        {
            listing |= a.starts_with("--list");
        } else if let Some(flags) = a
            .strip_prefix('-')
            .filter(|f| !f.starts_with('-') && !f.is_empty())
        {
            if !flags.chars().all(|c| "LSnvx".contains(c)) {
                return mut_(format!("iptables {a} 会改防火墙规则"));
            }
            listing |= flags.contains('L') || flags.contains('S');
        } else if a.starts_with('-') {
            return mut_(format!("iptables {a} 会改防火墙规则"));
        }
        // 其它非选项词是链名
        i += 1;
    }
    if listing {
        SegClass::Kind(Network)
    } else {
        mut_("iptables 没有 -L/-S，可能会改防火墙规则")
    }
}

fn classify_awk(args: &[String]) -> SegClass {
    let mut i = 0;
    let mut script: Option<&str> = None;
    let mut files = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-f" || a == "--file" {
            return unk("awk -f 运行脚本文件，需要确认");
        } else if a == "-F" || a == "-v" || a == "--assign" || a == "--field-separator" {
            i += 1;
        } else if a.starts_with('-') && a.len() > 1 {
            // -F: / -vX=1 之类
        } else if script.is_none() {
            script = Some(a);
        } else if !is_assignment(a) {
            files += 1;
        }
        i += 1;
    }
    let Some(s) = script else {
        return unk("awk 缺少脚本");
    };
    if s.contains("system(")
        || s.contains("| getline")
        || s.contains("|getline")
        || awk_has_output_redirect(s)
    {
        return mut_("awk 脚本里有写文件或执行命令");
    }
    file_reader(files)
}

/// awk 的 print/printf 后面跟 `>`、`>>`、`|`（比较运算 `$3 > 10` 不在 print 后面，不算）。
fn awk_has_output_redirect(script: &str) -> bool {
    let mut in_str = false;
    let mut after_print = false;
    let chars: Vec<char> = script.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            if c == '\\' {
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                ';' | '}' | '{' | '\n' => after_print = false,
                '>' | '|' if after_print => {
                    // 在 print 的括号参数里的比较：print ($1 > 2) 很少见，这里保守算写入
                    if c == '|' && chars.get(i + 1) == Some(&'|') {
                        i += 2;
                        continue;
                    }
                    return true;
                }
                _ => {
                    if script[byte_idx(&chars, i)..].starts_with("print") {
                        after_print = true;
                    }
                }
            }
        }
        i += 1;
    }
    false
}

fn byte_idx(chars: &[char], i: usize) -> usize {
    chars[..i].iter().map(|c| c.len_utf8()).sum()
}

fn classify_sed(args: &[String]) -> SegClass {
    let mut scripts: Vec<&str> = Vec::new();
    let mut files = 0;
    let mut explicit_script = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-i"
            || a.starts_with("-i")
            || a.starts_with("--in-place")
            || (a.starts_with('-') && !a.starts_with("--") && a[1..].contains('i') && a.len() <= 4)
        {
            return mut_("sed -i 会直接改文件");
        }
        if a == "-e" || a == "--expression" {
            explicit_script = true;
            if let Some(s) = args.get(i + 1) {
                scripts.push(s);
            }
            i += 2;
            continue;
        }
        if a == "-f" || a == "--file" {
            return unk("sed -f 运行脚本文件，需要确认");
        }
        if a.starts_with('-') && a.len() > 1 {
            i += 1;
            continue;
        }
        if !explicit_script && scripts.is_empty() {
            scripts.push(a);
        } else {
            files += 1;
        }
        i += 1;
    }
    for s in &scripts {
        if sed_script_writes(s) {
            return mut_("sed 脚本里有写文件（w）或执行命令（e）");
        }
    }
    file_reader(files)
}

/// 检查 sed 脚本里的 w/W/e 命令和 s///w、s///e 标志（地址 `/re/`、`1,5`、`$` 会先跳过）。
fn sed_script_writes(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let skip_ws = |i: &mut usize| {
        while *i < chars.len() && (chars[*i] == ' ' || chars[*i] == '\t') {
            *i += 1;
        }
    };
    // 跳过一个分隔符包住的部分，返回分隔符后的位置
    let skip_delim = |mut i: usize, d: char| -> usize {
        while i < chars.len() {
            if chars[i] == '\\' {
                i += 2;
                continue;
            }
            if chars[i] == d {
                return i + 1;
            }
            i += 1;
        }
        i
    };
    while i < chars.len() {
        skip_ws(&mut i);
        // 地址（最多两个）
        for _ in 0..2 {
            if i < chars.len() && chars[i] == '/' {
                i = skip_delim(i + 1, '/');
            } else if i < chars.len() && chars[i] == '\\' && i + 1 < chars.len() {
                let d = chars[i + 1];
                i = skip_delim(i + 2, d);
            } else {
                while i < chars.len()
                    && (chars[i].is_ascii_digit()
                        || chars[i] == '$'
                        || chars[i] == '~'
                        || chars[i] == '+')
                {
                    i += 1;
                }
            }
            if i < chars.len() && chars[i] == 'I' {
                i += 1;
            }
            if i < chars.len() && chars[i] == ',' {
                i += 1;
            } else {
                break;
            }
        }
        skip_ws(&mut i);
        while i < chars.len() && (chars[i] == '!' || chars[i] == ' ') {
            i += 1;
        }
        let Some(&c) = chars.get(i) else { break };
        match c {
            'w' | 'W' | 'e' => return true,
            's' | 'y' => {
                let Some(&d) = chars.get(i + 1) else {
                    return false;
                };
                let mut j = skip_delim(i + 2, d);
                j = skip_delim(j, d);
                if c == 's' {
                    while j < chars.len() && !matches!(chars[j], ';' | '\n' | '}') {
                        if chars[j] == 'w' || chars[j] == 'e' {
                            return true;
                        }
                        j += 1;
                    }
                }
                i = j;
            }
            '{' | '}' | ';' | '\n' => i += 1,
            'a' | 'i' | 'c' | 'r' | 'R' | 'b' | 't' | 'T' | ':' => {
                // 带文本或标签的命令：读到行尾
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            _ => {
                i += 1;
                skip_ws(&mut i);
                if i < chars.len() && chars[i] == ';' {
                    i += 1;
                }
            }
        }
    }
    false
}

fn classify_find(args: &[String], depth: usize) -> SegClass {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-delete" | "-fprint" | "-fprint0" | "-fprintf" | "-fls" => {
                return mut_(format!("find {a} 会删除或写文件"))
            }
            "-ok" | "-okdir" => return unk("find -ok 会逐个执行命令，需要确认"),
            "-exec" | "-execdir" => {
                let mut j = i + 1;
                let mut inner = Vec::new();
                while j < args.len() && args[j] != ";" && args[j] != "+" {
                    inner.push(args[j].clone());
                    j += 1;
                }
                match classify_words(&inner, depth) {
                    SegClass::Mutating(r) => return SegClass::Mutating(r),
                    SegClass::Unknown(r) => return SegClass::Unknown(r),
                    _ => {}
                }
                i = j;
            }
            _ => {}
        }
        i += 1;
    }
    SegClass::Kind(Disk)
}

fn classify_curl(args: &[String]) -> SegClass {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let next = args.get(i + 1).map(String::as_str).unwrap_or("");
        if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            let val = inline.unwrap_or(next);
            let consumed = if inline.is_some() { 1 } else { 2 };
            match name {
                "data" | "data-raw" | "data-binary" | "data-urlencode" | "data-ascii" | "json"
                | "form" | "form-string" | "upload-file" | "post301" | "post302" | "post303" => {
                    return mut_("curl 会向服务器提交数据")
                }
                "request" | "X" => {
                    if !is_safe_method(val) {
                        return mut_(format!("curl -X {val} 会改动远端数据"));
                    }
                    i += consumed;
                    continue;
                }
                "output" | "dump-header" | "cookie-jar" | "trace" | "trace-ascii" | "stderr"
                | "etag-save" | "hsts" | "alt-svc" | "libcurl" => {
                    if !is_harmless_redirect_target(val) {
                        return mut_(format!("curl 会把结果写到文件 {val}"));
                    }
                    i += consumed;
                    continue;
                }
                "remote-name" | "remote-name-all" | "remote-header-name" | "create-dirs"
                | "output-dir" => return mut_("curl 会下载文件到磁盘"),
                "config" => return unk("curl -K 读取配置文件，需要确认"),
                _ => {
                    // 带值的常见只读选项
                    if inline.is_none()
                        && [
                            "header",
                            "user-agent",
                            "max-time",
                            "connect-timeout",
                            "resolve",
                            "connect-to",
                            "write-out",
                            "cacert",
                            "cert",
                            "key",
                            "user",
                            "proxy",
                            "interface",
                            "retry",
                            "retry-delay",
                            "retry-max-time",
                            "url",
                            "referer",
                            "cookie",
                            "range",
                            "unix-socket",
                            "cert-type",
                            "key-type",
                            "limit-rate",
                            "max-redirs",
                            "noproxy",
                            "proxy-user",
                            "dns-servers",
                            "expect100-timeout",
                            "speed-time",
                            "speed-limit",
                        ]
                        .contains(&name)
                    {
                        i += 2;
                        continue;
                    }
                }
            }
            i += 1;
            continue;
        }
        if a.starts_with('-') && a.len() > 1 {
            // 组合短选项：-sSfL、-o/dev/null、-XPOST、-w '%{http_code}'
            let flags: Vec<char> = a[1..].chars().collect();
            let mut k = 0;
            let mut consumed_next = false;
            while k < flags.len() {
                let f = flags[k];
                let attached: String = flags[k + 1..].iter().collect();
                let val = if attached.is_empty() {
                    consumed_next = true;
                    next.to_string()
                } else {
                    attached.clone()
                };
                match f {
                    'd' | 'F' | 'T' => return mut_("curl 会向服务器提交数据"),
                    'O' | 'J' => return mut_("curl 会下载文件到磁盘"),
                    'K' => return unk("curl -K 读取配置文件，需要确认"),
                    'X' => {
                        if !is_safe_method(&val) {
                            return mut_(format!("curl -X {val} 会改动远端数据"));
                        }
                        break;
                    }
                    'o' | 'D' | 'c' => {
                        if !is_harmless_redirect_target(&val) {
                            return mut_(format!("curl 会把结果写到文件 {val}"));
                        }
                        break;
                    }
                    'H' | 'A' | 'm' | 'w' | 'u' | 'x' | 'e' | 'b' | 'r' | 'E' | 'U' | 'Y' | 'y'
                    | 'z' | 'C' | 'Q' | 't' => {
                        if f == 'Q' {
                            return unk("curl -Q 会发送 FTP 命令，需要确认");
                        }
                        break;
                    }
                    _ => {
                        consumed_next = false;
                        k += 1;
                        continue;
                    }
                }
            }
            i += if consumed_next && k < flags.len() {
                2
            } else {
                1
            };
            continue;
        }
        // URL：只认 http/https；其它协议（ftp、smtp、file、dict、gopher…）需要确认
        let lower = a.to_ascii_lowercase();
        if lower.contains("://") && !(lower.starts_with("http://") || lower.starts_with("https://"))
        {
            return unk(format!("curl 访问非 HTTP 地址 {a}，需要确认"));
        }
        i += 1;
    }
    SegClass::Kind(Network)
}

fn is_safe_method(m: &str) -> bool {
    matches!(m.to_ascii_uppercase().as_str(), "GET" | "HEAD" | "OPTIONS")
}

fn classify_wget(args: &[String]) -> SegClass {
    if has_flag(
        args,
        &[
            "--post-data",
            "--post-file",
            "--body-data",
            "--body-file",
            "--method",
        ],
    ) {
        return mut_("wget 会向服务器提交数据");
    }
    let spider = has_flag(args, &["--spider"]);
    let to_stdout = args
        .windows(2)
        .any(|w| (w[0] == "-O" || w[0] == "--output-document") && w[1] == "-")
        || args.iter().any(|a| {
            a == "--output-document=-"
                || a == "-O-"
                || (a.starts_with('-') && !a.starts_with("--") && a.ends_with("O-"))
        })
        || args.windows(2).any(|w| {
            w[0].starts_with('-') && !w[0].starts_with("--") && w[0].ends_with('O') && w[1] == "-"
        });
    let writes_log = has_flag(args, &["-o", "--output-file", "-a", "--append-output"]);
    if (spider || to_stdout) && !writes_log {
        SegClass::Kind(Network)
    } else {
        mut_("wget 会下载文件到磁盘")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ro(cmd: &str) -> BTreeSet<ReadOnlyKind> {
        match classify_command(cmd) {
            CommandClass::ReadOnly { kinds } => kinds,
            other => panic!("expected read-only for {cmd:?}, got {other:?}"),
        }
    }

    fn assert_mut(cmd: &str) {
        let c = classify_command(cmd);
        assert!(c.is_mutating(), "expected mutating for {cmd:?}, got {c:?}");
    }

    fn assert_unknown(cmd: &str) {
        let c = classify_command(cmd);
        assert!(
            matches!(c, CommandClass::Unknown { .. }),
            "expected unknown for {cmd:?}, got {c:?}"
        );
    }

    #[test]
    fn common_troubleshooting_commands_are_read_only() {
        let cmds = [
            // 日志
            "tail -n 200 /var/log/nginx/error.log",
            "tail -f /var/log/messages",
            "journalctl -u nginx --since '1 hour ago' --no-pager",
            "journalctl -xe | tail -50",
            "dmesg -T | tail",
            "grep -i error /var/log/syslog | tail -20",
            "zgrep 'Out of memory' /var/log/messages*",
            "cat /etc/nginx/nginx.conf",
            "less /var/log/app.log",
            "head -100 /var/log/app.log",
            "awk '{print $1}' /var/log/nginx/access.log | sort | uniq -c | sort -rn | head",
            "sed -n '100,200p' /var/log/app.log",
            "sed -n '/ERROR/p' /var/log/app.log",
            "sed -n '/warn/p' /var/log/app.log",
            "last -n 20",
            // 进程 / 服务
            "ps aux --sort=-%mem | head -20",
            "ps -ef | grep java | grep -v grep",
            "top -b -n 1 | head -30",
            "pgrep -af nginx",
            "lsof -i :8080",
            "systemctl status nginx",
            "systemctl --failed",
            "systemctl list-units --type=service --state=running",
            "systemctl is-active mysqld",
            "service nginx status",
            "docker ps -a",
            "docker logs --tail 100 web",
            "docker stats --no-stream",
            "docker compose ps",
            "kubectl get pods -n prod -o wide",
            "kubectl describe pod web-1",
            "kubectl logs deploy/web --tail=100",
            "supervisorctl status",
            // 磁盘
            "df -h",
            "df -i",
            "du -sh /var/log/* | sort -h | tail",
            "ls -lah /data",
            "lsblk",
            "find /var/log -name '*.log' -size +100M",
            "find / -xdev -type f -size +1G -exec ls -lh {} \\;",
            "mount | grep data",
            "stat /etc/passwd",
            // 网络
            "ss -tlnp",
            "netstat -anp | grep 3306",
            "ping -c 4 10.0.0.1",
            "dig example.com +short",
            "ip addr",
            "ip route show",
            "iptables -L -n -v",
            "iptables -t nat -S",
            "curl -I https://example.com",
            "curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/health",
            "curl -sSf http://localhost/healthz",
            "curl -X GET http://localhost:9200/_cluster/health?pretty",
            "curl -m 5 --connect-timeout 2 -H 'Host: a.com' http://127.0.0.1/",
            "wget -qO- http://127.0.0.1/health",
            "wget --spider http://127.0.0.1/",
            "nc -zv 10.0.0.2 22",
            "openssl s_client -connect example.com:443 </dev/null",
            // 系统
            "uptime",
            "free -m",
            "uname -a",
            "vmstat 1 5",
            "cat /proc/loadavg",
            "sysctl net.ipv4.ip_forward",
            "crontab -l",
            "date '+%F %T'",
            "hostname",
            "timedatectl",
            "rpm -qa | grep nginx",
            "dpkg -l | grep openssl",
            "env | grep PATH",
            "sudo journalctl -u sshd -n 50",
        ];
        for c in cmds {
            ro(c);
        }
    }

    #[test]
    fn loops_comments_substitutions_and_pipes() {
        assert_eq!(
            ro("for f in /var/log/*.log; do tail -n 5 \"$f\"; done"),
            BTreeSet::from([ReadOnlyKind::Logs])
        );
        ro("for i in 1 2 3\ndo\n  ping -c1 10.0.0.$i\ndone");
        ro("while true; do uptime; sleep 1; done");
        ro("for ((i=0;i<3;i++)); do free -m; done");
        ro("if systemctl is-active nginx; then echo ok; else echo down; fi");
        ro("# 看看磁盘\ndf -h # 根分区");
        ro("df -h   # rm -rf / 只是注释");
        ro("ls -l $(dirname /var/log/app.log)");
        ro("echo `hostname` && uptime");
        ro("diff <(sort a.txt) <(sort b.txt)");
        ro("ps aux | grep 'rm -rf' | grep -v grep");
        ro("grep 'service restart' /var/log/app.log");
        ro("journalctl -u app 2>&1 | grep -E 'kill |reboot'");
        ro("cat /var/log/app.log 2>/dev/null | wc -l");
        ro("ls /nonexistent > /dev/null 2>&1; echo $?");
        ro("timeout 5 curl -s http://127.0.0.1/health");
        ro("watch -n 1 'df -h'");
        ro("bash -c 'df -h && free -m'");
        ro("echo $((1+2))");
        ro("x=$(cat /proc/loadavg); echo $x");
        ro("ls /etc/{nginx,httpd}");
        ro("find /var/log -type f | xargs ls -lh");
        ro("cat a.log | xargs -n1 echo");
        ro("df -h; \\\n free -m");
    }

    #[test]
    fn mutating_commands_are_detected() {
        let cmds = [
            "rm -rf /tmp/x",
            "sudo rm /var/log/old.log",
            "systemctl restart nginx",
            "sudo systemctl stop mysqld",
            "systemctl enable --now app",
            "service nginx restart",
            "echo 3 > /proc/sys/vm/drop_caches",
            "echo 'alias ll=ls' >> ~/.bashrc",
            "df -h > /tmp/df.txt",
            "uptime &> /tmp/x",
            "ps aux | tee /tmp/ps.txt",
            "sed -i 's/a/b/' /etc/app.conf",
            "sed -n 'w /tmp/out' /etc/passwd",
            "sed 's/a/b/w /tmp/out' file",
            "find /tmp -name '*.tmp' -delete",
            "find /tmp -type f -exec rm {} \\;",
            "ls /tmp/*.log | xargs rm -f",
            "curl -X POST http://localhost/api/reload",
            "curl -d 'a=1' http://localhost/api",
            "curl -XDELETE http://localhost:9200/index",
            "curl --request=PUT http://x/",
            "curl -o /tmp/file http://x/file",
            "curl -O http://x/file.tar.gz",
            "curl -sSLo app.tar.gz http://x/app.tar.gz",
            "curl --json '{}' http://x/",
            "wget http://x/file",
            "docker restart web",
            "docker rm -f web",
            "docker exec web sh",
            "kubectl delete pod web-1",
            "kubectl scale deploy web --replicas=0",
            "iptables -F",
            "iptables -A INPUT -p tcp --dport 22 -j ACCEPT",
            "ip link set eth0 down",
            "ip addr add 10.0.0.5/24 dev eth0",
            "sysctl -w net.ipv4.ip_forward=1",
            "sysctl net.ipv4.ip_forward=1",
            "mount /dev/sdb1 /mnt",
            "journalctl --vacuum-time=2d",
            "crontab -r",
            "awk '{print $1 > \"/tmp/out\"}' access.log",
            "awk 'BEGIN{system(\"reboot\")}'",
            "yum install -y nginx",
            "apt-get remove nginx",
            "date -s '2020-01-01'",
            "hostname newname",
            "kill -9 1234",
            "pkill nginx",
            "reboot",
            "for f in *.log; do rm \"$f\"; done",
            "df -h && systemctl restart nginx",
            "ls $(rm -rf /tmp/x)",
            "watch 'systemctl restart app'",
            "bash -c 'rm -rf /tmp/x'",
            "fuser -k 8080/tcp",
            "dmesg -c",
            "pm2 restart all",
        ];
        for c in cmds {
            let class = classify_command(c);
            if matches!(
                c,
                "kill -9 1234"
                    | "pkill nginx"
                    | "reboot"
                    | "apt-get remove nginx"
                    | "docker exec web sh"
            ) {
                // 这些不在只读名单里：至少不能是只读
                assert!(
                    !class.is_read_only(),
                    "{c:?} must not be read-only, got {class:?}"
                );
                continue;
            }
            assert_mut(c);
        }
    }

    #[test]
    fn unknown_or_broken_commands_need_confirmation() {
        assert_unknown("my-script.sh --check");
        assert_unknown("./deploy.sh");
        assert_unknown("python3 -c 'print(1)'");
        assert_unknown("echo 'unclosed");
        assert_unknown("cat <<EOF\nhello\nEOF");
        assert_unknown("eval \"$CMD\"");
        assert_unknown("source /etc/profile");
        assert_unknown("top");
        assert_unknown("curl -K /tmp/cfg http://x/");
        assert_unknown("curl ftp://example.com/");
        assert_unknown("");
        assert_unknown("   # 只有注释");
        assert_unknown("nc 10.0.0.1 80");
    }

    #[test]
    fn kinds_are_grouped() {
        assert_eq!(ro("df -h"), BTreeSet::from([ReadOnlyKind::Disk]));
        assert_eq!(ro("tail /var/log/x"), BTreeSet::from([ReadOnlyKind::Logs]));
        assert_eq!(
            ro("curl -sI http://x/"),
            BTreeSet::from([ReadOnlyKind::Network])
        );
        assert_eq!(
            ro("systemctl status a"),
            BTreeSet::from([ReadOnlyKind::Processes])
        );
        assert_eq!(
            ro("uptime; free -m"),
            BTreeSet::from([ReadOnlyKind::System])
        );
        assert_eq!(ro("echo hi"), BTreeSet::from([ReadOnlyKind::Basic]));
        assert_eq!(
            ro("df -h && ps aux"),
            BTreeSet::from([ReadOnlyKind::Disk, ReadOnlyKind::Processes])
        );
        // grep 在管道里不改变类别
        assert_eq!(
            ro("ps aux | grep nginx"),
            BTreeSet::from([ReadOnlyKind::Processes])
        );
    }

    #[test]
    fn session_trust_only_covers_same_kind_read_only() {
        let trusted = BTreeSet::from([ReadOnlyKind::Logs, ReadOnlyKind::Disk]);
        let ok = |c: &str| readonly_auto_run_allowed(&classify_command(c), &trusted);
        assert!(ok("tail -n 100 /var/log/nginx/error.log"));
        assert!(ok("df -h && du -sh /var/log"));
        assert!(ok("grep -c ERROR /var/log/app.log | sort"));
        // 其它类别的只读命令仍要问一次
        assert!(!ok("ps aux"));
        assert!(!ok("df -h; ps aux"));
        assert!(!ok("curl -sI http://127.0.0.1/"));
        // 改动命令和看不懂的命令永远不自动执行
        assert!(!ok("rm -rf /var/log/old"));
        assert!(!ok("tail /var/log/x > /tmp/copy"));
        assert!(!ok("systemctl restart nginx"));
        assert!(!ok("./cleanup.sh"));
        assert!(!readonly_auto_run_allowed(
            &classify_command("df -h"),
            &BTreeSet::new()
        ));
        let all: BTreeSet<_> = [
            ReadOnlyKind::Logs,
            ReadOnlyKind::Processes,
            ReadOnlyKind::Disk,
            ReadOnlyKind::Network,
            ReadOnlyKind::System,
            ReadOnlyKind::Basic,
        ]
        .into();
        assert!(!readonly_auto_run_allowed(
            &classify_command("rm -f /tmp/x"),
            &all
        ));
        assert!(!readonly_auto_run_allowed(
            &classify_command("echo x >> ~/.bashrc"),
            &all
        ));
    }

    #[test]
    fn redirect_parsing_edge_cases() {
        ro("ls 2>/dev/null");
        ro("ls 2> /dev/null");
        ro("ls >/dev/null 2>&1");
        ro("ls 1>&2");
        ro("grep x < /var/log/app.log");
        ro("echo 'a > b'");
        ro("echo \"a > b\"");
        ro("awk '$3 > 80 {print $1}' /var/log/x");
        assert_mut("ls 2>/tmp/err");
        assert_mut("echo x >| /etc/x");
        assert_mut("echo x 1>>/var/log/x");
    }
}
