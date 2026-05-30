//! 服务器资源采集器
//!
//! 通过 SSH **`exec`** 采集 CPU、内存、磁盘、负载、网络等指标。UI 侧应经
//! [`crate::ssh::SshSessionHandle::enqueue_remote_exec`] 在 shell 泵线程执行，勿在其它线程与 PTY 并发
//! [`crate::ssh::SshManager::exec_remote`]（会争用 `Session`，终端易僵死）。
//! 远端一次执行多条只读命令：内容与 `top`/`free`/`df`/`uptime`/`/proc/net/dev`
//! 等价，输出由 [`super::parser`] 解析；CPU 使用率为连续两次刷新间 `/proc/stat` 聚合行的差分。

use crate::monitor::parser;
use crate::ssh::{SshManager, SshSessionHandle, SshSessionId};
use std::time::Instant;

fn section_after<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    let key = format!("---{}---", label);
    let (_, tail) = text.split_once(&key)?;
    let tail = tail.trim_start_matches(['\r', '\n']);
    if let Some(pos) = tail.find("\n---") {
        Some(tail[..pos].trim())
    } else {
        Some(tail.trim())
    }
}

/// 服务器统计信息
#[derive(Debug, Clone)]
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

impl Default for ServerStats {
    fn default() -> Self {
        Self {
            cpu_percent: 0.0,
            memory_used: 0,
            memory_total: 0,
            disk_used: 0,
            disk_total: 0,
            load_avg: (0.0, 0.0, 0.0),
            uptime_secs: 0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            collected_at: Instant::now(),
        }
    }
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
    /// SSH 会话句柄（与 PTY 同源，用于 `session_id`）
    ssh_handle: SshSessionHandle,
    /// 与 SFTP 相同的 `SshManager` 克隆，用于 exec 采集
    ssh_manager: SshManager,
    /// 最近一次采集数据
    last_stats: ServerStats,
    /// 历史数据（最近 60 条）
    history: Vec<ServerStats>,
    /// 上次网络统计（预留给扩展）
    #[allow(dead_code)]
    last_network: Option<(u64, u64, Instant)>,
    /// 上次采集时间
    last_refresh: Option<Instant>,
    /// 上一帧的 `/proc/stat` 聚合 CPU 行，用于差分 CPU%
    last_cpu_stat_line: Option<String>,
}

impl Monitor {
    /// 创建新的监控器
    pub fn new(ssh_handle: SshSessionHandle, ssh_manager: SshManager) -> Self {
        Self {
            ssh_handle,
            ssh_manager,
            last_stats: ServerStats::default(),
            history: Vec::with_capacity(60),
            last_network: None,
            last_refresh: None,
            last_cpu_stat_line: None,
        }
    }

    pub fn session_id(&self) -> SshSessionId {
        self.ssh_handle.session_id
    }

    pub fn ssh_manager(&self) -> SshManager {
        self.ssh_manager.clone()
    }

    pub fn ssh_session_handle(&self) -> &SshSessionHandle {
        &self.ssh_handle
    }

    /// 单次远程采集：`/proc/stat`、`free -b`、`df -B1 /`、`/proc/loadavg`、`/proc/uptime`、`/proc/net/dev`
    pub const COLLECT_CMD: &'static str = r#"sh -c 'printf "%s\n" "---CPU---"; grep "^cpu " /proc/stat | head -n1; printf "%s\n" "---FREE---"; free -b; printf "%s\n" "---DF---"; df -B1 /; printf "%s\n" "---LOAD---"; cat /proc/loadavg; printf "%s\n" "---UPTIME---"; cat /proc/uptime; printf "%s\n" "---NET---"; cat /proc/net/dev'"#;

    /// 解析一次远程采集的原始输出并更新内部状态（可在 UI 线程调用；[`Self::refresh`] 会先 `exec` 再解析）。
    pub fn ingest_remote_output(&mut self, raw: &str) -> Result<ServerStats, String> {
        let cpu_block = section_after(raw, "CPU").ok_or("采集输出缺少 CPU 段")?;
        let cpu_line = cpu_block
            .lines()
            .find(|l| l.trim_start().starts_with("cpu "))
            .map(str::trim)
            .unwrap_or_else(|| cpu_block.lines().next().unwrap_or("").trim());

        let cpu_percent = if !cpu_line.is_empty() {
            if let Some(ref prev) = self.last_cpu_stat_line {
                parser::cpu_percent_between(prev, cpu_line)
                    .unwrap_or(self.last_stats.cpu_percent)
            } else {
                0.0
            }
        } else {
            self.last_stats.cpu_percent
        };
        if !cpu_line.is_empty() {
            self.last_cpu_stat_line = Some(cpu_line.to_string());
        }

        let mem_block = section_after(raw, "FREE").ok_or("采集输出缺少 FREE 段")?;
        let (memory_used, memory_total) =
            parser::parse_memory(mem_block).unwrap_or((0, 0));

        let df_block = section_after(raw, "DF").ok_or("采集输出缺少 DF 段")?;
        let (disk_used, disk_total) = parser::parse_disk(df_block).unwrap_or((0, 0));

        let load_block = section_after(raw, "LOAD").ok_or("采集输出缺少 LOAD 段")?;
        let load_line = load_block.lines().next().unwrap_or(load_block).trim();
        let load_avg = parser::parse_loadavg(load_line);

        let up_block = section_after(raw, "UPTIME").ok_or("采集输出缺少 UPTIME 段")?;
        let up_line = up_block.lines().next().unwrap_or(up_block).trim();
        let uptime_secs = parser::parse_uptime(up_line);

        let net_block = section_after(raw, "NET").ok_or("采集输出缺少 NET 段")?;
        let (network_rx_bytes, network_tx_bytes) =
            parser::parse_network(net_block).unwrap_or((0, 0));

        let stats = ServerStats {
            cpu_percent,
            memory_used,
            memory_total,
            disk_used,
            disk_total,
            load_avg,
            uptime_secs,
            network_rx_bytes,
            network_tx_bytes,
            collected_at: Instant::now(),
        };

        self.history.push(stats.clone());
        if self.history.len() > 60 {
            self.history.remove(0);
        }

        self.last_stats = stats.clone();
        self.last_refresh = Some(Instant::now());

        Ok(stats)
    }

    /// 同步执行远程采集（会阻塞直至命令返回；UI 线程请用异步采集）。
    pub fn refresh(&mut self) -> Result<ServerStats, String> {
        let sid = self.ssh_handle.session_id;
        let raw = self
            .ssh_manager
            .exec_remote(sid, Self::COLLECT_CMD)
            .map_err(|e| format!("监控采集失败: {}", e))?;
        self.ingest_remote_output(&raw)
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
