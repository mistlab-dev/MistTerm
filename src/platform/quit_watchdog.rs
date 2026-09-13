//! 退出看门狗：保证应用在收到退出请求后**必然**及时终止，绝不无限卡死。
//!
//! 背景：eframe(0.23)+wgpu 在 macOS 上的退出顺序是
//! `App::save()` → `App::on_exit()` → `painter.destroy()` → drop(App) → `process::exit(0)`。
//! 其中 `painter.destroy()`（wgpu/Metal 资源释放）以及少数 `Drop`（如审计 worker 的
//! `join()`）在某些环境下会长时间阻塞，表现为「⌘Q 后窗口消失/卡住但进程迟迟不退出」。
//!
//! 对策：在 `on_exit()`（此时 `save()` 已完成、状态已落盘）里 arm 一个守护线程，
//! 睡眠一个很短的宽限期后强制 `process::exit(0)`。正常情况下进程会在拆卸完成后
//! 先一步退出，守护线程随进程一起消亡、永不触发；只有当拆卸阶段真的卡住时，
//! 看门狗才兜底强制退出，把「无限卡死」变成「最多等 grace 毫秒」。

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// 幂等标志：多条退出路径（⌘Q / 关闭按钮）可能都会调用 arm，只允许装一次。
static ARMED: AtomicBool = AtomicBool::new(false);

/// 退出宽限期：给正常的 wgpu 拆卸 / Drop 足够时间完成；一旦超时即强制退出。
/// 取值权衡：太短可能截断正常拆卸（罕见但存在）；太长则卡死体感明显。
/// 1500ms 对本地资源释放绰绰有余，同时把最坏卡死体感控制在可接受范围。
const DEFAULT_GRACE: Duration = Duration::from_millis(1500);

/// 使用默认宽限期装载退出看门狗。重复调用无副作用。
pub fn arm_quit_watchdog() {
    arm_quit_watchdog_with_grace(DEFAULT_GRACE);
}

/// 使用自定义宽限期装载退出看门狗（便于测试与特殊场景）。重复调用只有第一次生效。
pub fn arm_quit_watchdog_with_grace(grace: Duration) {
    if ARMED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = thread::Builder::new()
        .name("mistterm-quit-watchdog".to_string())
        .spawn(move || {
            thread::sleep(grace);
            log::warn!(
                "quit watchdog fired after {}ms — forcing process exit (teardown was too slow)",
                grace.as_millis()
            );
            #[allow(clippy::exit)]
            std::process::exit(0);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_is_idempotent() {
        // 首次装载返回后 ARMED 应为 true；再次装载不应 panic，也不应重复触发。
        // 用较长宽限期，确保测试进程不会被看门狗真的 exit 掉。
        arm_quit_watchdog_with_grace(Duration::from_secs(3600));
        assert!(ARMED.load(Ordering::SeqCst));
        // 二次调用直接短路返回。
        arm_quit_watchdog_with_grace(Duration::from_secs(3600));
        assert!(ARMED.load(Ordering::SeqCst));
    }
}
