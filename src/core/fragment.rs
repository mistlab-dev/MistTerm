//! 命令片段：统计、分类、标签、变量占位符

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader, BufWriter};
use std::path::PathBuf;
use uuid::Uuid;

use super::session::SessionConfig;

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
    /// 使用次数
    pub usage_count: u32,
    /// 成功次数
    pub success_count: u32,
    /// 累计耗时（毫秒）
    pub total_time_ms: u64,
    /// 最后使用时间（Unix 时间戳）
    pub last_used: Option<i64>,
}

impl FragmentStats {
    /// 创建新的片段统计
    pub fn new(id: String, title: String, command: String, category: String) -> Self {
        Self {
            id,
            title,
            command,
            category,
            tags: Vec::new(),
            usage_count: 0,
            success_count: 0,
            total_time_ms: 0,
            last_used: None,
        }
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

/// 提取模板中的占位符名称（去重、保序）
pub fn list_placeholder_keys(template: &str) -> Vec<String> {
    let mut seen = HashMap::<String, ()>::new();
    let mut order = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else { break };
        let key = rest[..close].trim().to_string();
        if !key.is_empty() && !seen.contains_key(&key) {
            seen.insert(key.clone(), ());
            order.push(key);
        }
        rest = &rest[close + 1..];
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
        let mut manager = Self {
            fragments: Vec::new(),
            id_map: HashMap::new(),
        };
        manager.add_default_fragments();
        manager.rebuild_id_map();
        manager
    }

    /// 重建 ID 映射
    fn rebuild_id_map(&mut self) {
        self.id_map.clear();
        for (i, frag) in self.fragments.iter().enumerate() {
            self.id_map.insert(frag.id.clone(), i);
        }
    }

    /// 添加默认片段
    fn add_default_fragments(&mut self) {
        let defaults = vec![
            // 系统监控
            ("磁盘使用", "df -h", "系统监控"),
            ("内存使用", "free -h", "系统监控"),
            ("CPU 负载", "uptime", "系统监控"),
            ("系统信息", "uname -a", "系统监控"),
            // 进程管理
            ("查看进程", "ps aux", "进程管理"),
            ("top 监控", "top", "进程管理"),
            ("查找进程", "ps aux | grep ", "进程管理"),
            ("杀死进程", "kill -9 ", "进程管理"),
            // 网络
            ("网络连接", "netstat -tulpn", "网络"),
            ("DNS 查询", "dig google.com", "网络"),
            ("Ping 测试", "ping -c 4 google.com", "网络"),
            // Docker
            ("查看容器", "docker ps -a", "Docker"),
            ("容器日志", "docker logs -f ", "Docker"),
            ("重启容器", "docker restart ", "Docker"),
            // Nginx
            ("重启 Nginx", "sudo systemctl restart nginx", "Nginx"),
            ("Nginx 状态", "sudo systemctl status nginx", "Nginx"),
            ("错误日志", "tail -f /var/log/nginx/error.log", "Nginx"),
        ];

        for (title, command, category) in defaults {
            let id = Uuid::new_v4().to_string();
            self.fragments.push(FragmentStats::new(id, title.to_string(), command.to_string(), category.to_string()));
        }
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
        if !path.exists() {
            let manager = Self::new();
            manager.save(path)?;
            return Ok(manager);
        }

        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut manager: FragmentManager = serde_json::from_reader(reader)
            .unwrap_or_else(|_| Self::new());
        manager.rebuild_id_map();
        Ok(manager)
    }

    /// 保存到文件
    pub fn save(&self, path: &PathBuf) -> io::Result<()> {
        let file = fs::File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    /// 获取所有片段
    pub fn get_all(&self) -> &[FragmentStats] {
        &self.fragments
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
    fn test_expand_command_template() {
        let s = SessionConfig {
            id: "1".into(),
            name: "生产".into(),
            group: "g".into(),
            host: "10.0.0.1".into(),
            port: 2222,
            username: "ubuntu".into(),
            password: String::new(),
            last_connected_at: None,
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
        
        // 检查默认片段
        assert!(!manager.get_all().is_empty());
        
        // 获取分类
        let categories = manager.get_categories();
        assert!(!categories.is_empty());
        
        // 记录使用
        let first = manager.get_all().first().unwrap();
        manager.record_usage(&first.id, true, 1000);
        
        let first_updated = manager.get_by_id(&first.id).unwrap();
        assert_eq!(first_updated.usage_count, 1);
        assert_eq!(first_updated.success_count, 1);
    }

    #[test]
    fn test_fragment_manager_sort() {
        let mut manager = FragmentManager::new();
        
        // 记录不同使用次数
        let fragments = manager.get_all();
        if fragments.len() >= 3 {
            manager.record_usage(&fragments[0].id, true, 100);
            manager.record_usage(&fragments[0].id, true, 100);
            manager.record_usage(&fragments[1].id, true, 100);
        }
        
        manager.sort(SortBy::UsageCount);
        
        let sorted = manager.get_all();
        for i in 0..sorted.len().saturating_sub(1) {
            assert!(sorted[i].usage_count >= sorted[i + 1].usage_count);
        }
    }

    #[test]
    fn test_fragment_manager_search() {
        let manager = FragmentManager::new();
        
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