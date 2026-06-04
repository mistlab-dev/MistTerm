//! 监控集成测试：SSH 远程命令与输出解析（需本地 sshd；无 sshd 时自动 skip）。

use mistterm::test_support::ssh_local::{exec_remote, skip_without_sshd};

#[test]
fn test_parse_free_output() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let output = exec_remote(&session, "free -b").expect("free -b failed");

    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 2, "free output should have header and data");

    let mem_line = lines[1];
    let parts: Vec<&str> = mem_line.split_whitespace().collect();
    assert!(parts.len() >= 3, "Should have total/used/free columns");

    let total: u64 = parts[1].parse().expect("Total should be numeric");
    let used: u64 = parts[2].parse().expect("Used should be numeric");

    println!("Memory: {}B total, {}B used", total, used);
    assert!(total > 0, "Total memory should be positive");
    assert!(used <= total, "Used should not exceed total");
}

#[test]
fn test_parse_df_output() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let output = exec_remote(&session, "df -B1 /").expect("df failed");

    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 2, "df output should have header and data");

    let disk_line = lines[1];
    let parts: Vec<&str> = disk_line.split_whitespace().collect();
    assert!(parts.len() >= 3, "Should have filesystem/total/used columns");

    let total: u64 = parts[1].parse().expect("Total should be numeric");
    let used: u64 = parts[2].parse().expect("Used should be numeric");

    println!("Disk: {}B total, {}B used", total, used);
    assert!(total > 0, "Total disk should be positive");
}

#[test]
fn test_parse_loadavg() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let output = exec_remote(&session, "cat /proc/loadavg").expect("loadavg failed");

    let parts: Vec<&str> = output.split_whitespace().collect();
    assert!(parts.len() >= 3, "Should have 3 load averages");

    let load1: f32 = parts[0].parse().expect("Load1 should be float");
    let load5: f32 = parts[1].parse().expect("Load5 should be float");
    let load15: f32 = parts[2].parse().expect("Load15 should be float");

    println!("Load: {}, {}, {}", load1, load5, load15);
    assert!(load1 >= 0.0 && load5 >= 0.0 && load15 >= 0.0);
}

#[test]
fn test_parse_uptime() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let output = exec_remote(&session, "cat /proc/uptime").expect("uptime failed");

    let uptime_secs: f64 = output
        .split_whitespace()
        .next()
        .expect("Should have uptime")
        .parse()
        .expect("Uptime should be numeric");

    println!("Uptime: {:.0} seconds", uptime_secs);
    assert!(uptime_secs > 0.0, "Uptime should be positive");
}

#[test]
fn test_parse_proc_stat_cpu() {
    let Some(session) = skip_without_sshd() else {
        return;
    };
    let output = exec_remote(&session, "grep 'cpu ' /proc/stat").expect("proc stat failed");

    let parts: Vec<&str> = output.split_whitespace().collect();
    assert!(parts.len() >= 8, "CPU stat should have multiple fields");
    assert_eq!(parts[0], "cpu", "First field should be 'cpu'");

    let user: u64 = parts[1].parse().expect("User should be numeric");
    let nice: u64 = parts[2].parse().expect("Nice should be numeric");
    let system: u64 = parts[3].parse().expect("System should be numeric");
    let idle: u64 = parts[4].parse().expect("Idle should be numeric");

    println!(
        "CPU: user={}, nice={}, system={}, idle={}",
        user, nice, system, idle
    );
    let total = user + nice + system + idle;
    assert!(total > 0, "Total CPU time should be positive");
}
