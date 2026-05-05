//! 服务器资源采集器
//!
//! 通过 SSH 执行命令采集 CPU、内存、磁盘、负载、网络等指标

use super::parser;
use crate::ssh::SshSessionHandle;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// 服务器统计信息
#[derive(Debug, Clone, Default)]
pub struct ServerStats {
    /// CPU 使用率百分比 (0-100)
    pub cpu_percent: f32,
    /// 已用内存（字节）
    pub memory_used: u64,
    /// 总内存（字节）
    pub memory_total: u64,
    /// 已用磁盘（字节）
    pub disk_used: u64,
    /// 总磁盘（字节）
    pub disk_total: u64,
    /// 系统负载 (1分钟, 5分钟, 15分钟)
    pub load_avg: (f32, f32, f32),
    /// 运行时间（秒）
    pub uptime_secs: u64,
    /// 网络接收字节
    pub network_rx_bytes: u64,
    /// 网络发送字节
    pub network_tx_bytes: u64,
    /// 采集时间戳
    pub collected_at: Instant,
}

impl ServerStats {
    /// 内存使用百分比
    pub fn memory_percent(&self) -> f32 {
        if self.memory_total == 0 {
            0.0
        } else {
            (self.memory_used as f64 / self.memory_total as f64 * 100.0) as f32
        }
    }

    /// 磁盘使用百分比
    pub fn disk_percent(&self) -> f32 {
        if self.disk_total == 0 {
            0.0
        } else {
            (self.disk_used as f64 / self.disk_total as f64 * 100.0) as f32
        }
    }

    /// 格式化内存显示
    pub fn format_memory(&self) -> String {
        format!(
            "{} / {}",
            format_bytes(self.memory_used),
            format_bytes(self.memory_total)
        )
    }

    /// 格式化磁盘显示
    pub fn format_disk(&self) -> String {
        format!(
            "{} / {}",
            format_bytes(self.disk_used),
            format_bytes(self.disk_total)
        )
    }

    /// 格式化运行时间
    pub fn format_uptime(&self) -> String {
        let days = self.uptime_secs / 86400;
        let hours = (self.uptime_secs % 86400) / 3600;
        let mins = (self.uptime_secs % 3600) / 60;
        if days > 0 {
            format!("{}天 {:02}:{:02}", days, hours, mins)
        } else {
            format!("{:02}:{:02}:{:02}", hours, mins, self.uptime_secs % 60)
        }
    }
}

/// 格式化字节数为人类可读格式
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.1}T", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// 监控器
pub struct Monitor {
    /// SSH 会话句柄
    ssh_handle: SshSessionHandle,
    /// 最近一次采集数据
    last_stats: ServerStats,
    /// 历史数据（最近 60 条）
    history: Vec<ServerStats>,
    /// 上次网络统计（用于计算速率）
    last_network: Option<(u64, u64, Instant)>,
    /// 上次采集时间
    last_refresh: Option<Instant>,
}

impl Monitor {
    /// 创建新的监控器
    pub fn new(ssh_handle: SshSessionHandle) -> Self {
        Self {
            ssh_handle,
            last_stats: ServerStats::default(),
            history: Vec::with_capacity(60),
            last_network: None,
            last_refresh: None,
        }
    }

    /// 刷新采集数据
    pub fn refresh(&mut self) -> Result<ServerStats, String> {
        let channel = self
            .ssh_handle
            .get_channel()
            .ok_or_else(|| "No SSH channel available".to_string())?;

        let mut stats = ServerStats::default();
        stats.collected_at = Instant::now();

        // 采集各项指标
        if let Ok(output) = self.exec_command(&channel, "cat /proc/stat | head -1") {
            stats.cpu_percent = parser::parse_cpu(&output, self.last_stats.cpu_percent);
        }

        if let Ok(output) = self.exec_command(&channel, "free -b") {
            if let Some((used, total)) = parser::parse_memory(&output) {
                stats.memory_used = used;
                stats.memory_total = total;
            }
        }

        if let Ok(output) = self.exec_command(&channel, "df -B1 /") {
            if let Some((used, total)) = parser::parse_disk(&output) {
                stats.disk_used = used;
                stats.disk_total = total;
            }
        }

        if let Ok(output) = self.exec_command(&channel, "cat /proc/loadavg") {
            stats.load_avg = parser::parse_loadavg(&output);
        }

        if let Ok(output) = self.exec_command(&channel, "cat /proc/uptime") {
            stats.uptime_secs = parser::parse_uptime(&output);
        }

        if let Ok(output) = self.exec_command(&channel, "cat /proc/net/dev") {
            if let Some((rx, tx)) = parser::parse_network(&output) {
                stats.network_rx_bytes = rx;
                stats.network_tx_bytes = tx;
            }
        }

        // 保存历史数据
        self.history.push(stats.clone());
        if self.history.len() > 60 {
            self.history.remove(0);
        }

        self.last_stats = stats.clone();
        self.last_refresh = Some(Instant::now());

        Ok(stats)
    }

    /// 获取最近一次采集数据
    pub fn last_stats(&self) -> &ServerStats {
        &self.last_stats
    }

    /// 获取历史数据
    pub fn get_history(&self) -> &[ServerStats] {
        &self.history
    }

    /// 获取网络速率 (rx_bytes/sec, tx_bytes/sec)
    pub fn network_rate(&self) -> (f64, f64) {
        if self.history.len() < 2 {
            return (0.0, 0.0);
        }

        let prev = &self.history[self.history.len() - 2];
        let curr = &self.last_stats;

        let dt = (curr.collected_at - prev.collected_at).as_secs_f64();
        if dt <= 0.0 {
            return (0.0, 0.0);
        }

        let rx_rate = (curr.network_rx_bytes as f64 - prev.network_rx_bytes as f64) / dt;
        let tx_rate = (curr.network_tx_bytes as f64 - prev.network_tx_bytes as f64) / dt;

        (rx_rate.max(0.0), tx_rate.max(0.0))
    }

    /// 执行远程命令并获取输出
    fn exec_command(
        &self,
        channel: &std::sync::Arc<std::sync::Mutex<ssh2::Channel>>,
        command: &str,
    ) -> Result<String, String> {
        let channel = channel.lock().map_err(|e| e.to_string())?;

        // 创建新的 exec channel
        let session = channel.session();
        let mut exec_channel = session
            .channel_session()
            .map_err(|e| format!("Failed to create exec channel: {}", e))?;

        exec_channel
            .exec(true, command)
            .map_err(|e| format!("Failed to exec command: {}", e))?;

        // 读取输出
        let mut output = String::new();
        exec_channel
            .read_to_string(&mut output)
            .map_err(|e| format!("Failed to read output: {}", e))?;

        exec_channel
            .close()
            .map_err(|e| format!("Failed to close channel: {}", e))?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500B");
        assert_eq!(format_bytes(1024), "1.0K");
        assert_eq!(format_bytes(1024 * 1024), "1.0M");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0G");
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 1024), "1.0T");
    }

    #[test]
    fn test_server_stats_percent() {
        let mut stats = ServerStats::default();
        stats.memory_used = 4 * 1024 * 1024 * 1024; // 4GB
        stats.memory_total = 8 * 1024 * 1024 * 1024; // 8GB
        assert!((stats.memory_percent() - 50.0).abs() < 0.1);

        stats.disk_used = 50 * 1024 * 1024 * 1024; // 50GB
        stats.disk_total = 100 * 1024 * 1024 * 1024; // 100GB
        assert!((stats.disk_percent() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_format_uptime() {
        let mut stats = ServerStats::default();
        stats.uptime_secs = 3661; // 1小时1分1秒
        assert_eq!(stats.format_uptime(), "01:01:01");

        stats.uptime_secs = 90061; // 1天1小时1分1秒
        assert_eq!(stats.format_uptime(), "1天 01:01");
    }
}
