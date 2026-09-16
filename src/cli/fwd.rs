//! `mist fwd` — 端口转发（Local / Remote / Dynamic SOCKS5）。

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::ssh::{
    spawn_dynamic_forward_controllable, spawn_local_forward_controllable,
    spawn_remote_forward_controllable, DynamicPortForward, ForwardControl, LocalPortForward,
    RemotePortForward, SshClient,
};

use super::CliContext;

/// 解析形如 `8080:remote.host:80` 或 `127.0.0.1:8080:remote.host:80` 的本地转发参数
pub fn parse_local_forward_arg(s: &str) -> Result<LocalPortForward> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        3 => {
            let local_port: u16 = parts[0].parse().context("Invalid local port")?;
            let remote_host = parts[1].to_string();
            let remote_port: u16 = parts[2].parse().context("Invalid remote port")?;
            Ok(LocalPortForward {
                local_port,
                remote_host,
                remote_port,
                bind_address: "127.0.0.1".into(),
            })
        }
        4 => {
            let bind_address = parts[0].to_string();
            let local_port: u16 = parts[1].parse().context("Invalid local port")?;
            let remote_host = parts[2].to_string();
            let remote_port: u16 = parts[3].parse().context("Invalid remote port")?;
            Ok(LocalPortForward {
                local_port,
                remote_host,
                remote_port,
                bind_address,
            })
        }
        _ => anyhow::bail!("Invalid local forward format. Use [bind_addr:]local_port:remote_host:remote_port"),
    }
}

/// 解析形如 `9090:127.0.0.1:3000` 或 `remote_bind:9090:127.0.0.1:3000` 的远程转发参数
pub fn parse_remote_forward_arg(s: &str) -> Result<RemotePortForward> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        3 => {
            let remote_port: u16 = parts[0].parse().context("Invalid remote port")?;
            let target_host = parts[1].to_string();
            let target_port: u16 = parts[2].parse().context("Invalid target port")?;
            Ok(RemotePortForward {
                remote_port,
                target_host,
                target_port,
                remote_bind_address: None,
            })
        }
        4 => {
            let remote_bind_address = Some(parts[0].to_string());
            let remote_port: u16 = parts[1].parse().context("Invalid remote port")?;
            let target_host = parts[2].to_string();
            let target_port: u16 = parts[3].parse().context("Invalid target port")?;
            Ok(RemotePortForward {
                remote_port,
                target_host,
                target_port,
                remote_bind_address,
            })
        }
        _ => anyhow::bail!("Invalid remote forward format. Use [bind_addr:]remote_port:target_host:target_port"),
    }
}

/// 解析形如 `1080` 或 `127.0.0.1:1080` 的动态 SOCKS5 转发参数
pub fn parse_dynamic_forward_arg(s: &str) -> Result<DynamicPortForward> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => {
            let local_port: u16 = parts[0].parse().context("Invalid local port")?;
            Ok(DynamicPortForward {
                local_port,
                bind_address: "127.0.0.1".into(),
            })
        }
        2 => {
            let bind_address = parts[0].to_string();
            let local_port: u16 = parts[1].parse().context("Invalid local port")?;
            Ok(DynamicPortForward {
                local_port,
                bind_address,
            })
        }
        _ => anyhow::bail!("Invalid dynamic forward format. Use [bind_addr:]local_port"),
    }
}

pub fn run_fwd(
    ctx: &mut CliContext,
    target: &str,
    locals: &[String],
    remotes: &[String],
    dynamics: &[String],
) -> Result<i32> {
    let session_cfg = ctx.resolve_target(target)?;
    let mut config = ctx.ssh_config(&session_cfg)?;

    // 若命令行指定了转发规则，则优先使用/追加命令行指定的；若未指定则使用会话配置里已有的规则
    let has_cli_forwards = !locals.is_empty() || !remotes.is_empty() || !dynamics.is_empty();

    let (local_fwds, remote_fwds, dynamic_fwds) = if has_cli_forwards {
        let mut l_list = Vec::new();
        for l in locals {
            l_list.push(parse_local_forward_arg(l)?);
        }
        let mut r_list = Vec::new();
        for r in remotes {
            r_list.push(parse_remote_forward_arg(r)?);
        }
        let mut d_list = Vec::new();
        for d in dynamics {
            d_list.push(parse_dynamic_forward_arg(d)?);
        }
        (l_list, r_list, d_list)
    } else {
        (
            config.local_forwards.clone(),
            config.remote_forwards.clone(),
            config.dynamic_forwards.clone(),
        )
    };

    if local_fwds.is_empty() && remote_fwds.is_empty() && dynamic_fwds.is_empty() {
        anyhow::bail!("未指定任何转发规则，且目标会话中未配置任何端口转发。");
    }

    // 将 client 默认配置里的自动 spawn 清空，由我们在 CLI 显式可控地启动并展示
    config.local_forwards.clear();
    config.remote_forwards.clear();
    config.dynamic_forwards.clear();

    let mut client = SshClient::new(config);
    client
        .connect()
        .map_err(|e| anyhow::anyhow!("连接失败 {}: {}", session_cfg.name, e))?;
    ctx.mark_connected(&session_cfg);

    let ssh_sess = client.get_session().clone();
    let mut controls: Vec<ForwardControl> = Vec::new();

    println!("================ 端口转发运行中 (Ctrl+C 停止) ================");
    for fwd in &local_fwds {
        match spawn_local_forward_controllable(ssh_sess.clone(), fwd.clone()) {
            Ok(ctrl) => {
                controls.push(ctrl);
                println!(
                    "  [Local -L]   {}:{} -> {}:{}",
                    fwd.bind_address, fwd.local_port, fwd.remote_host, fwd.remote_port
                );
            }
            Err(e) => eprintln!("  [Local -L 失败] {}: {}", fwd.local_port, e),
        }
    }

    for fwd in &remote_fwds {
        match spawn_remote_forward_controllable(ssh_sess.clone(), fwd.clone()) {
            Ok(ctrl) => {
                controls.push(ctrl);
                let bind = fwd.remote_bind_address.as_deref().unwrap_or("0.0.0.0");
                println!(
                    "  [Remote -R]  {}:{} <- {}:{}",
                    bind, fwd.remote_port, fwd.target_host, fwd.target_port
                );
            }
            Err(e) => eprintln!("  [Remote -R 失败] {}: {}", fwd.remote_port, e),
        }
    }

    for fwd in &dynamic_fwds {
        match spawn_dynamic_forward_controllable(ssh_sess.clone(), fwd.clone()) {
            Ok(ctrl) => {
                controls.push(ctrl);
                println!(
                    "  [Dynamic -D] SOCKS5 {}:{}",
                    fwd.bind_address, fwd.local_port
                );
            }
            Err(e) => eprintln!("  [Dynamic -D 失败] {}: {}", fwd.local_port, e),
        }
    }
    println!("==============================================================");

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    #[cfg(unix)]
    {
        use std::sync::atomic::AtomicPtr;
        static RUNNING_PTR: AtomicPtr<AtomicBool> = AtomicPtr::new(std::ptr::null_mut());
        RUNNING_PTR.store(
            Arc::as_ptr(&running) as *mut AtomicBool,
            Ordering::SeqCst,
        );

        extern "C" fn handle_sigint(_: libc::c_int) {
            let ptr = RUNNING_PTR.load(Ordering::SeqCst);
            if !ptr.is_null() {
                unsafe {
                    (*ptr).store(false, Ordering::SeqCst);
                }
            }
        }

        unsafe {
            libc::signal(
                libc::SIGINT,
                handle_sigint as *const () as libc::sighandler_t,
            );
            libc::signal(
                libc::SIGTERM,
                handle_sigint as *const () as libc::sighandler_t,
            );
        }
    }

    #[cfg(not(unix))]
    {
        // 非 unix fallback，简单的前台阻塞
    }

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }

    println!("\n正在关闭所有端口转发...");
    for ctrl in controls {
        ctrl.stop();
    }
    client.disconnect();
    println!("已退出。");

    Ok(0)
}
