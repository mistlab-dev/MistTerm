//! 监控快照供 AI 附带的纯文本格式（无 SSH 依赖）。

use mistterm::monitor::ServerStats;

#[test]
fn format_for_ai_includes_core_metrics() {
    let mut stats = ServerStats::default();
    stats.cpu_percent = 33.3;
    stats.memory_used = 4 * 1024 * 1024 * 1024;
    stats.memory_total = 16 * 1024 * 1024 * 1024;
    stats.disk_used = 100 * 1024 * 1024 * 1024;
    stats.disk_total = 500 * 1024 * 1024 * 1024;
    stats.load_avg = (0.1, 0.2, 0.3);
    stats.uptime_secs = 7200;
    stats.network_rx_bytes = 4096;
    stats.network_tx_bytes = 8192;

    let text = stats.format_for_ai();
    assert!(text.contains("CPU: 33.3%"));
    assert!(text.contains("Memory:"));
    assert!(text.contains("Disk:"));
    assert!(text.contains("Load (1/5/15m):"));
    assert!(text.contains("Uptime:"));
    assert!(text.contains("Network RX/TX:"));
}

#[test]
fn empty_stats_still_formats_without_panic() {
    let stats = ServerStats::default();
    let text = stats.format_for_ai();
    assert!(text.contains("CPU:"));
}
