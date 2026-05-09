//! 服务器指标解析器
//!
//! 解析各种 Linux 命令输出，提取系统指标

/// 从 `cpu` 行解析活跃 jiffies 与总 jiffies（用于两次采样差分算 CPU%）
/// 格式: cpu  user nice system idle iowait irq softirq steal guest guest_nice
pub fn parse_cpu_jiffies_line(line: &str) -> Option<(u64, u64)> {
    let line = line.trim();
    if !line.starts_with("cpu ") {
        return None;
    }
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() < 4 {
        return None;
    }
    let idle = parts[3];
    let iowait = parts.get(4).copied().unwrap_or(0);
    let idle_total = idle + iowait;
    let sum: u64 = parts.iter().sum();
    let active = sum.saturating_sub(idle_total);
    Some((active, sum))
}

/// 根据两次相邻的 `grep '^cpu ' /proc/stat` 输出行计算 CPU 使用率
pub fn cpu_percent_between(prev_line: &str, curr_line: &str) -> Option<f32> {
    let (a1, t1) = parse_cpu_jiffies_line(prev_line)?;
    let (a2, t2) = parse_cpu_jiffies_line(curr_line)?;
    let da = a2.saturating_sub(a1);
    let dt = t2.saturating_sub(t1);
    if dt == 0 {
        return None;
    }
    Some(((da as f64 / dt as f64) * 100.0).clamp(0.0, 100.0) as f32)
}

/// 解析 CPU 使用率
/// 从 /proc/stat 第一行解析
/// 格式: cpu  user nice system idle iowait irq softirq steal guest guest_nice
pub fn parse_cpu(output: &str, last_cpu_percent: f32) -> f32 {
    let line = output.trim();
    if !line.starts_with("cpu ") {
        return last_cpu_percent;
    }

    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1) // 跳过 "cpu" 标识
        .filter_map(|s| s.parse().ok())
        .collect();

    if parts.len() < 4 {
        return last_cpu_percent;
    }

    // user + nice + system + irq + softirq + steal = active
    // idle + iowait = idle
    let user = parts[0];
    let nice = parts[1];
    let system = parts[2];
    let idle = parts[3];
    let iowait = parts.get(4).copied().unwrap_or(0);
    let irq = parts.get(5).copied().unwrap_or(0);
    let softirq = parts.get(6).copied().unwrap_or(0);
    let steal = parts.get(7).copied().unwrap_or(0);

    let active = user + nice + system + irq + softirq + steal;
    let idle_total = idle + iowait;
    let total = active + idle_total;

    if total == 0 {
        return last_cpu_percent;
    }

    // 由于没有上次的数据来计算差值，返回一个估算值
    // 实际应用中应该保存上次的数据计算差值
    let usage = (active as f64 / total as f64 * 100.0) as f32;
    usage.clamp(0.0, 100.0)
}

/// 解析内存信息
/// 从 free -b 输出解析
pub fn parse_memory(output: &str) -> Option<(u64, u64)> {
    for line in output.lines() {
        if line.starts_with("Mem:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let total: u64 = parts[1].parse().ok()?;
                let used: u64 = parts[2].parse().ok()?;
                return Some((used, total));
            }
        }
    }
    None
}

/// 解析磁盘信息
/// 从 df -B1 / 输出解析
pub fn parse_disk(output: &str) -> Option<(u64, u64)> {
    for line in output.lines().skip(1) {
        // 跳过标题行
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let total: u64 = parts[1].parse().ok()?;
            let used: u64 = parts[2].parse().ok()?;
            return Some((used, total));
        }
    }
    None
}

/// 解析系统负载
/// 从 /proc/loadavg 解析
/// 格式: 1.23 0.98 0.56 2/128 12345
pub fn parse_loadavg(output: &str) -> (f32, f32, f32) {
    let parts: Vec<&str> = output.split_whitespace().collect();
    if parts.len() >= 3 {
        let load1: f32 = parts[0].parse().unwrap_or(0.0);
        let load5: f32 = parts[1].parse().unwrap_or(0.0);
        let load15: f32 = parts[2].parse().unwrap_or(0.0);
        (load1, load5, load15)
    } else {
        (0.0, 0.0, 0.0)
    }
}

/// 解析运行时间
/// 从 /proc/uptime 解析
/// 格式: 123456.78 456789.01
pub fn parse_uptime(output: &str) -> u64 {
    let parts: Vec<&str> = output.split_whitespace().collect();
    if parts.is_empty() {
        return 0;
    }

    // 取整数部分
    let uptime_secs: f64 = parts[0].parse().unwrap_or(0.0);
    uptime_secs as u64
}

/// 解析网络流量
/// 从 /proc/net/dev 解析
/// 返回 (接收字节, 发送字节)
pub fn parse_network(output: &str) -> Option<(u64, u64)> {
    let mut total_rx: u64 = 0;
    let mut total_tx: u64 = 0;

    for line in output.lines().skip(2) {
        // 跳过标题行
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // 格式: interface: rx_bytes rx_packets ... tx_bytes tx_packets ...
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        // 获取接口名（去掉冒号）
        let iface = parts[0].trim_end_matches(':');

        // 跳过 lo 接口
        if iface == "lo" {
            continue;
        }

        // 接收字节在第 1 列（0-indexed），发送字节在第 9 列
        if parts.len() >= 10 {
            if let Ok(rx) = parts[1].parse::<u64>() {
                if let Ok(tx) = parts[9].parse::<u64>() {
                    total_rx += rx;
                    total_tx += tx;
                }
            }
        }
    }

    if total_rx > 0 || total_tx > 0 {
        Some((total_rx, total_tx))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu() {
        let output = "cpu  12345 678 9012 345678 9012 345 678 90\n";
        let result = parse_cpu(output, 0.0);
        assert!(result > 0.0 && result <= 100.0);
    }

    #[test]
    fn test_cpu_percent_between() {
        let prev = "cpu  200 0 0 400 0 0 0 0\n";
        let curr = "cpu  250 0 0 450 0 0 0 0\n";
        let pct = cpu_percent_between(prev, curr).expect("delta");
        assert!((pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_memory() {
        let output = "              total        used        free      shared  buff/cache   available\nMem:   16384000     8000000     2000000     1000000     6384000     7000000\nSwap:   8000000           0     8000000\n";
        let result = parse_memory(output);
        assert_eq!(result, Some((8000000, 16384000)));
    }

    #[test]
    fn test_parse_disk() {
        let output = "Filesystem     1B-blocks      Used Available Use% Mounted on\n/dev/sda1     107374182400 53687091200 53687091200  50% /\n";
        let result = parse_disk(output);
        assert_eq!(result, Some((53687091200, 107374182400)));
    }

    #[test]
    fn test_parse_loadavg() {
        let output = "1.23 0.98 0.56 2/128 12345\n";
        let result = parse_loadavg(output);
        assert!((result.0 - 1.23).abs() < 0.01);
        assert!((result.1 - 0.98).abs() < 0.01);
        assert!((result.2 - 0.56).abs() < 0.01);
    }

    #[test]
    fn test_parse_uptime() {
        let output = "86400.12 172800.24\n";
        let result = parse_uptime(output);
        assert_eq!(result, 86400);
    }

    #[test]
    fn test_parse_network() {
        let output = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n  eth0: 12345678  1000    0    0    0     0          0         0 87654321   800    0    0    0     0       0          0\n    lo: 1234  10    0    0    0     0          0         0     1234  10    0    0    0     0       0          0\n";
        let result = parse_network(output);
        assert_eq!(result, Some((12345678, 87654321)));
    }
}