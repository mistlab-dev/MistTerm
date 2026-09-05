//! 命令片段智能推荐（命令历史 + 执行日志，纯本地）。

use std::collections::HashMap;

use crate::core::command_history::CommandHistory;
use crate::core::FragmentStats;

#[derive(Debug, Clone)]
pub struct FragmentRecommendation {
    pub command: String,
    pub count: u32,
    pub source: &'static str,
}

/// 审计拦截后的合规替代建议（团队优先）。
#[derive(Debug, Clone)]
pub struct CompliantFragmentSuggestion {
    pub fragment: FragmentStats,
    /// `"team"` / `"personal"`
    pub source: &'static str,
}

/// 当前会话环境：用于按主机/环境标签过滤推荐；无匹配时回退全局。
#[derive(Debug, Clone, Default)]
pub struct SuggestionEnvContext {
    pub host: String,
    pub color_tag: String,
    pub env_tags: Vec<String>,
}

impl SuggestionEnvContext {
    pub fn from_session(host: &str, color_tag: &str, env_tags: &[String]) -> Self {
        Self {
            host: host.trim().to_string(),
            color_tag: color_tag.trim().to_string(),
            env_tags: env_tags.to_vec(),
        }
    }

    /// 用于匹配的小写标签集合（host 短名、color_tag、TeamServer tags）。
    pub fn effective_match_tags(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let push_unique = |out: &mut Vec<String>, raw: &str| {
            let t = raw.trim().to_lowercase();
            if t.is_empty() {
                return;
            }
            if !out.iter().any(|x| x == &t) {
                out.push(t);
            }
        };
        for t in &self.env_tags {
            push_unique(&mut out, t);
        }
        if !self.color_tag.is_empty() {
            push_unique(&mut out, &self.color_tag);
        }
        if !self.host.is_empty() {
            let host = self.host.split(':').next().unwrap_or(&self.host);
            push_unique(&mut out, host);
            if let Some(short) = host.split('.').next() {
                push_unique(&mut out, short);
            }
        }
        out
    }

    pub fn fragment_matches(&self, f: &FragmentStats) -> bool {
        let tags = self.effective_match_tags();
        if tags.is_empty() {
            return true;
        }
        let frag_tags_lower: Vec<String> = f.tags.iter().map(|t| t.to_lowercase()).collect();
        let hay = format!(
            "{} {} {}",
            f.tags.join(" "),
            f.category,
            f.title
        )
        .to_lowercase();
        tags.iter().any(|t| {
            frag_tags_lower.iter().any(|ft| ft == t || ft.contains(t) || t.contains(ft.as_str()))
                || hay.contains(t)
        })
    }
}

/// 入库候选原因（必须经用户确认，禁止静默写入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentCandidateReason {
    FailedPath,
    SuccessPath,
}

impl FragmentCandidateReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FailedPath => "failed_path",
            Self::SuccessPath => "success_path",
        }
    }
}

/// 待用户确认后写入个人库的片段候选。
#[derive(Debug, Clone)]
pub struct FragmentCandidate {
    pub command: String,
    pub title: String,
    pub reason: FragmentCandidateReason,
}

impl FragmentCandidate {
    pub fn from_command(command: &str, reason: FragmentCandidateReason) -> Self {
        let cmd = normalize_command(command);
        let title: String = cmd.chars().take(40).collect();
        Self {
            command: cmd,
            title,
            reason,
        }
    }
}

/// 若命令尚未被个人库覆盖，生成失败路径入库候选。
pub fn candidate_from_failed_command(
    command: &str,
    personal_fragments: &[FragmentStats],
) -> Option<FragmentCandidate> {
    let n = normalize_command(command);
    if n.is_empty() || is_trivial_command(&n) {
        return None;
    }
    if covered_by_library(&n, personal_fragments) {
        return None;
    }
    Some(FragmentCandidate::from_command(&n, FragmentCandidateReason::FailedPath))
}

/// 成功路径入库：仅当「刚执行的这条」累计次数**刚好达到**阈值时提示（避免每敲一条都弹最热门命令）。
pub const SUCCESS_CANDIDATE_MIN_COUNT: u32 = 5;

/// 将历史频次推荐转为需确认的成功路径候选（取第一条）。
pub fn candidate_from_success_recommendation(
    rec: &FragmentRecommendation,
) -> FragmentCandidate {
    FragmentCandidate {
        command: rec.command.clone(),
        title: rec.command.chars().take(40).collect(),
        reason: FragmentCandidateReason::SuccessPath,
    }
}

/// 仅针对刚执行的命令：历史中出现次数刚好等于 `min_count` 且未入库时，生成成功路径候选。
pub fn candidate_from_just_ran_success(
    history: &CommandHistory,
    personal_fragments: &[FragmentStats],
    just_ran: &str,
    min_count: u32,
) -> Option<FragmentCandidate> {
    let n = normalize_command(just_ran);
    if n.is_empty() || is_trivial_command(&n) {
        return None;
    }
    if covered_by_library(&n, personal_fragments) {
        return None;
    }
    let count = history
        .entries_newest_first()
        .filter(|e| normalize_command(&e.command) == n)
        .count() as u32;
    // 刚好达标才弹一次；之后再跑不再因「最热门」反复打扰
    if count != min_count {
        return None;
    }
    Some(FragmentCandidate {
        command: n.clone(),
        title: n.chars().take(40).collect(),
        reason: FragmentCandidateReason::SuccessPath,
    })
}

/// 从被拦截命令推断主题关键词，并在片段库中打分取 Top1。
/// 刻意排除与拦截命令相同/明显同危的片段，避免「推荐再执行一遍危险命令」。
/// `env` 非空时优先按主机/环境标签过滤；无匹配则回退全局库。
pub fn suggest_compliant_after_block(
    blocked_command: &str,
    team_fragments: &[FragmentStats],
    personal_fragments: &[FragmentStats],
) -> Option<CompliantFragmentSuggestion> {
    suggest_compliant_after_block_with_env(
        blocked_command,
        team_fragments,
        personal_fragments,
        None,
    )
}

pub fn suggest_compliant_after_block_with_env(
    blocked_command: &str,
    team_fragments: &[FragmentStats],
    personal_fragments: &[FragmentStats],
    env: Option<&SuggestionEnvContext>,
) -> Option<CompliantFragmentSuggestion> {
    let keywords = block_topic_keywords(blocked_command);
    if keywords.is_empty() {
        return None;
    }
    let blocked_norm = normalize_command(blocked_command);

    let team_scoped = scope_fragments(team_fragments, env);
    let personal_scoped = scope_fragments(personal_fragments, env);

    if let Some(f) = best_compliant_match(&keywords, &blocked_norm, &team_scoped) {
        return Some(CompliantFragmentSuggestion {
            fragment: f,
            source: "team",
        });
    }
    best_compliant_match(&keywords, &blocked_norm, &personal_scoped).map(|f| {
        CompliantFragmentSuggestion {
            fragment: f,
            source: "personal",
        }
    })
}

fn scope_fragments(
    fragments: &[FragmentStats],
    env: Option<&SuggestionEnvContext>,
) -> Vec<FragmentStats> {
    let Some(env) = env else {
        return fragments.to_vec();
    };
    if env.effective_match_tags().is_empty() {
        return fragments.to_vec();
    }
    let filtered: Vec<FragmentStats> = fragments
        .iter()
        .filter(|f| env.fragment_matches(f))
        .cloned()
        .collect();
    if filtered.is_empty() {
        fragments.to_vec()
    } else {
        filtered
    }
}

/// 自然语言/拦截主题共用的关键词抽取（供 knowledge 检索）。
pub fn query_topic_keywords(query: &str) -> Vec<String> {
    block_topic_keywords(query)
}

/// 片段相对关键词的打分（供 knowledge 检索公开）。
pub fn score_fragment_against_keywords(f: &FragmentStats, keywords: &[String]) -> i64 {
    score_fragment_keywords(f, keywords)
}

fn block_topic_keywords(blocked: &str) -> Vec<String> {
    let lower = blocked.to_lowercase();
    let mut keys: Vec<String> = Vec::new();

    // 常见危险模式 → 运维主题词（中英），便于命中「清理日志」等合规片段
    if lower.contains("rm")
        && (lower.contains("-rf")
            || lower.contains("-fr")
            || lower.split_whitespace().any(|t| t == "-r" || t == "-f"))
    {
        for k in [
            "clean", "cleanup", "log", "logs", "disk", "df", "清理", "日志", "磁盘", "空间",
        ] {
            keys.push(k.to_string());
        }
    }
    if lower.contains("mkfs") || lower.contains("dd if=") || lower.contains("ddof=") {
        for k in ["disk", "partition", "备份", "backup", "磁盘"] {
            keys.push(k.to_string());
        }
    }
    if lower.contains("iptables") || lower.contains("firewall") || lower.contains("ufw") {
        for k in ["firewall", "iptables", "网络", "network", "端口"] {
            keys.push(k.to_string());
        }
    }
    if lower.contains("chmod") && (lower.contains("777") || lower.contains("-r")) {
        for k in ["chmod", "权限", "permission", "secure"] {
            keys.push(k.to_string());
        }
    }

    for tok in tokenize_cmd(blocked) {
        if is_noise_token(&tok) {
            continue;
        }
        if !keys.iter().any(|k| k == &tok) {
            keys.push(tok);
        }
    }
    keys
}

fn tokenize_cmd(cmd: &str) -> Vec<String> {
    cmd.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|s| s.len() >= 2)
        .map(|s| s.to_lowercase())
        .collect()
}

fn is_noise_token(tok: &str) -> bool {
    matches!(
        tok,
        "rm" | "rf"
            | "fr"
            | "sudo"
            | "doas"
            | "bash"
            | "sh"
            | "zsh"
            | "cmd"
            | "exe"
            | "bin"
            | "usr"
            | "dev"
            | "etc"
            | "var"
            | "tmp"
            | "true"
            | "false"
            | "yes"
            | "no"
            | "the"
            | "and"
            | "for"
            | "from"
            | "with"
            | "how"
            | "what"
            | "why"
            | "can"
            | "please"
            // 中文问法噪声（「我们怎么清理」→ 保留「清理」）
            | "我们"
            | "你们"
            | "怎么"
            | "怎样"
            | "如何"
            | "请问"
            | "帮我"
            | "一下"
    ) || tok.chars().all(|c| c == '-')
}

fn best_compliant_match(
    keywords: &[String],
    blocked_norm: &str,
    fragments: &[FragmentStats],
) -> Option<FragmentStats> {
    let mut best: Option<(i64, FragmentStats)> = None;
    for f in fragments {
        let cmd_norm = normalize_command(&f.command);
        if cmd_norm.is_empty() {
            continue;
        }
        // 不要推荐与被拦命令实质相同的内容
        if cmd_norm == blocked_norm
            || blocked_norm.contains(&cmd_norm)
            || cmd_norm.contains(blocked_norm)
        {
            continue;
        }
        if looks_like_same_danger(&f.command, blocked_norm) {
            continue;
        }
        let score = score_fragment_keywords(f, keywords);
        if score <= 0 {
            continue;
        }
        let rank = score * 1000 + i64::from(f.usage_count.min(999));
        match &best {
            None => best = Some((rank, f.clone())),
            Some((r, _)) if rank > *r => best = Some((rank, f.clone())),
            _ => {}
        }
    }
    best.map(|(_, f)| f)
}

fn looks_like_same_danger(candidate: &str, blocked_norm: &str) -> bool {
    let c = candidate.to_lowercase();
    if blocked_norm.contains("rm") && c.contains("rm") && (c.contains("-rf") || c.contains("-fr")) {
        return true;
    }
    false
}

fn score_fragment_keywords(f: &FragmentStats, keywords: &[String]) -> i64 {
    let title = f.title.to_lowercase();
    let command = f.command.to_lowercase();
    let category = f.category.to_lowercase();
    let tags = f.tags.join(" ").to_lowercase();
    let mut score: i64 = 0;
    for k in keywords {
        if k.is_empty() {
            continue;
        }
        if title.contains(k) {
            score += 3;
        }
        if command.contains(k) {
            score += 2;
        }
        if category.contains(k) || tags.contains(k) {
            score += 2;
        }
    }
    score
}

fn normalize_command(cmd: &str) -> String {
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_trivial_command(cmd: &str) -> bool {
    let c = cmd.trim();
    if c.len() < 4 {
        return true;
    }
    let first = c.split_whitespace().next().unwrap_or("");
    matches!(
        first,
        "cd"
            | "ls"
            | "pwd"
            | "clear"
            | "exit"
            | "logout"
            | ":"
            | "history"
            | "echo"
            | "true"
            | "false"
    ) && c.split_whitespace().count() <= 2
}

fn covered_by_library(cmd: &str, fragments: &[FragmentStats]) -> bool {
    let n = normalize_command(cmd);
    fragments.iter().any(|f| {
        let fc = normalize_command(&f.command);
        fc == n || fc.contains(&n) || n.contains(&fc)
    })
}

pub fn recommend_from_history(
    history: &CommandHistory,
    personal_fragments: &[FragmentStats],
    cutoff: Option<i64>,
    limit: usize,
) -> Vec<FragmentRecommendation> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for entry in history.entries_newest_first() {
        if let Some(c) = cutoff {
            if entry.executed_at < c {
                continue;
            }
        }
        let n = normalize_command(&entry.command);
        if n.is_empty() || is_trivial_command(&n) {
            continue;
        }
        if covered_by_library(&n, personal_fragments) {
            continue;
        }
        *counts.entry(n).or_insert(0) += 1;
    }
    let mut rows: Vec<FragmentRecommendation> = counts
        .into_iter()
        .filter(|(_, c)| *c >= 3)
        .map(|(command, count)| FragmentRecommendation {
            command,
            count,
            source: "history",
        })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count));
    rows.truncate(limit);
    rows
}

pub fn merge_recommendations(
    mut a: Vec<FragmentRecommendation>,
    b: Vec<FragmentRecommendation>,
    limit: usize,
) -> Vec<FragmentRecommendation> {
    for item in b {
        if let Some(existing) = a.iter_mut().find(|x| x.command == item.command) {
            existing.count = existing.count.max(item.count);
        } else {
            a.push(item);
        }
    }
    a.sort_by(|x, y| y.count.cmp(&x.count));
    a.truncate(limit);
    a
}

pub fn build_efficiency_report_markdown(
    dash: &crate::core::FragmentAnalyticsDashboard,
    range: crate::core::FragmentAnalyticsTimeRange,
    recommendations: &[FragmentRecommendation],
) -> String {
    let mut out = String::from("# MistTerm 效率报告\n\n");
    out.push_str(&format!(
        "- 时间范围: {}\n- 导出时间: {}\n\n",
        match range {
            crate::core::FragmentAnalyticsTimeRange::AllTime => "全部",
            crate::core::FragmentAnalyticsTimeRange::Last7Days => "近 7 天",
            crate::core::FragmentAnalyticsTimeRange::Last30Days => "近 30 天",
            crate::core::FragmentAnalyticsTimeRange::Last90Days => "近 90 天",
        },
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    ));
    out.push_str("## 汇总\n\n");
    out.push_str(&format!(
        "| 维度 | 执行次数 | 成功率 | 平均耗时 |\n|------|----------|--------|----------|\n| 个人 | {} | {:.0}% | {}ms |\n| 团队 | {} | {:.0}% | {}ms |\n\n",
        dash.personal_total_usage,
        dash.personal_success_rate,
        dash.personal_avg_ms,
        dash.team_total_usage,
        dash.team_success_rate,
        dash.team_avg_ms,
    ));
    if dash.period_stats_from_events {
        out.push_str("> 区间内次数来自本机执行日志。\n\n");
    }
    out.push_str("## 个人 Top 5\n\n");
    for (i, f) in dash.personal_top.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} — {}× · {:.0}% · {}ms\n",
            i + 1,
            f.title,
            f.usage_count,
            f.success_rate(),
            f.avg_time_ms()
        ));
    }
    out.push_str("\n## 团队 Top 5\n\n");
    for (i, f) in dash.team_top.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} — {}× · {:.0}% · {}ms\n",
            i + 1,
            f.title,
            f.usage_count,
            f.success_rate(),
            f.avg_time_ms()
        ));
    }
    if !dash.member_rows.is_empty() {
        out.push_str("\n## 团队成员（本机）\n\n");
        for m in &dash.member_rows {
            let rate = if m.run_count == 0 {
                0.0
            } else {
                (m.success_count as f32 / m.run_count as f32) * 100.0
            };
            out.push_str(&format!(
                "- {} — {}× · {:.0}% OK\n",
                m.display_name, m.run_count, rate
            ));
        }
    }
    if !recommendations.is_empty() {
        out.push_str("\n## 建议添加到片段库\n\n");
        for r in recommendations {
            out.push_str(&format!(
                "- `{}`（{} 次，来源：{}）\n",
                r.command, r.count, r.source
            ));
        }
    }
    out
}

/// 将效率报告渲染为 PDF 字节（需可加载的 CJK TTF/TTC）。
pub fn build_efficiency_report_pdf(
    dash: &crate::core::FragmentAnalyticsDashboard,
    range: crate::core::FragmentAnalyticsTimeRange,
    recommendations: &[FragmentRecommendation],
) -> Result<Vec<u8>, String> {
    let font = load_pdf_cjk_font()?;
    let family = genpdf::fonts::FontFamily {
        regular: font.clone(),
        bold: font.clone(),
        italic: font.clone(),
        bold_italic: font,
    };
    let mut doc = genpdf::Document::new(family);
    doc.set_title("MistTerm Efficiency Report");
    doc.set_line_spacing(1.15);
    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(12);
    doc.set_page_decorator(decorator);

    let md = build_efficiency_report_markdown(dash, range, recommendations);
    for line in md.lines() {
        if line.is_empty() {
            doc.push(genpdf::elements::Break::new(0.6));
        } else {
            doc.push(genpdf::elements::Paragraph::new(line.to_string()));
        }
    }

    let mut buf = Vec::new();
    doc.render(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn load_pdf_cjk_font() -> Result<genpdf::fonts::FontData, String> {
    const EMBEDDED: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-Regular.ttf");
    if let Some(font) = try_font_data(EMBEDDED.to_vec()) {
        log::debug!("PDF export using embedded NotoSansSC-Regular.ttf");
        return Ok(font);
    }
    for path in pdf_cjk_font_paths() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if let Some(font) = try_font_data(bytes) {
            log::info!("PDF export using system font: {}", path.display());
            return Ok(font);
        }
    }
    Err(
        "未找到可用于 PDF 的中文字体（请运行 scripts/fetch-cjk-font.sh 下载嵌入字体）"
            .to_string(),
    )
}

fn try_font_data(bytes: Vec<u8>) -> Option<genpdf::fonts::FontData> {
    genpdf::fonts::FontData::new(bytes, None).ok()
}

fn pdf_cjk_font_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            let fonts = std::path::PathBuf::from(windir).join("Fonts");
            for name in ["msyh.ttc", "msyhbd.ttc", "simhei.ttf", "simsun.ttc"] {
                paths.push(fonts.join(name));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        for p in [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ] {
            paths.push(std::path::PathBuf::from(p));
        }
    }
    #[cfg(target_os = "linux")]
    {
        for p in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        ] {
            paths.push(std::path::PathBuf::from(p));
        }
    }
    paths
}

#[cfg(test)]
mod pdf_tests {
    use super::*;
    use crate::core::FragmentAnalyticsDashboard;

    #[test]
    fn just_ran_success_only_at_exact_threshold() {
        use crate::core::command_history::CommandHistory;
        let mut h = CommandHistory::new();
        // 交错写入避免「连续相同合并」
        for i in 0..4 {
            h.record("ps -ef", Some("s"), Some("n"), true);
            h.record(&format!("echo {i}"), Some("s"), Some("n"), true);
        }
        // 现有 4 次 ps -ef
        assert!(candidate_from_just_ran_success(&h, &[], "ps -ef", 5).is_none());
        h.record("echo x", Some("s"), Some("n"), true);
        h.record("ps -ef", Some("s"), Some("n"), true); // 第 5 次
        let c = candidate_from_just_ran_success(&h, &[], "ps -ef", 5).unwrap();
        assert_eq!(c.command, "ps -ef");
        // 再跑第 6 次不再弹
        h.record("echo y", Some("s"), Some("n"), true);
        h.record("ps -ef", Some("s"), Some("n"), true);
        assert!(candidate_from_just_ran_success(&h, &[], "ps -ef", 5).is_none());
        // 刚跑的是别的命令也不弹 ps
        assert!(candidate_from_just_ran_success(&h, &[], "uptime", 5).is_none());
    }

    #[test]
    fn efficiency_report_pdf_non_empty() {
        // Skip if embedded font cannot be parsed by genpdf
        // (e.g. CFF-based OTF on platforms where printpdf rejects it)
        let dash = FragmentAnalyticsDashboard {
            personal_total_usage: 1,
            personal_success_rate: 100.0,
            personal_avg_ms: 10,
            team_total_usage: 0,
            team_success_rate: 0.0,
            team_avg_ms: 0,
            personal_top: vec![],
            team_top: vec![],
            slowest: vec![],
            highest_error: vec![],
            team_api_available: false,
            member_rows: vec![],
            period_stats_from_events: false,
            member_stats_from_server: false,
        };
        let pdf = match build_efficiency_report_pdf(
            &dash,
            crate::core::FragmentAnalyticsTimeRange::AllTime,
            &[],
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("SKIP: PDF font not available: {e}");
                return;
            }
        };
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 512);
    }
}
