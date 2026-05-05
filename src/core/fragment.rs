//! 命令片段管理

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandFragment {
    pub id: String,
    pub name: String,
    pub command: String,
    pub tags: Vec<String>,
    pub category: String,
    #[serde(default)]
    pub execution_count: u64,
    #[serde(default)]
    pub success_count: u64,
    #[serde(default)]
    pub failure_count: u64,
    #[serde(default)]
    pub total_duration_ms: u64,
    #[serde(default)]
    pub last_executed_at: Option<i64>,
}

impl CommandFragment {
    fn new(name: &str, command: &str, category: &str, tags: Vec<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.trim().to_string(),
            command: command.trim().to_string(),
            tags,
            category: if category.trim().is_empty() {
                "默认".to_string()
            } else {
                category.trim().to_string()
            },
            execution_count: 0,
            success_count: 0,
            failure_count: 0,
            total_duration_ms: 0,
            last_executed_at: None,
        }
    }
}

pub struct FragmentManager {
    fragments: Vec<CommandFragment>,
    file_path: PathBuf,
}

impl FragmentManager {
    pub fn new() -> Self {
        let mut file_path = std::env::current_dir().unwrap_or_default();
        file_path.push("fragments.json");
        let mut manager = Self {
            fragments: Vec::new(),
            file_path,
        };
        manager.load();
        if manager.fragments.is_empty() {
            manager.fragments = Self::default_fragments();
            manager.save();
        }
        manager
    }

    pub fn list_fragments(&self) -> &[CommandFragment] {
        &self.fragments
    }

    pub fn search_fragments(&self, query: &str) -> Vec<CommandFragment> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.fragments.clone();
        }
        self.fragments
            .iter()
            .filter(|f| {
                f.name.to_lowercase().contains(&q)
                    || f.command.to_lowercase().contains(&q)
                    || f.category.to_lowercase().contains(&q)
                    || f.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .cloned()
            .collect()
    }

    pub fn create_fragment(
        &mut self,
        name: &str,
        command: &str,
        category: &str,
        tags_csv: &str,
    ) -> Option<CommandFragment> {
        if name.trim().is_empty() || command.trim().is_empty() {
            return None;
        }
        let tags = tags_csv
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let fragment = CommandFragment::new(name, command, category, tags);
        self.fragments.push(fragment.clone());
        self.save();
        Some(fragment)
    }

    pub fn delete_fragment(&mut self, id: &str) -> bool {
        let old_len = self.fragments.len();
        self.fragments.retain(|f| f.id != id);
        let deleted = self.fragments.len() != old_len;
        if deleted {
            self.save();
        }
        deleted
    }

    pub fn update_fragment(
        &mut self,
        id: &str,
        name: &str,
        command: &str,
        category: &str,
        tags_csv: &str,
    ) -> bool {
        let tags = tags_csv
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        if let Some(fragment) = self.fragments.iter_mut().find(|f| f.id == id) {
            if name.trim().is_empty() || command.trim().is_empty() {
                return false;
            }
            fragment.name = name.trim().to_string();
            fragment.command = command.trim().to_string();
            fragment.category = if category.trim().is_empty() {
                "默认".to_string()
            } else {
                category.trim().to_string()
            };
            fragment.tags = tags;
            self.save();
            return true;
        }
        false
    }

    pub fn categories(&self) -> Vec<String> {
        let mut categories = self
            .fragments
            .iter()
            .map(|f| f.category.clone())
            .collect::<Vec<_>>();
        categories.sort();
        categories.dedup();
        categories
    }

    pub fn record_execution(&mut self, fragment_id: &str, success: bool, duration_ms: u64) {
        if let Some(fragment) = self.fragments.iter_mut().find(|f| f.id == fragment_id) {
            fragment.execution_count = fragment.execution_count.saturating_add(1);
            if success {
                fragment.success_count = fragment.success_count.saturating_add(1);
            } else {
                fragment.failure_count = fragment.failure_count.saturating_add(1);
            }
            fragment.total_duration_ms = fragment.total_duration_ms.saturating_add(duration_ms);
            fragment.last_executed_at = Some(now_unix_ts());
            self.save();
        }
    }

    pub fn tags(&self) -> Vec<String> {
        let mut tags = self
            .fragments
            .iter()
            .flat_map(|f| f.tags.clone())
            .collect::<Vec<_>>();
        tags.sort();
        tags.dedup();
        tags
    }

    pub fn export_to_path(&self, path: &std::path::Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(&self.fragments)
            .map_err(|e| format!("序列化失败: {}", e))?;
        fs::write(path, content).map_err(|e| format!("写入失败: {}", e))
    }

    pub fn import_from_path(
        &mut self,
        path: &std::path::Path,
        replace_all: bool,
    ) -> Result<usize, String> {
        let content = fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
        let mut imported: Vec<CommandFragment> =
            serde_json::from_str(&content).map_err(|e| format!("格式错误: {}", e))?;

        for item in &mut imported {
            if item.id.trim().is_empty() {
                item.id = uuid::Uuid::new_v4().to_string();
            }
            item.name = item.name.trim().to_string();
            item.command = item.command.trim().to_string();
            if item.category.trim().is_empty() {
                item.category = "默认".to_string();
            } else {
                item.category = item.category.trim().to_string();
            }
            item.tags = item
                .tags
                .iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>();
        }
        imported.retain(|f| !f.name.is_empty() && !f.command.is_empty());

        if replace_all {
            self.fragments = imported;
        } else {
            for fragment in imported {
                if let Some(existing) = self
                    .fragments
                    .iter_mut()
                    .find(|f| f.name == fragment.name && f.category == fragment.category)
                {
                    *existing = fragment;
                } else {
                    self.fragments.push(fragment);
                }
            }
        }

        self.save();
        Ok(self.fragments.len())
    }

    fn load(&mut self) {
        if !self.file_path.exists() {
            return;
        }
        if let Ok(content) = fs::read_to_string(&self.file_path) {
            if let Ok(items) = serde_json::from_str::<Vec<CommandFragment>>(&content) {
                self.fragments = items;
            }
        }
    }

    fn save(&self) {
        if let Ok(content) = serde_json::to_string_pretty(&self.fragments) {
            let _ = fs::write(&self.file_path, content);
        }
    }

    fn default_fragments() -> Vec<CommandFragment> {
        vec![
            CommandFragment::new("磁盘使用", "df -h", "系统监控", vec!["disk".into()]),
            CommandFragment::new("内存使用", "free -h", "系统监控", vec!["memory".into()]),
            CommandFragment::new("查看进程", "ps aux", "进程管理", vec!["process".into()]),
            CommandFragment::new("Docker 容器", "docker ps -a", "Docker", vec!["docker".into()]),
        ]
    }
}

fn now_unix_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
