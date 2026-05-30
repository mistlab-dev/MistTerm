//! 命令片段：统计、分类、标签、变量占位符

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use uuid::Uuid;

use super::session::SessionConfig;

/// 命令片段变量定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentVariable {
    /// 变量名（对应 `<name>` 占位符）
    pub name: String,
    /// 变量描述（用于提示用户）
    pub description: String,
    /// 默认值
    pub default_value: Option<String>,
}

impl FragmentVariable {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            default_value: None,
        }
    }

    pub fn with_default(name: &str, description: &str, default: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            default_value: Some(default.to_string()),
        }
    }
}

/// 单个命令片段的统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentStats {
    /// 唯一标识符
    pub id: String,
    /// 显示标题
    pub title: String,
    /// 命令内容
    pub command: String,
    /// 分类
    pub category: String,
    /// 标签（用于筛选与同步）
    #[serde(default)]
    pub tags: Vec<String>,
    /// 变量定义列表
    #[serde(default)]
    pub variables: Vec<FragmentVariable>,
    /// 使用次数
    pub usage_count: u32,
    /// 成功次数
    pub success_count: u32,
    /// 累计耗时（毫秒）
    pub total_time_ms: u64,
    /// 最后使用时间（Unix 时间戳）
    pub last_used: Option<i64>,
}

/// 兼容旧代码路径的类型别名（原「命令片段」模型）。
pub type CommandFragment = FragmentStats;

impl FragmentStats {
    /// 创建新的片段统计
    pub fn new(id: String, title: String, command: String, category: String) -> Self {
        Self {
            id,
            title,
            command,
            category,
            tags: Vec::new(),
            variables: Vec::new(),
            usage_count: 0,
            success_count: 0,
            total_time_ms: 0,
            last_used: None,
        }
    }

    /// 检查是否有需要用户输入的变量
    pub fn has_variables(&self) -> bool {
        !self.variables.is_empty()
    }

    /// 从命令中提取所有占位符名称
    pub fn extract_placeholders(&self) -> Vec<String> {
        list_placeholder_keys(&self.command)
    }

    /// 获取变量的默认值映射
    pub fn variable_defaults(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for var in &self.variables {
            if let Some(default) = &var.default_value {
                map.insert(var.name.clone(), default.clone());
            }
        }
        map
    }

    /// 应用变量值替换命令中的占位符
    pub fn apply_variables(&self, values: &HashMap<String, String>) -> String {
        let mut result = self.command.clone();
        for var in &self.variables {
            let placeholder = format!("<{}>", var.name);
            if let Some(value) = values.get(&var.name) {
                result = result.replace(&placeholder, value);
            }
        }
        result
    }

    /// 计算成功率
    pub fn success_rate(&self) -> f32 {
        if self.usage_count == 0 {
            0.0
        } else {
            (self.success_count as f32 / self.usage_count as f32) * 100.0
        }
    }

    /// 计算平均耗时（毫秒）
    pub fn avg_time_ms(&self) -> u32 {
        if self.usage_count == 0 {
            0
        } else {
            (self.total_time_ms / self.usage_count as u64) as u32
        }
    }

    /// 生成人类可读的统计字符串
    /// 格式: "{次数}次 · {成功率}%成功 · {耗时}s"
    pub fn human_readable(&self) -> String {
        let avg_ms = self.avg_time_ms();
        let time_str = if avg_ms >= 1000 {
            format!("{:.1}s", avg_ms as f32 / 1000.0)
        } else {
            format!("{}ms", avg_ms)
        };
        format!(
            "{}次 · {:.0}%成功 · {}",
            self.usage_count,
            self.success_rate(),
            time_str
        )
    }

    /// 记录一次使用
    pub fn record_usage(&mut self, success: bool, time_ms: u32) {
        self.usage_count += 1;
        if success {
            self.success_count += 1;
        }
        self.total_time_ms += time_ms as u64;
        self.last_used = Some(chrono::Utc::now().timestamp());
    }
}

/// 将 `<key>` 占位符替换为 `extras` 或当前会话上下文的值；未解析的占位符保持原样。
pub fn expand_command_template(
    template: &str,
    session: Option<&SessionConfig>,
    extras: &HashMap<String, String>,
) -> String {
    fn session_value(s: &SessionConfig, key: &str) -> Option<String> {
        match key {
            "host" | "hostname" => Some(s.host.clone()),
            "user" | "username" => Some(s.username.clone()),
            "port" => Some(s.port.to_string()),
            "session" | "session_name" | "name" => Some(s.name.clone()),
            _ => None,
        }
    }

    let mut out = String::with_capacity(template.len().saturating_mul(2));
    let mut rest = template;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else {
            out.push('<');
            out.push_str(rest);
            return out;
        };
        let key = rest[..close].trim();
        rest = &rest[close + 1..];
        let replacement = extras
            .get(key)
            .cloned()
            .or_else(|| session.and_then(|s| session_value(s, key)))
            .unwrap_or_else(|| format!("<{}>", key));
        out.push_str(&replacement);
    }
    out.push_str(rest);
    out
}

/// 将 `pairs` 中出现的 `<key>` 逐字替换为对应 `value`（与 UI 填写片段变量、`pending_fragment_vars` 行为一致）。
#[inline]
pub fn substitute_angle_placeholders(template: &str, pairs: &[(String, String)]) -> String {
    let mut output = template.to_string();
    for (key, value) in pairs {
        output = output.replace(&format!("<{}>", key), value);
    }
    output
}

/// UI 与片段库共用的展开顺序：**不得在整串上先于 Rhai 做 `<>` 字面替换**，否则会破坏 `{{ md5(<user>) }}`（会变成非法的 `md5(alice)` 标识符）。
/// 流程：在 `expand_rhai_blocks` 内部的 `{{ … }}` 里把 `<占位符>` 换成带引号的 Rhai 字面量（见 `fragment_expr::substitute_angle_placeholders_in_expr`）→ 会话/表单变量上下文求值；
/// 再对整条结果做 [`expand_command_template`]，展开 Rhai **之外**残留的 `<占位符>`（会话与 `extras`）。
pub fn expand_fragment_command_stages(
    template: &str,
    session: Option<&SessionConfig>,
    template_extras: &HashMap<String, String>,
) -> Result<String, String> {
    use super::fragment_expr::{expand_rhai_blocks, merge_rhai_context};
    let ctx = merge_rhai_context(session, template_extras);
    let after_rhai = expand_rhai_blocks(template, &ctx)?;
    Ok(expand_command_template(
        &after_rhai,
        session,
        template_extras,
    ))
}

/// 提取模板中的 `<占位符>` 名称（去重、保序）。`{{ … }}` 表达式内的内容不参与扫描，避免 `a < b` 等误匹配。
pub fn list_placeholder_keys(template: &str) -> Vec<String> {
    use super::fragment_expr::find_closing_double_brace;

    let mut seen = HashMap::<String, ()>::new();
    let mut order = Vec::new();
    let bytes = template.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let open = i;
            if let Some(close) = find_closing_double_brace(template, open) {
                i = close + 2;
            } else {
                break;
            }
            continue;
        }
        if bytes[i] == b'<' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b'>' {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let key = template
                .get(start..i)
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            if !key.is_empty() && !seen.contains_key(&key) {
                seen.insert(key.clone(), ());
                order.push(key);
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    order
}

/// 排序方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortBy {
    /// 按使用次数降序
    UsageCount,
    /// 按成功率降序
    SuccessRate,
    /// 按最后使用时间降序
    LastUsed,
    /// 按名称排序
    Name,
}

/// 合并导入片段时的摘要
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FragmentMergeReport {
    pub added: usize,
    pub skipped_duplicate_id: usize,
}

/// 片段管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentManager {
    /// 所有片段
    fragments: Vec<FragmentStats>,
    /// ID 到索引的映射（用于快速查找）
    #[serde(skip)]
    id_map: HashMap<String, usize>,
}

impl Default for FragmentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FragmentManager {
    /// 创建新的片段管理器
    pub fn new() -> Self {
        Self {
            fragments: Vec::new(),
            id_map: HashMap::new(),
        }
    }

    /// 重建 ID 映射
    fn rebuild_id_map(&mut self) {
        self.id_map.clear();
        for (i, frag) in self.fragments.iter().enumerate() {
            self.id_map.insert(frag.id.clone(), i);
        }
    }

    /// Initialize from market catalog if available, otherwise start empty.
    pub fn init_from_market_or_defaults(market: Option<&crate::core::market::MarketFragmentCache>) -> Self {
        let mut manager = Self {
            fragments: Vec::new(),
            id_map: HashMap::new(),
        };
        if let Some(cache) = market {
            if !cache.fragments.is_empty() {
                for item in &cache.fragments {
                    let mut frag = FragmentStats::new(
                        item.id.clone(),
                        item.title.clone(),
                        item.command.clone(),
                        if item.category.is_empty() { "market".to_string() } else { item.category.clone() },
                    );
                    frag.tags = crate::core::team::parse_tags_json(&item.tags);
                    if !frag.tags.iter().any(|t| t.eq_ignore_ascii_case("market")) {
                        frag.tags.push("market".into());
                    }
                    frag.tags.push(format!("mkt:{}", item.id));
                    frag.variables = crate::core::team::parse_variables_json(&item.variables);
                    manager.fragments.push(frag);
                }
                manager.rebuild_id_map();
            }
        }
        manager
    }

    /// 获取默认配置文件路径
    pub fn default_config_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mistterm");
        
        // 确保目录存在
        let _ = fs::create_dir_all(&config_dir);
        config_dir.join("fragments.json")
    }

    /// 从文件加载
    pub fn load(path: &PathBuf) -> io::Result<Self> {
        let mut manager: FragmentManager =
            crate::security::encrypted_file::load_encrypted_json(path);
        if manager.fragments.is_empty() && !path.exists() {
            manager = Self::new();
            manager.save(path)?;
            return Ok(manager);
        }
        manager.rebuild_id_map();
        Ok(manager)
    }

    /// 保存到文件
    pub fn save(&self, path: &PathBuf) -> io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(path, self)
    }

    /// 获取所有片段
    pub fn get_all(&self) -> &[FragmentStats] {
        &self.fragments
    }

    /// 旧 UI / 合并分支兼容别名
    pub fn list(&self) -> &[FragmentStats] {
        self.get_all()
    }

    pub fn get(&self, id: &str) -> Option<&FragmentStats> {
        self.get_by_id(id)
    }

    /// 将一次「片段已展开并写入 PTY（含末尾回车）」记为一次使用；`success` 表示 PTY 写入是否成功。
    /// FUNCTIONAL_SPEC §3.3.4 要求的「按远端退出码判定成败」需在 shell 侧配合（如 `PROMPT_COMMAND` 回传）后才能闭环；
    /// 当前实现不把普通交互式命令的退出码与片段自动关联。
    pub fn record_execution(&mut self, id: &str, success: bool, dur_ms: u64) {
        let ms = dur_ms.min(u32::MAX as u64) as u32;
        self.record_usage(id, success, ms);
    }

    /// 按分类获取片段
    pub fn get_by_category(&self, category: &str) -> Vec<&FragmentStats> {
        self.fragments
            .iter()
            .filter(|f| f.category == category)
            .collect()
    }

    /// 获取所有分类
    pub fn get_categories(&self) -> Vec<String> {
        let mut categories: Vec<String> = self.fragments
            .iter()
            .map(|f| f.category.clone())
            .collect();
        categories.sort();
        categories.dedup();
        categories
    }

    /// 按 ID 查找片段
    pub fn get_by_id(&self, id: &str) -> Option<&FragmentStats> {
        self.id_map.get(id).and_then(|&i| self.fragments.get(i))
    }

    /// 按 ID 查找可变引用
    pub fn get_by_id_mut(&mut self, id: &str) -> Option<&mut FragmentStats> {
        self.id_map.get(id).and_then(|&i| self.fragments.get_mut(i))
    }

    /// 按命令查找片段
    pub fn get_by_command(&self, command: &str) -> Option<&FragmentStats> {
        self.fragments.iter().find(|f| f.command == command)
    }

    /// 按命令查找可变引用
    pub fn get_by_command_mut(&mut self, command: &str) -> Option<&mut FragmentStats> {
        self.fragments.iter_mut().find(|f| f.command == command)
    }

    /// 记录使用
    pub fn record_usage(&mut self, id: &str, success: bool, time_ms: u32) {
        if let Some(frag) = self.get_by_id_mut(id) {
            frag.record_usage(success, time_ms);
        }
    }

    /// 记录命令使用（按命令内容查找）
    pub fn record_usage_by_command(&mut self, command: &str, success: bool, time_ms: u32) {
        if let Some(frag) = self.get_by_command_mut(command) {
            frag.record_usage(success, time_ms);
        }
    }

    /// 添加新片段
    pub fn add_fragment(&mut self, title: String, command: String, category: String) -> &FragmentStats {
        self.add_fragment_with_tags(title, command, category, Vec::new())
    }

    /// 添加带标签的片段
    pub fn add_fragment_with_tags(
        &mut self,
        title: String,
        command: String,
        category: String,
        tags: Vec<String>,
    ) -> &FragmentStats {
        let id = Uuid::new_v4().to_string();
        let mut fragment = FragmentStats::new(id, title, command, category);
        fragment.tags = tags;
        self.fragments.push(fragment);
        self.rebuild_id_map();
        self.fragments.last().unwrap()
    }

    /// 添加带标签和变量的片段
    pub fn add_fragment_with_all(
        &mut self,
        title: String,
        command: String,
        category: String,
        tags: Vec<String>,
        variables: Vec<FragmentVariable>,
    ) -> &FragmentStats {
        let id = Uuid::new_v4().to_string();
        let mut fragment = FragmentStats::new(id, title, command, category);
        fragment.tags = tags;
        fragment.variables = variables;
        self.fragments.push(fragment);
        self.rebuild_id_map();
        self.fragments.last().unwrap()
    }

    /// 更新片段元数据（保留统计）
    pub fn update_fragment(
        &mut self,
        id: &str,
        title: String,
        command: String,
        category: String,
        tags: Vec<String>,
    ) -> bool {
        if let Some(frag) = self.get_by_id_mut(id) {
            frag.title = title;
            frag.command = command;
            frag.category = category;
            frag.tags = tags;
            true
        } else {
            false
        }
    }

    /// 更新片段（含变量定义）
    pub fn update_fragment_with_vars(
        &mut self,
        id: &str,
        title: String,
        command: String,
        category: String,
        tags: Vec<String>,
        variables: Vec<FragmentVariable>,
    ) -> bool {
        if let Some(frag) = self.get_by_id_mut(id) {
            frag.title = title;
            frag.command = command;
            frag.category = category;
            frag.tags = tags;
            frag.variables = variables;
            true
        } else {
            false
        }
    }

    /// 删除片段
    pub fn remove_fragment(&mut self, id: &str) -> bool {
        if let Some(&index) = self.id_map.get(id) {
            self.fragments.remove(index);
            self.rebuild_id_map();
            true
        } else {
            false
        }
    }

    /// 排序片段
    pub fn sort(&mut self, sort_by: SortBy) {
        match sort_by {
            SortBy::UsageCount => {
                self.fragments.sort_by(|a, b| {
                    b.usage_count.cmp(&a.usage_count)
                });
            }
            SortBy::SuccessRate => {
                self.fragments.sort_by(|a, b| {
                    b.success_rate().partial_cmp(&a.success_rate())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SortBy::LastUsed => {
                self.fragments.sort_by(|a, b| {
                    b.last_used.cmp(&a.last_used)
                });
            }
            SortBy::Name => {
                self.fragments.sort_by(|a, b| {
                    a.title.cmp(&b.title)
                });
            }
        }
        self.rebuild_id_map();
    }

    pub fn merge_from_other(&mut self, other: &FragmentManager) -> FragmentMergeReport {
        let mut report = FragmentMergeReport::default();
        for f in &other.fragments {
            if self.id_map.contains_key(&f.id) {
                report.skipped_duplicate_id += 1;
                continue;
            }
            self.fragments.push(f.clone());
            report.added += 1;
        }
        self.rebuild_id_map();
        report
    }

    pub fn replace_with(&mut self, other: FragmentManager) {
        self.fragments = other.fragments;
        self.rebuild_id_map();
    }

    /// 从 `fragments.json` 路径加载并合并或替换到 `target`
    pub fn import_from_json_path(
        path: &PathBuf,
        merge: bool,
        target: &mut Self,
    ) -> io::Result<FragmentMergeReport> {
        let loaded = Self::load(path)?;
        if merge {
            Ok(target.merge_from_other(&loaded))
        } else {
            target.replace_with(loaded);
            Ok(FragmentMergeReport {
                added: target.fragments.len(),
                skipped_duplicate_id: 0,
            })
        }
    }

    /// 导出所有片段为 JSON 字符串
    pub fn export_to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&self.fragments)
            .map_err(|e| format!("JSON导出失败: {}", e))
    }

    /// 从 JSON 字符串导入片段（合并模式，跳过重复 ID）
    pub fn import_from_json(&mut self, json: &str) -> Result<usize, String> {
        let fragments: Vec<FragmentStats> = serde_json::from_str(json)
            .map_err(|e| format!("JSON导入失败: {}", e))?;
        
        let mut added = 0;
        for fragment in fragments {
            if !self.id_map.contains_key(&fragment.id) {
                self.fragments.push(fragment.clone());
                self.id_map.insert(fragment.id, self.fragments.len() - 1);
                added += 1;
            }
        }
        
        Ok(added)
    }

    /// 导出片段到文件
    pub fn export_to_file(&self, path: &std::path::Path) -> Result<(), String> {
        let json = self.export_to_json()?;
        std::fs::write(path, json)
            .map_err(|e| format!("写入文件失败: {}", e))
    }

    /// 从文件导入片段
    pub fn import_from_file(&mut self, path: &std::path::Path) -> Result<usize, String> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        self.import_from_json(&json)
    }

    /// 搜索片段
    pub fn search(&self, query: &str) -> Vec<&FragmentStats> {
        let query_lower = query.to_lowercase();
        self.fragments
            .iter()
            .filter(|f| {
                f.title.to_lowercase().contains(&query_lower)
                    || f.command.to_lowercase().contains(&query_lower)
                    || f.category.to_lowercase().contains(&query_lower)
                    || f.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session::SessionConfig;

    #[test]
    fn test_substitute_angle_placeholders() {
        let pairs = vec![
            ("svc".into(), "nginx".into()),
            ("path".into(), "/tmp".into()),
        ];
        let out = substitute_angle_placeholders("systemctl status <svc> <path>", &pairs);
        assert_eq!(out, "systemctl status nginx /tmp");
    }

    #[test]
    fn test_expand_fragment_command_md5_inside_rhai_then_plain_user() {
        let mut m = std::collections::HashMap::new();
        m.insert("user".into(), "alice".into());
        let out =
            expand_fragment_command_stages("echo u_{{ md5(<user>) }}_<user>", None, &m)
                .expect("inside-{{ }} `<user>` is quoted before Rhai");
        assert!(!out.contains('<'), "{}", out);
        assert!(!out.contains("{{"), "{}", out);
        assert!(
            out.contains("alice"),
            "trailing `_<user>` should become `_alice`: {}",
            out
        );
    }

    #[test]
    fn test_expand_fragment_command_stages_order() {
        let s = SessionConfig {
            id: "1".into(),
            name: "生产".into(),
            group: "g".into(),
            host: "10.1.2.3".into(),
            port: 22,
            username: "alice".into(),
            password: String::new(),
            private_key_path: String::new(),
            last_connected_at: None,
            ..SessionConfig::default()
        };
        let mut extras = HashMap::new();
        extras.insert("svc".into(), "127.0.0.1".into());
        let out = expand_fragment_command_stages(
            "ping <svc> @ <host> — {{ 1 + 2 }}",
            Some(&s),
            &extras,
        )
        .unwrap();
        assert!(out.contains("127.0.0.1"));
        assert!(out.contains("10.1.2.3"));
        assert!(
            out.contains("3"),
            "Rhai scalar should stringify in output: {}",
            out
        );
        assert!(!out.contains("{{"));
    }

    #[test]
    fn test_expand_command_template() {
        let s = SessionConfig {
            id: "1".into(),
            name: "生产".into(),
            group: "g".into(),
            host: "10.0.0.1".into(),
            port: 2222,
            username: "ubuntu".into(),
            password: String::new(),
            private_key_path: String::new(),
            last_connected_at: None,
            ..SessionConfig::default()
        };
        let mut m = HashMap::new();
        m.insert("service".into(), "nginx".into());
        let t = "ssh <user>@<host> -p <port> restart <service>";
        let out = expand_command_template(t, Some(&s), &m);
        assert_eq!(out, "ssh ubuntu@10.0.0.1 -p 2222 restart nginx");
    }

    #[test]
    fn test_list_placeholder_keys() {
        let t = "echo <a> and <b> <a>";
        assert_eq!(list_placeholder_keys(t), vec!["a", "b"]);
    }

    #[test]
    fn test_list_placeholder_keys_skips_rhai_blocks() {
        let t = "{{ a + b }} <host>";
        assert_eq!(list_placeholder_keys(t), vec!["host"]);
    }

    #[test]
    fn test_fragment_stats_human_readable() {
        let mut stats = FragmentStats::new(
            "test-id".to_string(),
            "Test Command".to_string(),
            "echo test".to_string(),
            "Test".to_string(),
        );
        
        // 无使用记录
        assert_eq!(stats.human_readable(), "0次 · 0%成功 · 0ms");
        
        // 记录使用
        stats.record_usage(true, 500);
        stats.record_usage(true, 1500);
        stats.record_usage(false, 300);
        
        let readable = stats.human_readable();
        assert!(readable.contains("3次"));
        assert!(readable.contains("67%")); // 2/3 成功
        assert!(readable.contains("766ms")); // 平均耗时
    }

    #[test]
    fn test_fragment_manager() {
        let mut manager = FragmentManager::new();

        manager.fragments.push(FragmentStats::new(
            "test-1".to_string(),
            "Docker PS".to_string(),
            "docker ps".to_string(),
            "docker".to_string(),
        ));
        manager.fragments.push(FragmentStats::new(
            "test-2".to_string(),
            "Docker Images".to_string(),
            "docker images".to_string(),
            "docker".to_string(),
        ));
        manager.rebuild_id_map();

        assert!(!manager.get_all().is_empty());

        let categories = manager.get_categories();
        assert!(!categories.is_empty());

        let first_id = manager.get_all().first().unwrap().id.clone();
        manager.record_usage(&first_id, true, 1000);

        let first_updated = manager.get_by_id(&first_id).unwrap();
        assert_eq!(first_updated.usage_count, 1);
        assert_eq!(first_updated.success_count, 1);
    }

    #[test]
    fn test_fragment_manager_sort() {
        let mut manager = FragmentManager::new();
        
        // 记录不同使用次数 - 先克隆需要的 ID，避免借用冲突
        let fragment_ids: Vec<String> = manager.get_all().iter().take(3).map(|f| f.id.clone()).collect();
        if fragment_ids.len() >= 3 {
            manager.record_usage(&fragment_ids[0], true, 100);
            manager.record_usage(&fragment_ids[0], true, 100);
            manager.record_usage(&fragment_ids[1], true, 100);
        }
        
        manager.sort(SortBy::UsageCount);
        
        let sorted = manager.get_all();
        for i in 0..sorted.len().saturating_sub(1) {
            assert!(sorted[i].usage_count >= sorted[i + 1].usage_count);
        }
    }

    #[test]
    fn test_fragment_manager_search() {
        let mut manager = FragmentManager::new();

        manager.fragments.push(FragmentStats::new(
            "docker-1".to_string(),
            "Docker PS".to_string(),
            "docker ps".to_string(),
            "containers".to_string(),
        ));
        manager.fragments.push(FragmentStats::new(
            "docker-2".to_string(),
            "Docker Images".to_string(),
            "docker images".to_string(),
            "images".to_string(),
        ));
        manager.rebuild_id_map();

        let results = manager.search("docker");
        assert!(!results.is_empty());

        for frag in results {
            let found = frag.title.to_lowercase().contains("docker")
                || frag.command.to_lowercase().contains("docker")
                || frag.category.to_lowercase().contains("docker");
            assert!(found);
        }
    }
}