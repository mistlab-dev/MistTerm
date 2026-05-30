# MistTerm 布局

---

## 一、窗口结构（ASCII）

```
┌─ 系统窗口标题（macOS 原生，不可控）────────────────────────┐
├─ top_chrome 28px（仅菜单行）───────────────────────────────┤
│ 文件 · 视图 · 工具 · 帮助              （可选）⚡待导入 ×   │
├─ main work area（bg #0d0d14，外 padding 8px）─────────────┤
│ ┌─左栏 panel─┐ gap ┌─terminal area─┐ gap │ right dock   │
│ │ 连接/搜索  │ 6px │ Tab + VTE     │ 6px │ (SidePanel)  │
│ └────────────┘     └───────────────┘     │              │
├─ bottom_chrome 32px ─────────────────────────────────────┤
│ 连接信息 · chip · 复原按钮      [片段][SFTP][监控] · 统计      │
└──────────────────────────────────────────────────────────┘

浮动层（最后绘制）：Ctrl+R、终端搜索条、各类 Window 弹窗
```

---

## 二、egui 注册顺序（禁止随意调整）

顺序错误会导致右栏与终端叠绘错位（「花屏」）。

| 步骤 | 类型 | ID / 说明 |
|------|------|-----------|
| 1 | `TopBottomPanel::top` | `"top_chrome"`（仅菜单行 28px，见 `render_top_chrome_panel`） |
| 2 | `SidePanel::right` | 片段 → Git → 凭证 → 云同步 → SFTP → 监控（以 `workspace.rs` 为准） |
| 3 | `TopBottomPanel::bottom` | `"bottom_chrome"` **必须在 Central 之前** |
| 4 | `CentralPanel` | 工作区三列（左栏 + 终端） |
| 5 | `Area`（`Order::Foreground`） | **监控 / 命令片段正文**（须在 Central 之后，见 §八） |
| 6 | `Window` / `Area` | 弹窗、终端搜索、命令历史覆盖层 |

> **注意：** 步骤 2 的 `SidePanel::right("fragment_panel")` 仅做**布局占位**（透明 Frame）；可见 UI 在步骤 5 重绘。其它右 dock 若出现同类「白膜盖住、仍可点穿」，按 §八 同样处理。

---

## 三、区域 → 代码映射

| 区域 | 职责 | 主文件 |
|------|------|--------|
| 顶栏 | 仅菜单行；连接在 Tab + 底栏 | [`src/ui/chrome.rs`](../src/ui/chrome.rs) `render_top_chrome_panel` |
| 顶栏菜单项 | 文件 / 视图 / 工具 / 帮助 | [`src/ui/app.rs`](../src/ui/app.rs) `show_application_menu_bar` |
| 底栏 | 连接点、监控 chip、日志 chip、工具按钮、统计 | [`src/ui/app.rs`](../src/ui/app.rs) `show_bottom_chrome` |
| 工作区编排 | 注册顺序、三列、外 padding | [`src/ui/workspace.rs`](../src/ui/workspace.rs) |
| 左栏 | SSH 导入条 + 连接/搜索/筛选/列表 | [`src/ui/sidebar.rs`](../src/ui/sidebar.rs) `Sidebar::show_column` |
| 终端 | Tab 栏 + PTY 视图 | [`src/ui/terminal.rs`](../src/ui/terminal.rs) |
| 右 dock 片段（布局占位） | `SidePanel` 仅占宽 | [`src/ui/app.rs`](../src/ui/app.rs) `show_fragment_panel` |
| 右 dock 片段（可见 UI） | `Area` Foreground 重绘 | [`src/ui/app.rs`](../src/ui/app.rs) `show_fragment_panel_foreground` |
| 右 dock SFTP | 文件浏览 | [`src/ui/sftp_panel.rs`](../src/ui/sftp_panel.rs) |
| 右 dock 监控 | CPU/内存等 | [`src/ui/monitor_panel.rs`](../src/ui/monitor_panel.rs) |
| 布局数学 | `central_work_rect`、`work_area_inner_rect`、列宽 | [`src/ui/layout_util.rs`](../src/ui/layout_util.rs) |
| 设计 token | 颜色、间距、圆角、Frame 工厂 | [`src/ui/theme.rs`](../src/ui/theme.rs) |
| 弹窗/覆盖层 | 会话编辑、偏好、导入、搜索等 | [`src/ui/workspace.rs`](../src/ui/workspace.rs) `render_overlays` |

---

## 四、间距与尺寸（theme）

| Token | 值 | 用途 |
|-------|-----|------|
| `spacing_work_area_pad` | 8px | 工作区相对 `available_rect` 外圈留白 |
| `spacing_region_gap` | 6px | 左栏｜终端｜右 dock 列间缝（露出 `bg_body`） |
| `title_bar_height` | 36px | 顶栏 |
| `status_bar_height` | 32px | 底栏 |
| `radius_panel` | 6px | 左栏、终端外框 |
| `frame_region_panel` | — | 左栏圆角面板 |
| `frame_terminal_column` | — | 终端列 |

左栏内部顺序（自上而下）：

1. SSH 导入提示条（可选，~34px，可 dismiss）
2. 圆角面板：连接标题 + 排序 + ＋/− → 搜索 → 全部/在线/离线 → 会话列表

---

## 五、覆盖层 Z 序

| 层级 | 内容 |
|------|------|
| L0 | 左 `SidePanel` + 各右 `SidePanel`（含片段**占位槽**） |
| L0b | `CentralPanel`（左栏 + 终端；**同 `LayerId::background`，但注册更晚，会盖住同层右栏像素**） |
| L1 | 命令片段 `Area`（`mistterm_fragment_fg`，`Order::Foreground`） |
| L2 | 终端内 Ctrl+R（`command_history_overlay`） |
| L3 | 终端列底部内嵌查找条（⌘F / 底栏 🔍） |
| L4 | 模态对话框（新建/编辑会话、偏好、导入等） |

---

## 八、Central 盖住右栏（白膜 / 裁切 / 可点穿）

改造或新增右 `SidePanel` 时注意以下约束。2026-05 命令片段栏踩坑总结。

### 8.1 现象（勿误判为「侧栏内部宽度」）

| 表现 | 说明 |
|------|------|
| 右栏文字发灰、像蒙一层白/浅色 | 中央区**后绘制**的底色/终端白底叠在侧栏**上面**（仅像素，不是布局挤窄） |
| 右缘文字被「切掉」 | 曾用 `clip_rect`（常为**整窗宽**）做 `set_max_width`，内容按窗宽排版后在侧栏右内缘被裁 |
| **整块侧栏偏出屏、看不到右边框** | `PanelState` 存的是**内层内容**矩形；Foreground 若直接 `fixed_pos(PanelState.min)` 且宽≈窗宽，整块会伸出 `screen_rect` 右缘被裁 |
| **能点透**：挡显示但能点到下面片段 | 典型 **paint 盖住、hit-test 仍在下层 SidePanel/Foreground 之前那层** |

### 8.2 根因（egui 0.23 行为）

1. **注册顺序 = 同层绘制顺序**  
   `SidePanel` 与 `CentralPanel` 均使用 `LayerId::background()`。  
   **Central 必须最后注册**（用于扣减 `available_rect`），因此 Central 的 Frame / `painter` **一定画在右 SidePanel 之上**。

2. **SidePanel 根 `Ui` 的 `clip_rect` 常为整窗**  
   应用 `ui.max_rect().width()` 作为侧栏内容宽；**勿优先**用 `clip_rect.width()` 给侧栏 `set_max_width`（见 [`side_panel_row_width`](../src/ui/layout_util.rs)）。

3. **终端列挂在 `horizontal` 上未 allocate 固定宽**  
   `frame_terminal_column().show(ui, …)` 直接挂在 `horizontal` 上会吃满剩余宽，白底向右铺，加剧盖住右栏。

4. **在中央对过大矩形 `rect_filled`**  
   若 `work` 未按 `right_dock_outer_left_x` 收紧，或 `clip_rect` 为整窗，会铺出「白膜」。

### 8.3 现行方案（命令片段栏）

**双通道：布局占位 + Foreground 重绘**

```
帧内顺序：
  SidePanel::right("fragment_panel")   // 透明 Frame，allocate 槽位 → PanelState.rect
  … 其它右 dock …
  bottom_chrome
  CentralPanel                         // 左栏 + 终端；bg_body 仅 painter 填入 work（见下）
  Area::Foreground("mistterm_fragment_fg")  // 按 right_dock_slot_rect（右缘钉屏）画完整片段 UI
  Window / 其它覆盖层
```

| 步骤 | 函数 | 职责 |
|------|------|------|
| 占位 | `show_fragment_panel` / `show_side_panel` | 回调内 **`ui.max_rect()`** 作槽位 → [`record_right_dock_panel_rect`](../src/ui/layout_util.rs)（与中央区交界一致，避免黑缝） |
| 正文 | `show_*_foreground` | [`paint_right_dock_slot_shell`](../src/ui/chrome.rs) **铺满槽位** + `allocate_ui_at_rect` 内容区（勿仅用 `region_panel_frame` 包内容，左侧会透出 Central `bg_body`） |
| 调度 | `workspace.rs` | Central 结束后：**先**监控 FG、**再**片段 FG（靠右的后画） |

常量：[`FRAGMENT_PANEL_ID`](../src/ui/layout_util.rs)、[`MONITOR_PANEL_ID`](../src/ui/layout_util.rs) 与对应 `SidePanel` id 一致。

### 8.4 中央区底色（避免黑边，且不盖住右栏）

- **不要**在 `CentralPanel::default().frame(...)` 上铺 `bg_body`（整槽 Frame 仍会盖住右栏像素）。
- **要**在 `CentralPanel::show` 内、收紧后的 `work` 上：

```rust
let work = layout_util::central_work_rect_in_ui(ui, right_dock_outer_left_x);
ui.set_clip_rect(work);
ui.painter()
    .with_clip_rect(work)
    .rect_filled(work, 0.0, theme.bg_body_color());
```

- 终端列必须先 `allocate_ui_with_layout(vec2(term_col_w, h), …)` 再 `frame_terminal_column().show`。

### 8.5 侧栏内容宽度（防「内部右缘裁切」）

使用 [`layout_util::side_panel_row_width`](../src/ui/layout_util.rs) / [`dock_panel_content_width`](../src/ui/layout_util.rs)：

- 以 `ui.max_rect().width()` 为准；
- 仅当 `clip_rect.width() < max_rect.width()` 时再取 `min`（clip 更窄才信 clip）；
- **禁止**对 SidePanel 根 `Ui` 使用 `set_max_width(整窗 clip 宽)`。

### 8.6 禁止 / 慎用

| 不要做 | 原因 |
|--------|------|
| 只在 `SidePanel` 回调里画片段正文、指望调宽度解决 | Central 后绘仍会盖住 |
| `CentralPanel` 上用 `frame_central_workspace()` 满铺 | 同层盖住右栏 |
| 用 `ctx.available_rect()` 在 Central 内算 `work` 却不收紧右缘 | 易铺到右栏区域 |
| `finite_content_width_inset(..., max: 2000)` 在右栏内 | 图表/列表撑出槽位 |
| 把 `record_right_dock_panel` 的 rect 校验设得过严导致从不记录 | `right_dock_outer_left_x` 为 `None`，中央不裁剪 |
| `Area::order(Tooltip)` + `constrain_to(screen)` | Area 内 `max_rect` 拉到屏右下 → **全窗点不了**；列表 `max_rect` 悬停出现大片白/透明 |
| 右 dock Area 约束 | `Order::Middle` + `constrain_to(paint)`（在 Central 之上、模态 Window 之下）；列表行先 `allocate_at_least` 再铺 hover 底 |

### 8.7 扩展到其它右 dock

监控 / SFTP / 凭证 / Git / 云同步若出现**同款白膜 + 可点穿**：

1. 该 `SidePanel` 改为透明占位（或保留 Frame 仅布局）；  
2. Central 之后增加 `Area::Foreground` + [`right_dock_slot_rect`](../src/ui/layout_util.rs)（本帧布局 `response.rect` + 右缘钉屏）；  
3. 正文抽到 `show_*_contents`，与占位 `show_*_side_panel` 分离。

未出现叠绘问题的 dock 可暂维持单通道 `SidePanel` 直绘。

---

## 六、与 `UI-GUIDELINES.md` 的差异（勿再按旧稿实现）

| 旧规范 | 当前实现 |
|--------|----------|
| 双行：菜单栏 + 标题栏 | **仅 28px 菜单行**；连接信息在 Tab + 底栏（避免顶栏重复） |
| §8.3 📤 = 导出 | 实现为 **上传**（SCP/ZMODEM 入口） |
| 左栏 `bg_window` 铺满 | 圆角 `#13131c` 面板 + 列间 gap |

---

## 七、视觉验收表（改布局后勾选）

- [ ] 窗口四边：主内容距边缘约 8px `surface_body`（右 dock 屏右缘用 [`Theme::spacing_right_dock_screen_inset`](../src/ui/theme.rs)；缝由 [`paint_right_dock_screen_gutter`](../src/ui/chrome.rs) 铺 `surface_body`）
- [ ] 三列之间 6px 缝隙可见（`surface_body`，**非** GPU 黑底）
- [ ] 左栏 / 终端 / 右 dock：**1px `panel_stroke()`** 外框可辨；内部分隔用 `divider_stroke()`（见 [`theme.rs`](../src/ui/theme.rs) Token v2）
- [ ] **四套主题**各走查：面板边框、hint、Tab/按钮不裁切（细则见 [`QA.md`](../tech/QA.md) §主题视觉）
- [ ] 左栏：导入条 + 单块圆角面板，无「条在 panel 外、列表在 panel 内」割裂感
- [ ] 顶栏一行：菜单（文件/视图/工具/帮助）；无重复连接条
- [ ] 底栏 32px，无第二行快捷栏
- [ ] 窄屏 &lt;1200px：右 dock 关闭、侧栏可折叠
- [ ] 命令片段栏：标题/「新建」/Docker·K8s 标签/列表文字清晰，**无白膜、无右缘裁切**
- [ ] 片段栏可正常点击；不存在「看得见但像被挡住」的叠层
