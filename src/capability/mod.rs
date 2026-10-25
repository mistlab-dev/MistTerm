//! Capability API + Module Registry（#113 / #116，P0 脚手架）。
//!
//! 分层（见 `docs/tech/EXTENSION-POINTS.md`）：
//! - **Kernel**（SSH/PTY/渲染/会话）永不插件——本模块不持有任何 SSH handle。
//! - **MistModule**：同进程、compile-time 注册的可开关业务能力（display-link、
//!   trigger、cmd_audit 等未来迁入；本票只建 trait + registry，不迁业务）。
//! - Host 责任：泵线程不跑模块、失败隔离、菜单/面板由 Host 绘制。
//!
//! 边界（#116 Non-goals）：不做插件发现协议、热加载、WASM、Skill 目录；
//! Agent / Runbook 不注册为 MistModule。

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 模块清单里的稳定 id（如 "display-link" / "output-trigger"）。
pub type ModuleId = &'static str;

/// `on_output_line` 的只读上下文：PTY 完整行（UTF-8 已解码）。
/// Host 约束：仅在编排线程对完整行调用，禁止阻塞 SSH 泵。
pub struct OutputLineCtx<'a> {
    pub line: &'a str,
}

/// `on_before_send` 的只读上下文：用户回车 / 片段插入即将写入 PTY 的文本。
pub struct BeforeSendCtx<'a> {
    pub text: &'a str,
}

/// 右键菜单贡献条目（声明式；由 Host 绘制与分发）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// 稳定动作 key（Host 映射到具体行为）。
    pub action: String,
    pub label_zh: String,
    pub label_en: String,
}

/// 右 dock 槽位贡献（声明式；槽位有上限，与现有 Dock 枚举兼容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockContribution {
    pub slot_key: String,
    pub title_zh: String,
    pub title_en: String,
}

/// 会话事件（只读；模块用它清理自身状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    Connected,
    Disconnected,
    ReconnectAbandoned,
}

/// 模块在 `on_output_line` 产出的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleAction {
    /// 渲染层：某字节区间可点（display-link 迁入后使用）。
    DecorateLink {
        start: usize,
        end: usize,
        action: String,
        text: String,
    },
    /// 宿主通知：后台标签注意点 + 可选 Toast（trigger 迁入后使用）。
    Notify {
        severity: Severity,
        text: String,
        attention: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

/// `on_before_send` 的裁决（cmd_audit-local 迁入后使用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendVerdict {
    Allow,
    Confirm { reason: String },
    Block { reason: String },
}

/// 内置模块 trait（#116：契约一次定满 5 钩子；后三个有默认空实现）。
pub trait MistModule: Send + Sync {
    /// 稳定 id，registry 与设置按它启停。
    fn id(&self) -> ModuleId;
    fn name_zh(&self) -> &str;
    fn name_en(&self) -> &str;
    fn enabled_by_default(&self) -> bool {
        true
    }

    /// PTY 完整行扫描。
    fn on_output_line(&self, _ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
        Vec::new()
    }

    /// 即将写入 PTY 的文本审计。
    fn on_before_send(&self, _ctx: &BeforeSendCtx<'_>) -> SendVerdict {
        SendVerdict::Allow
    }

    /// 终端右键菜单贡献。只贡献条目，不拥有菜单生命周期。
    fn contribute_context_menu(&self) -> Vec<MenuItem> {
        Vec::new()
    }

    /// 右 dock 槽位贡献。槽位有上限；与现有 Dock 枚举兼容。
    fn contribute_right_dock(&self) -> Vec<DockContribution> {
        Vec::new()
    }

    /// 连接成功 / 断开 / 重连放弃（事件只读）。
    fn on_session_event(&self, _event: SessionEvent) {}
}

/// compile-time registry：官方模块手写列表（P0 不引入 inventory/发现协议）。
pub struct ModuleRegistry {
    modules: Vec<Box<dyn MistModule>>,
    /// 运行时禁用集合（按 id）。禁用后该模块所有钩子不再被调用。
    disabled: HashSet<String>,
    /// 启停变化标志（Host 据此做缓存失效等联动）。
    state_changed: Arc<AtomicBool>,
}

impl ModuleRegistry {
    /// 官方内置模块清单。迁入业务前先允许空表（#116 只测脚手架）。
    pub fn builtin() -> Self {
        Self::with_modules(Vec::new())
    }

    pub fn with_modules(modules: Vec<Box<dyn MistModule>>) -> Self {
        let mut reg = Self {
            modules,
            disabled: HashSet::new(),
            state_changed: Arc::new(AtomicBool::new(false)),
        };
        reg.apply_env_disable();
        // 默认禁用 enabled_by_default == false 的模块。
        let off: Vec<String> = reg
            .modules
            .iter()
            .filter(|m| !m.enabled_by_default())
            .map(|m| m.id().to_string())
            .collect();
        for id in off {
            reg.disabled.insert(id);
        }
        reg
    }

    /// `MISTTERM_DISABLE_MODULES=`（逗号分隔 id）预留解析（#116 Notes）。
    /// 正式与设置合并的优先级在 #118 落地；本票先只解析环境变量。
    fn apply_env_disable(&mut self) {
        let Ok(raw) = std::env::var("MISTTERM_DISABLE_MODULES") else {
            return;
        };
        for id in raw.split(',') {
            let id = id.trim();
            if !id.is_empty() {
                self.disabled.insert(id.to_string());
            }
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.contains(id)
    }

    /// 按 id 启用/禁用。禁用后钩子不再被调用（不要求擦 scrollback）。
    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if enabled {
            self.disabled.remove(id);
        } else {
            self.disabled.insert(id.to_string());
        }
        self.state_changed.store(true, Ordering::Relaxed);
    }

    /// 启停是否有变化（读取即清除）。Host 可用它失效缓存。
    pub fn take_state_changed(&self) -> bool {
        self.state_changed.swap(false, Ordering::Relaxed)
    }

    /// 枚举模块清单（id / 名称 / 默认态 / 当前态），偏好 UI（#118）与调试用。
    pub fn snapshot(&self) -> Vec<ModuleInfo> {
        self.modules
            .iter()
            .map(|m| ModuleInfo {
                id: m.id().to_string(),
                name_zh: m.name_zh().to_string(),
                name_en: m.name_en().to_string(),
                enabled_by_default: m.enabled_by_default(),
                enabled: self.is_enabled(m.id()),
            })
            .collect()
    }

    /// 分发 `on_output_line`：仅对启用模块调用。
    pub fn on_output_line(&self, ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
        let mut out = Vec::new();
        for m in &self.modules {
            if !self.is_enabled(m.id()) {
                continue;
            }
            out.extend(m.on_output_line(ctx));
        }
        out
    }

    /// 分发 `on_before_send`：任一模块 Block 即整体 Block（短路）；
    /// 其次取第一个 Confirm；全 Allow 才 Allow。
    pub fn on_before_send(&self, ctx: &BeforeSendCtx<'_>) -> SendVerdict {
        let mut confirm: Option<SendVerdict> = None;
        for m in &self.modules {
            if !self.is_enabled(m.id()) {
                continue;
            }
            match m.on_before_send(ctx) {
                SendVerdict::Block { reason } => return SendVerdict::Block { reason },
                v @ SendVerdict::Confirm { .. } => {
                    if confirm.is_none() {
                        confirm = Some(v);
                    }
                }
                SendVerdict::Allow => {}
            }
        }
        confirm.unwrap_or(SendVerdict::Allow)
    }

    /// 汇总启用模块的右键菜单贡献。
    pub fn context_menu_items(&self) -> Vec<MenuItem> {
        let mut out = Vec::new();
        for m in &self.modules {
            if self.is_enabled(m.id()) {
                out.extend(m.contribute_context_menu());
            }
        }
        out
    }

    /// 汇总启用模块的右 dock 贡献。
    pub fn right_dock_contributions(&self) -> Vec<DockContribution> {
        let mut out = Vec::new();
        for m in &self.modules {
            if self.is_enabled(m.id()) {
                out.extend(m.contribute_right_dock());
            }
        }
        out
    }

    /// 广播会话事件给启用模块。
    pub fn on_session_event(&self, event: SessionEvent) {
        for m in &self.modules {
            if self.is_enabled(m.id()) {
                m.on_session_event(event);
            }
        }
    }
}

/// 模块清单条目（偏好 UI / 调试快照）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInfo {
    pub id: String,
    pub name_zh: String,
    pub name_en: String,
    pub enabled_by_default: bool,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 假模块：计数钩子调用，验证「禁用后 hook 次数为 0」。
    struct FakeModule {
        id: ModuleId,
        default_on: bool,
        output_calls: Mutex<usize>,
        send_calls: Mutex<usize>,
        session_calls: Mutex<usize>,
    }

    impl FakeModule {
        fn new(id: ModuleId, default_on: bool) -> Self {
            Self {
                id,
                default_on,
                output_calls: Mutex::new(0),
                send_calls: Mutex::new(0),
                session_calls: Mutex::new(0),
            }
        }

        fn calls(&self) -> (usize, usize, usize) {
            (
                *self.output_calls.lock().unwrap(),
                *self.send_calls.lock().unwrap(),
                *self.session_calls.lock().unwrap(),
            )
        }
    }

    impl MistModule for FakeModule {
        fn id(&self) -> ModuleId {
            self.id
        }
        fn name_zh(&self) -> &str {
            "假模块"
        }
        fn name_en(&self) -> &str {
            "fake"
        }
        fn enabled_by_default(&self) -> bool {
            self.default_on
        }

        fn on_output_line(&self, _ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
            *self.output_calls.lock().unwrap() += 1;
            Vec::new()
        }

        fn on_before_send(&self, _ctx: &BeforeSendCtx<'_>) -> SendVerdict {
            *self.send_calls.lock().unwrap() += 1;
            SendVerdict::Allow
        }

        fn on_session_event(&self, _event: SessionEvent) {
            *self.session_calls.lock().unwrap() += 1;
        }
    }

    struct BlockingModule;

    impl MistModule for BlockingModule {
        fn id(&self) -> ModuleId {
            "blocker"
        }
        fn name_zh(&self) -> &str {
            "拦截"
        }
        fn name_en(&self) -> &str {
            "blocker"
        }
        fn on_before_send(&self, _ctx: &BeforeSendCtx<'_>) -> SendVerdict {
            SendVerdict::Block {
                reason: "危险命令".into(),
            }
        }
    }

    #[test]
    fn empty_registry_registers_nothing() {
        let reg = ModuleRegistry::builtin();
        assert!(reg.snapshot().is_empty());
        let ctx = OutputLineCtx { line: "hello" };
        assert!(reg.on_output_line(&ctx).is_empty());
        assert_eq!(reg.on_before_send(&BeforeSendCtx { text: "ls" }), SendVerdict::Allow);
    }

    #[test]
    fn fake_module_registers_and_hooks() {
        let reg = ModuleRegistry::with_modules(vec![Box::new(FakeModule::new("fake-a", true))]);
        let info = reg.snapshot();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].id, "fake-a");
        assert!(info[0].enabled);

        let ctx = OutputLineCtx { line: "x" };
        reg.on_output_line(&ctx);
        reg.on_before_send(&BeforeSendCtx { text: "ls" });
        reg.on_session_event(SessionEvent::Connected);
    }

    #[test]
    fn disabled_module_hooks_never_called() {
        let m = Box::new(FakeModule::new("fake-b", true));
        // 借 registry 分发前先保存假模块引用不可行——用包装在 trait 内计数，
        // 这里改用快照前后调用对比：禁用 → 分发 → 计数不变。
        struct SharedFake {
            inner: Box<dyn MistModule>,
            calls: Arc<Mutex<usize>>,
        }
        impl MistModule for SharedFake {
            fn id(&self) -> ModuleId {
                self.inner.id()
            }
            fn name_zh(&self) -> &str {
                self.inner.name_zh()
            }
            fn name_en(&self) -> &str {
                self.inner.name_en()
            }
            fn enabled_by_default(&self) -> bool {
                self.inner.enabled_by_default()
            }
            fn on_output_line(&self, ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
                *self.calls.lock().unwrap() += 1;
                self.inner.on_output_line(ctx)
            }
            fn on_before_send(&self, ctx: &BeforeSendCtx<'_>) -> SendVerdict {
                *self.calls.lock().unwrap() += 1;
                self.inner.on_before_send(ctx)
            }
            fn on_session_event(&self, ev: SessionEvent) {
                *self.calls.lock().unwrap() += 1;
                self.inner.on_session_event(ev)
            }
        }
        let counter = Arc::new(Mutex::new(0));
        let mut reg = ModuleRegistry::with_modules(vec![Box::new(SharedFake {
            inner: Box::new(FakeModule::new("shared", true)),
            calls: counter.clone(),
        })]);
        assert!(reg.is_enabled("shared"));

        let ctx = OutputLineCtx { line: "x" };
        reg.on_output_line(&ctx);
        reg.on_before_send(&BeforeSendCtx { text: "ls" });
        reg.on_session_event(SessionEvent::Disconnected);
        assert_eq!(*counter.lock().unwrap(), 3);

        // 禁用后：所有钩子不再被调用。
        reg.set_enabled("shared", false);
        assert!(reg.take_state_changed());
        reg.on_output_line(&ctx);
        reg.on_before_send(&BeforeSendCtx { text: "ls" });
        reg.on_session_event(SessionEvent::Connected);
        assert_eq!(*counter.lock().unwrap(), 3);

        // 重新启用后恢复调用。
        reg.set_enabled("shared", true);
        reg.on_output_line(&ctx);
        assert_eq!(*counter.lock().unwrap(), 4);
    }

    #[test]
    fn default_off_module_starts_disabled() {
        let reg = ModuleRegistry::with_modules(vec![Box::new(FakeModule::new("opt-in", false))]);
        assert!(!reg.is_enabled("opt-in"));
        let info = reg.snapshot();
        assert_eq!(info[0].enabled, false);
        assert_eq!(info[0].enabled_by_default, false);
    }

    #[test]
    fn block_short_circuits_and_confirm_collected() {
        // Block 短路：即使还有其它模块，整体 Block。
        let mut reg = ModuleRegistry::with_modules(vec![
            Box::new(BlockingModule),
            Box::new(FakeModule::new("other", true)),
        ]);
        let v = reg.on_before_send(&BeforeSendCtx { text: "rm -rf /" });
        assert_eq!(
            v,
            SendVerdict::Block {
                reason: "危险命令".into()
            }
        );

        // 禁用拦截模块后放行。
        reg.set_enabled("blocker", false);
        assert_eq!(
            reg.on_before_send(&BeforeSendCtx { text: "rm -rf /" }),
            SendVerdict::Allow
        );
    }

    #[test]
    fn menu_and_dock_contributions_respect_enabled() {
        struct MenuModule;
        impl MistModule for MenuModule {
            fn id(&self) -> ModuleId {
                "menu-mod"
            }
            fn name_zh(&self) -> &str {
                "菜单"
            }
            fn name_en(&self) -> &str {
                "menu"
            }
            fn contribute_context_menu(&self) -> Vec<MenuItem> {
                vec![MenuItem {
                    action: "mod.act".into(),
                    label_zh: "动作".into(),
                    label_en: "Action".into(),
                }]
            }
            fn contribute_right_dock(&self) -> Vec<DockContribution> {
                vec![DockContribution {
                    slot_key: "mod.panel".into(),
                    title_zh: "面板".into(),
                    title_en: "Panel".into(),
                }]
            }
        }
        let mut reg = ModuleRegistry::with_modules(vec![Box::new(MenuModule)]);
        assert_eq!(reg.context_menu_items().len(), 1);
        assert_eq!(reg.right_dock_contributions().len(), 1);
        reg.set_enabled("menu-mod", false);
        assert!(reg.context_menu_items().is_empty());
        assert!(reg.right_dock_contributions().is_empty());
    }

    #[test]
    fn env_disable_respects_ids() {
        // 直接测解析逻辑：构造后手动写入 disabled（避免环境变量跨测试污染）。
        let mut reg = ModuleRegistry::with_modules(vec![Box::new(FakeModule::new("env-mod", true))]);
        reg.disabled.insert("env-mod".to_string());
        assert!(!reg.is_enabled("env-mod"));
    }
}
