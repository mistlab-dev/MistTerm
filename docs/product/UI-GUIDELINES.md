# MistTerm — 界面设计规范

**交付对象**: Rust/egui 前端开发  
**版本**: v1.0  
**基于代码**: `src/ui/app.rs` + `src/ui/theme.rs` + `src/ui/chrome.rs` + `src/ui/layout_util.rs`  
**关联面板**: `monitor_panel.rs` · `fragment_library.rs` · `credential_panel.rs` · `cloud_sync_panel.rs` · `sftp_panel.rs`  
**设计原则**: 每项设计严格对应代码中已有实现

---

## 一、界面总览

```
┌─ 菜单栏 ───────────────────────────────────────────────────────────┐
│ ✦ MistTerm │ 文件 │ 视图 │ 工具 │ 帮助                            │
├─ 标题栏 ───────────────────────────────────────────────────────────┤
│  [状态点]  连接信息  IP      状态圆点  配置导入提示                 │
├─ 右侧Dock(可选) ───┬─ 左侧栏 ───┬─ 终端工作区 ────────────────────┤
│                    │            │                                  │
│  显示条件:         │ SSH配置导入 │  终端仿真器                     │
│  show_monitor      │ 提示(条件)  │  SSH连接 + VTE渲染              │
│  show_sftp         │            │  命令提示符 + 输出               │
│  fragment_panel    │ 🔍 搜索    │  光标闪烁                       │
│  credential_panel  │            │  搜索覆盖层(⌘F)                  │
│  cloud_sync_panel  │ 会话列表   │  Ctrl+R 覆盖层(⌘⇧J)             │
│                    │ * 色点标记  │  大文件上传确认                  │
│                    │ * 状态指示  │                                  │
│                    │ * 选中高亮  │                                  │
│                    │            │                                  │
├────────────────────┴────────────┴──────────────────────────────────┤
│ 状态栏: 当前连接  CPU/MEM  自动重连状态  日志记录  工具图标         │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 二、菜单栏

### 2.1 总体规范

| 属性 | 值 |
|------|-----|
| 高度 | `title_bar_height()` |
| 背景色 | `bg_tab_bar_color()` |
| 字体 | `font_size_menu_item()` |
| 文字默认色 | `fg_medium_color()` |
| 文字悬停色 | `fg_high_color()` |
| 悬停背景 | `bg_hover_color()` |
| 下拉菜单背景 | `color_panel_surface()` |
| 下拉菜单边框 | 1px `border_color()`, 圆角 8px, 阴影 |
| 分隔线 | 1px, `border_color()`, margin 3px 8px |

下拉菜单最小宽度: 200px  
菜单项 padding: 5px 10px  
快捷键颜色: `fg_low_color()`  
选中态(主题子菜单): ✓ + `accent_color()`

### 2.2 菜单条目

#### 文件菜单

| 菜单项 | 快捷键 | 代码入口 | 行为 |
|--------|--------|----------|------|
| 新建会话 | ⌘N | `show_new_session_dialog = true` | 打开新建会话对话框(§6.1) |
| 偏好设置 | ⌘, | `show_preferences_dialog = true` | 打开偏好设置对话框(§6.2) |
| — | | | 分隔线 |
| 关闭标签 | ⌘W | `request_close_active_tab()` | 关闭当前Tab；连接中则弹出确认(§6.6) |
| 断开 SSH（保留输出） | | `disconnect_ssh_keep_buffer_active()` | 断开SSH但终端内容不清除，可浏览历史输出 |
| 重连当前标签 | | `reconnect_active_tab()` | 重新建立SSH连接，复用已有Tab |
| — | | | 分隔线 |
| 退出 | | `frame.close()` | 关闭应用 |

#### 视图菜单

| 菜单项 | 代码入口 | 行为 |
|--------|----------|------|
| 折叠侧边栏 / 展开侧边栏 | `sidebar_collapsed = !sidebar_collapsed` | 切换左侧栏显隐 |
| — | | 分隔线 |
| ✓ SFTP 文件面板 | `show_sftp_panel = true/false` | 切换SFTP侧栏开关，✓表示当前开启 |
| — | | 分隔线 |
| 主题 → 子菜单(▸) | `theme_manager.set_theme_index(i)` | 展开子菜单列出所有主题 |

**主题子菜单**：当前选中主题前有 ✓ 标记。数据来自 `theme_manager.list_themes()`。当前内置主题：暗夜、深灰、极光、暖阳。

#### 工具菜单

| 菜单项 | 代码入口 | 行为 |
|--------|----------|------|
| 命令片段库… | `fragment_library.open = true` | 打开命令片段库对话框(§6.7) |
| 凭证管理 | `credential_panel.open = true` | 打开凭证侧栏 |
| 云端同步 | `cloud_sync_panel.open = true` | 打开云端同步侧栏 |

#### 帮助菜单

| 菜单项 | 代码入口 | 行为 |
|--------|----------|------|
| 关于 | `show_about_dialog = true` | 打开关于对话框(§6.3) |

---

## 三、标题栏

### 3.1 位置

紧接在菜单栏下方，由 `TopBottomPanel::top("title_bar")` 渲染。

### 3.2 布局

```
┌─────────────────────────────────────────────────────────────────┐
│ ● ● ●  连接名 · IP              连接状态 🟢  ⚡ N个待导入 ×  │
└─────────────────────────────────────────────────────────────────┘
```

| 区域 | 类型 | 说明 |
|------|------|------|
| ● ● ● | macOS 红绿灯 | `-webkit-app-region: no-drag`, 仅 macOS 显示 |
| 连接名 · IP | 文本 | `connection_server_text()`, 字号 `font_size_normal()` |
| 连接状态 | 圆点+文字 | 🟢 在线 / ⚫ 已断开 / ⏳ 连接中 |
| ⚡ N个待导入 | 条件显示 | 检测到 `~/.ssh/config` 有未导入配置时显示 |
| ✕ | 关闭按钮 | 关闭导入提示 |

### 3.3 高度

`theme.title_bar_height()` — 与菜单栏 `title_bar_height()` 共用同一常量。

---

## 四、左侧栏

### 4.1 整体

| 属性 | 值 |
|------|-----|
| 宽度 | `sidebar_width`（持久化存储，可拖拽，初始值 200px） |
| 背景色 | `bg_window_color()` |
| 显示控制 | `sidebar_collapsed` 布尔开关 |
| 响应式 | `RESP_LAYOUT_WIDE_MIN_PX`（1200px）以下自动折叠 |

### 4.2 SSH 配置导入提示

仅在检测到未导入的 Host 时渲染，始终位于左侧栏顶部。

| 元素 | 内容 |
|------|------|
| 图标 | ⚡ 黄色 |
| 文字 | "检测到 N 个待导入的 SSH 配置" |
| 按钮 | "导入" → 触发导入弹窗 |
| 关闭 | ✕ → 关闭提示（dismiss） |

- 背景: 渐变深色 `bg_medium_color()` → `bg_body`
- 底部边框: `border_color()`, 1px
- 高度: ~34px

### 4.3 搜索框

实时过滤会话列表。`focus_sidebar_connection_search(ctx)` 快捷键 ⌘J。

| 属性 | 值 |
|------|-----|
| 图标 | 🔍 |
| 提示文字 | "搜索会话..." |
| 背景 | `color_subtle_inset_fill()` |
| 圆角 | 4px |
| padding | 7px 10px |

### 4.4 会话列表

每行代表一个 SSH 连接配置，数据结构来自 `SessionManager`。色点仅区分连接状态，分组颜色管理为未来扩展项。

**行布局**:
```
┌─────────────────────────────────────────────────────┐
│ ▌ ●  会话名                刚刚/5m/2h/离线          │
│ ▌ 色点 12px 主色           10px 灰 状态文字         │
└─────────────────────────────────────────────────────┘
```

**色点含义**:

| 颜色 | 含义 | 代码 |
|------|------|------|
| 🟢 绿色 | 在线/活跃连接 | `green_color()` |
| ⚫ 灰色 | 已断开/离线 | `fg_high_a64()` |
| 🟡 黄色 | 临时/特殊用途 | `#ffc832` |
| ⚫ 灰色 | 已断开/离线 | `#555` |
| ◯ 空心 | 新建未连接 | 1.5px `#444` 边框，无填充 |

色点大小: 5px

**状态文字**: 连接时显示相对时间（"刚刚" / "5m" / "2h" / "3d"），断开时显示 "离线"。

**交互状态**:

| 状态 | 样式 |
|------|------|
| 默认 | 无背景 |
| 悬停 | `bg_hover_color()` |
| 选中 active | `bg_selected_color()` + 左侧 3px `accent_color()` 竖线 |
| 字体颜色 | `fg_high_color()` (主要文字) |
| 次文字/IP | `fg_low_color()` |

**选中交互**:
- 鼠标点击选中
- Delete 键 → 触发 `delete_session_confirm`
- ⌘E → 触发 `open_edit_session_dialog`

---

## 五、终端工作区

### 5.1 终端仿真器

由 `CentralPanel` 渲染，基于 VTE 的 Rust 实现。

| 属性 | 值 |
|------|-----|
| 背景 | `#06060a`（终端特有深色） |
| 字体 | `font_family_mono()` |
| 字号 | `font_size_terminal()` |
| 行高 | 1.6 |
| 提示符颜色 | `accent_color()` (#667ae9) |
| 命令输入颜色 | `fg_high_color()` |
| 输出颜色 | `fg_medium_color()` |
| 错误输出 | `red_a128()` (#f66) |
| 光标 | 7px × 14px, `accent_color()`, 1s 闪烁 |

### 5.2 快捷键

| 快捷键 | 功能 | 代码 |
|--------|------|------|
| ⌘N | 新建会话 | `show_new_session_dialog = true` |
| ⌘T | 新建Tab | `open_new_tab_from_selection()` |
| ⌘W | 关闭当前Tab | `request_close_active_tab()` |
| ⌘J | 定位到左侧搜索框 | `focus_sidebar_connection_search(ctx)` |
| ⌘K | 打开片段面板搜索 | `focus_fragment_panel_search(ctx)` |
| ⌘E | 编辑当前选中会话 | `open_edit_session_dialog(sid)` |
| ⌘H | 打开关于 | `show_about_dialog = true` |
| ⌘, | 偏好设置 | `show_preferences_dialog = true` |
| ⌘F / ⌃F | 终端内容搜索 | `show_terminal_search` 切换 |
| ⌘⇧J / ⌃⇧J | 快速片段选择器 | `quick_selector.open = true` |
| ⌘⇥ / ⌘⇧⇥ | 下一个Tab / 上一个Tab | `switch_to_next_tab()` / `switch_to_prev_tab()` |
| ⌘1…⌘9 | 切换到第 N 个Tab | `switch_to_tab(n-1)` |
| Delete | 删除选中会话 | `delete_session_confirm` |

### 5.3 Tab 栏

由 `TopBottomPanel` 中的 tab bar 区域渲染。

**Tab 元素**:
| 部分 | 说明 |
|------|------|
| 色点 | 🟢 在线 / ⚫ 离线，5px 圆点 |
| Tab 标题 | 会话名 |
| 状态 | 分屏标示(2屏/3屏等) |
| ✕ | 关闭按钮 |

**交互**:
- 激活 Tab: 底部高亮或背景变化
- 点击切换 Tab: `switch_to_tab(idx)`
- 点击 ✕: `request_close_active_tab()`（连接中弹出确认）

### 5.4 终端搜索覆盖层（⌘F）

| 属性 | 值 |
|------|-----|
| 触发 | ⌘F / ⌃F |
| 功能 | 在当前终端内容中搜索文本 |
| 数据 | `rebuild_terminal_search_matches()` |

---

## 六、对话框

所有对话框统一规范:

| 属性 | 值 |
|------|-----|
| 定位 | `CENTER_CENTER`, `anchor(Align2::CENTER_CENTER)` |
| movable | `false` |
| resizable | `false` |
| collapsible | `false` |
| title_bar | `false`（自定义 header） |
| frame | `modal_window_frame(&theme)` |
| 内容容器 | `modal_content_frame(&theme)` |
| header | `modal_header(ui, theme, title, &mut close)` |
| 底部按钮 | `modal_footer_actions()` |

### 6.1 新建会话对话框

| 属性 | 值 |
|------|-----|
| 尺寸 | `modal_edit_size(ctx)` → `(sw×36% [340~520], sh×48% [360~540])` |
| 状态变量 | `show_new_session_dialog` |

**表单字段**:

| 字段 | 控件类型 | Hint文本 | 必填 |
|------|---------|----------|------|
| 会话名称 | `TextEdit::singleline` | "例: 生产服务器-01" | ✅ |
| 主机地址 | `TextEdit::singleline` | "IP 或域名" | ✅ |
| 端口 | `DragValue` clamp(1..65535) | 默认 22 | ✅ |
| 用户名 | `TextEdit::singleline` | "root" | |
| 密码 | `TextEdit::singleline` + `password(true)` | "可留空" | |
| SSH 私钥路径 | `TextEdit::singleline` | "~/.ssh/id_rsa（留空则用密码或系统默认密钥）" | |
| 分组 | `TextEdit::singleline` | "默认分组" | |

**验证**:
- 名称和主机都为空时, `required_missing = true`
- 显示红色提示: "请先填写会话名称和主机地址"
- [保存并连接] 按钮 `add_enabled(!required_missing)`
- Enter 提交（`ui.input() |i| i.key_pressed(Key::Enter)`）

**按钮**: [保存并连接]（primary） | [取消]（secondary）

**关闭行为**: `reset_new_session_form()` 清空表单

### 6.2 偏好设置对话框

| 属性 | 值 |
|------|-----|
| 尺寸 | `modal_pref_size(ctx)` → `(sw×40% [380~560], sh×42% [320~560])` |
| 状态变量 | `show_preferences_dialog` |

**外观区域**:
- 标题: "外观"
- 主题选择列表: 遍历 `theme_manager.list_themes()`, 每个主题名作为按钮
- 当前选中主题前显示 "✓"
- 点击 → `theme_manager.set_theme_index(i); theme_manager.save()`

**连接区域**:
- 标题: "连接"
- 复选框: "网络断开后自动重连（最多 5 次，指数退避）"
- 绑定: `auto_reconnect_enabled`
- hover tooltip: 说明"FUNCTIONAL_SPEC §1.4"

**同步与数据区域**:
- 标题: "同步与数据"
- 链接按钮: "打开云端同步…" → 关闭本对话框，打开 `cloud_sync_panel`

**底部提示**:
"其余项请用顶部菜单：视图、工具、帮助。"

**按钮**: [关闭]（secondary）

### 6.3 关于对话框

| 属性 | 值 |
|------|-----|
| 尺寸 | `modal_about_size(ctx)` → `(sw×38% [360~520], sh×44% [340~540])` |
| 状态变量 | `show_about_dialog` |

**内容**:
- 应用名称: "MistTerm" — 使用 `font_size_prominent()`
- 副标题: "一个现代化 SSH 终端工具"
- 版本信息框（带边框和圆角）

| 行 | 内容 |
|---|------|
| 1 | 版本: v0.1.0 |
| 2+ | 快捷键列表 (等宽字体 10px, `ScrollArea` 最大高 200px) |

- 快捷键数据来源: `mistterm_functional_spec_shortcuts()`

**按钮**: [关闭]（secondary）

### 6.4 编辑会话对话框

| 属性 | 值 |
|------|-----|
| 尺寸 | `modal_edit_size(ctx)` |
| 状态变量 | `show_edit_session_dialog` |

表单字段**同新建会话对话框**(§6.1)，但预填当前会话数据（`edit_session_*` 字段）。

**密码字段特殊行为**:
- 编辑时密码 hint: "**** 表示沿用原密码；改为新口令以保存新密码"
- 空密码 → 保留原密码

**验证**: 同新建，名称/主机为空时红色提示

**按钮**: [保存]（primary） | [取消]（secondary）
- Enter 提交

### 6.5 删除确认对话框

| 属性 | 值 |
|------|-----|
| 尺寸 | `modal_clone_size(ctx)` → `(sw×38% [340~520], sh×26% [180~320])` |
| 状态 | `delete_session_confirm: Option<(String, String)>` — `(session_id, session_name)` |
| 触发 | 选中会话后按 Delete 键 |

**内容**:
- 标题: "删除会话"
- 确认文案: `"确认删除「{name}」的会话配置？此操作不可恢复。"`

**按钮**: [删除]（danger, `modal_danger_button`） | [取消]（secondary）

**关闭行为**:
- 确认删除 → `self.delete_session(&del_id)`
- 取消/关闭 → `self.delete_session_confirm = None`

### 6.6 关闭标签确认对话框

| 属性 | 值 |
|------|-----|
| 尺寸 | `modal_clone_size(ctx)` |
| 状态 | `close_tab_confirm_idx: Option<usize>` |

仅当正在关闭的标签仍处于连接或握手中时弹出。

**内容**:
- 标题: "关闭标签"
- 确认文案: `"标签「{title}」仍连接或握手中，确定关闭？"`

**按钮**: [关闭]（primary） | [取消]（secondary）
- 确认 → `self.remove_tab_at(pending_idx)`

### 6.7 大文件上传确认对话框

| 属性 | 值 |
|------|-----|
| 尺寸 | `modal_quick_fragment_size(ctx)` → `(sw×42% [360~560], sh×32% [220~380])` |
| 状态 | `large_upload_pending_path: Option<PathBuf>` |
| 触发 | 文件 ≥ 10MB 时自动弹出 |

**内容**:
- 标题: "大文件上传"
- 文案: `"「{path}」≥ 10MB：SCP 无断点续传；ZMODEM 需远端 lrzsz，并将向 PTY 发送 rz -y。"`

**按钮**:
| 按钮 | 类型 | 行为 |
|------|------|------|
| ZMODEM（推荐） | primary | `queue_zmodem_upload_after_rz()` |
| 仍用 SCP | secondary | `start_upload()` |
| 取消 | secondary | `dismiss` |

### 6.8 命令片段对话框

| 属性 | 值 |
|------|-----|
| 尺寸 | `modal_confirm_size(ctx)` → `(sw×36% [320~480], sh×24% [160~280])` |
| 状态 | `show_fragments_dialog` |

**内容**:
- 标题: "命令片段"
- 提示: "提示：点击底部「命令片段」按钮打开侧边栏面板"

---

## 七、右侧 Dock

### 7.1 通用规范

| 属性 | 值 |
|------|-----|
| 宽度 | `side_panel_widths(ctx, profile)` — 每个面板有独立的 default/min/max |
| 显示 | 以 `SidePanel::right` 注册 |
| 响应式 | `RIGHT_DOCK_OPEN_ALLOWED` 阈值 `RESP_LAYOUT_WIDE_MIN_PX`（1200px）以下不能打开 |
| 位置记录 | `right_dock_outer_left_x: Option<f32>` |
| 顺序注册 | 右侧面板须先于底栏与 CentralPanel 注册，避免叠绘错位 |
| Central 盖住右栏 | egui 同层后绘问题；命令片段栏用「占位 SidePanel + Foreground 重绘」。**实现与排障见 [`LAYOUT.md` §八](LAYOUT.md#八central-盖住右栏白膜--裁切--可点穿)** |

### 7.2 监控面板 (`monitor_panel`)

**状态结构**:
```rust
pub struct MonitorPanel {
    monitor: Option<Monitor>,          // 监控器
    auto_refresh: bool,                // 自动刷新，默认开启
    refresh_interval_secs: f32,        // 刷新间隔(秒)
    alert_cpu_pct: f32,                // CPU 告警阈值(%)
    alert_mem_pct: f32,                // 内存告警阈值(%)
    alert_disk_pct: f32,               // 磁盘告警阈值(%)
    last_ui_refresh: f64,              // 上次UI刷新时间
    last_error: Option<String>,        // 最后一次错误
    refresh_label: String,             // 刷新按钮标签
    pending_raw: Option<Receiver<...>>,// 远程采集结果通道
}
```

**面板内容**:

**标题栏**:
- "📊 系统监控"（`font_size_xl()`）
- 告警计数: 有告警时显示 "⚠ N 项告警"（红色）
- 关闭按钮 (✕), tooltip: "隐藏侧栏 · 也可用底部「📊 监控」切换"

**控制栏**:
- 复选框: "自动刷新" — `auto_refresh`
- 当自动刷新开启时显示 `Slider::new(interval, 1.0..=30.0)`, 后缀 "s"
- "🔄 刷新" 按钮 — `pending_raw.is_none()` 时才可点击

**告警阈值折叠**（默认折叠）:
- 说明文字: "超出阈值时在标题与下方显示告警（仅当前会话）"
- CPU 告警 %: `Slider [50..100]`
- 内存告警 %: `Slider [50..100]`
- 磁盘告警 %: `Slider [50..100]`

**告警区**（有告警时显示）:
- 背景框: `frame_monitor_alert()`
- 每行一条告警文字

**指标区域**:

| 指标 | 图标 | 数据格式 | 颜色规则 |
|------|------|---------|---------|
| 运行时间 | ⏱ | `format_uptime()` | `fg_medium_color()` |
| CPU | 🖥 | `{:.1}%` | ≥80% 红, ≥60% 黄, 其余 `accent_color()` |
| 内存 | 💾 | `format_memory()` | 同CPU颜色规则 |
| 磁盘 | 💿 | `format_disk()` | 同CPU颜色规则 |

每个指标用 `show_metric_bar()` 渲染: 图标+标签 | 进度条(3px高) | 数值

**系统负载**:
- 标题: "📊 系统负载"
- 三个 `load_chip`: 1m / 5m / 15m

**网络速率**:
- 标题: "🌐 网络速率"
- ↓ 下载速率 / ↑ 上传速率
- 单位自适应: B/s, KB/s, MB/s

**历史图表**:
- `get_history()` — 最近N帧的指标历史
- 折线/柱状图展示趋势

### 7.3 SFTP 面板 (`sftp_panel`)

| 属性 | 值 |
|------|-----|
| 开关 | `show_sftp_panel` |
| 显示 | `sftp_panel.show_side_panel()` |
| 宽度 | `side_panel_widths(ctx, SidePanelProfile::Sftp)` |

### 7.4 凭证管理面板 (`credential_panel`)

| 属性 | 值 |
|------|-----|
| 开关 | `credential_panel.open` |
| 显示 | `credential_panel.show_side_panel()` |
| 宽度 | `side_panel_widths(ctx, SidePanelProfile::Credential)` |

**功能**:
- 凭证列表: 🔑 图标 + 主机名(用户) + 认证类型
- 点击应用凭证: `apply_credential_to_new_session_form(c)`

### 7.5 云端同步面板 (`cloud_sync_panel`)

| 属性 | 值 |
|------|-----|
| 开关 | `cloud_sync_panel.open` |
| 处理 | `cloud_sync_panel.show(ctx, theme)` |

### 7.6 命令片段侧栏 (`fragment_panel`)

| 属性 | 值 |
|------|-----|
| 开关 | `show_fragment_panel` |
| 宽度 | `side_panel_widths(ctx, SidePanelProfile::Fragment)` |
| 触发 | 底部「命令片段」按钮 / 快捷键 ⌘K |

**标题栏**:
- "命令片段" 标题
- "➕ 新建…" 按钮 → `fragment_library.open = true`
- 排序切换按钮: 🔢 次数 / ✅ 成功率 / 🕐 最近 / 🔤 名称
- ✕ 关闭按钮

**搜索过滤**: `fragment_filter_category` 支持 "常用" / "Docker" / "K8s" / "全部"

### 7.7 Git 同步面板 (`git_sync_panel`)

| 属性 | 值 |
|------|-----|
| 开关 | `show_git_sync_panel` |
| 宽度 | `side_panel_widths(ctx, SidePanelProfile::GitSync)` |
| 渲染 | `git_sync_panel.show(ui, theme)` |

---

## 八、状态栏

### 8.1 位置

`TopBottomPanel::bottom("status_bar")`，紧接在 CentralPanel 下方。

### 8.2 规范

| 属性 | 值 |
|------|-----|
| 高度 | `status_bar_height()` |
| 背景 | `bg_tab_bar_color()` |
| 字号 | `font_size_small()` |
| 文字颜色 | `fg_low_color()` |

### 8.3 布局

```
┌─────────────────────────────────────────────────────────────────┐
│ ● 连接名  |  📊 CPU · MEM  |  ↻ 自动重连  |  📝 日志          │
└─────────────────────────────────────────────────────────────────┘
```

| 区域 | 元素 | 说明 |
|------|------|------|
| 左端 | ● 连接名 | 当前连接状态圆点 + 名称 |
| 中间区 | 📊 CPU 23% · MEM 1.2G/4G | 系统资源占用（来自 monitor_panel） |
| | ↻ 自动重连 | 自动重连启用状态 |
| | 📝 日志 | 日志录制状态（文件日期） |
| 右端 | 工具图标 | `status_tool_glyph()` — 📊 监控 / 🔍 搜索 / 📤 导出 |

右侧工具图标:
- 📊 → `show_monitor_panel = true`
- 🔍 → 搜索功能
- 📤 → 数据导出

---

## 九、响应式行为

### 9.1 窗口宽度阈值

| 宽度 | 行为 | 代码 |
|------|------|------|
| ≥ 1200px (`RESP_LAYOUT_WIDE_MIN_PX`) | 完整三栏 | `right_dock_open_allowed()` = true |
| < 1200px | 右侧 Dock 隐藏 | 菜单项点击右侧面板时显示提示 |
| 小于阈值 | 侧边栏自动折叠 | `responsive_sidebar_auto_collapse()` |

### 9.2 对话框自适应

所有对话框尺寸均为百分比 + clamp:

| 对话框 | 宽公式 | 高公式 |
|--------|--------|--------|
| 新建/编辑会话 | sw×36% [340,520] | sh×48% [360,540] |
| 偏好设置 | sw×40% [380,560] | sh×42% [320,560] |
| 关于 | sw×38% [360,520] | sh×44% [340,540] |
| 大文件上传 | sw×42% [360,560] | sh×32% [220,380] |
| 删除/关闭确认 | sw×38% [340,520] | sh×26% [180,320] |
| 通用小确认 | sw×36% [320,480] | sh×24% [160,280] |

---

## 十、UI 组件规范

### 10.1 按钮

| 类型 | 样式 | 函数 |
|------|------|------|
| Primary | `accent_color()` 背景, 白色文字 | `modal_primary_button_widget()` |
| Secondary | `bg_medium_color()` 背景, `fg_medium_color()` 文字 | `modal_secondary_button()` |
| Danger | 红色背景, 白色文字 | `modal_danger_button()` |
| Icon(关闭) | 透明背景, 悬停变红 | `close_icon_button()` |
| Status tool | 透明, 底部状态栏用 | `status_tool_glyph()` |

### 10.2 输入框

| 属性 | 值 |
|------|-----|
| 背景 | `color_panel_surface()` |
| 边框 | 1px, `fg_high_alpha(8)` |
| 圆角 | 4px |
| 文字颜色 | `fg_high_a179()` |
| Hint颜色 | `color_form_hint()` |

### 10.3 表单标签

`ui_field_label()` — 字号 `font_size_small()`，颜色 `color_form_label()`

### 10.4 Frame 类型

| Frame | 用途 |
|-------|------|
| `modal_window_frame` | 对话框外框 |
| `modal_content_frame` | 对话框内容容器 |
| `region_panel_frame` | 侧栏、面板区域 |
| `status_chip` | 状态徽章 — 淡色底, padding 2px 8px, 圆角 4px, 11px |

### 10.5 分隔线

1px, `border_color()`
- 菜单内: margin 3px 8px
- 面板内: full width

---

## 十一、与已有文档的关系

| 文档 | 关系 |
|------|------|
| [`SPECIFICATION_DETAILED.md`](./SPECIFICATION_DETAILED.md) | 定义颜色体系、字号、圆角的具体值；本文档引用 theme 令牌名 |
| [`FUNCTIONAL_SPEC.md`](./FUNCTIONAL_SPEC.md) | 定义功能逻辑与边界条件；本文档描述 UI 布局与交互 |
| [`LAYOUT.md`](./LAYOUT.md) | 当前布局真源（egui 区域注册顺序、间距、底栏） |
| [`docs/archive/`](../archive/) | 历史稿：`P0功能详细设计.md`（已落地的新增功能设计）、`改造设计规范.md`（自适应布局改造方案） |