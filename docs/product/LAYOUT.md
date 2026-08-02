# MistTerm 布局

---

## 一、窗口结构（ASCII）— 2026 统一 chrome

全平台同一套窗口 chrome（macOS 仍可用系统菜单栏，但**无常驻底栏**）。

```
┌─ 系统窗口标题（macOS 原生 / Win·Linux 自绘）──────────────┐
├─ [非 macOS] top_chrome 菜单行 ────────────────────────────┤
├─▮ activity_rail（可隐藏；隐藏时留窄恢复条）─┬─ 主工作区 ──┤
│ 连接 / 片段 / 文件                           │ Tab + 终端   │
│ 转发 / 监控 / AI                             │ [可选] 连接列表 │
│ 设置（底）                                   │ [可选] 右单抽屉 │
└──────────────────────────────────────────────┴────────────┘
右下角：status Toast（瞬时 / 需确认；非底栏）
```

---

## 二、egui 注册顺序（禁止随意调整）

| 步骤 | 类型 | ID / 说明 |
|------|------|-----------|
| 1 | `TopBottomPanel::top` | `"top_chrome"`（macOS 通常高度 0） |
| 2 | `SidePanel::left` | `"activity_rail"`；隐藏时改为 `"activity_rail_reveal"`（窄条） |
| 3 | `SidePanel::right` | **至多一个**右抽屉（片段 / 凭证 / 云同步 / SFTP / 监控 / 转发 / AI） |
| 4 | `CentralPanel` | 连接列表（可关）+ Tab + 终端 |
| 5 | `Area` Foreground | 右抽屉正文重绘、Toast、弹窗 |

> 旧版 `bottom_chrome` 状态栏已移除；布局底缘不再预留 `status_bar_height`（token 现为 `0`）。

---

## 三、区域 → 代码映射

| 区域 | 职责 | 主文件 |
|------|------|--------|
| Activity Rail | 导航：连接列表 + 各面板 + 偏好；可完全隐藏 | [`app.rs`](../../src/ui/app.rs) `show_activity_rail` / `show_activity_rail_reveal_strip` / `toggle_activity_rail` |
| Toast | 瞬时提示与需确认提示 | `tick_status_toast` / `show_status_toast` + [`chrome.rs`](../../src/ui/chrome.rs) `paint_status_toast` |
| 顶栏 | 非 macOS 菜单；连接信息在 Tab | [`chrome.rs`](../../src/ui/chrome.rs) `render_top_chrome_panel` |
| 工作区编排 | 注册顺序、三列 | [`workspace.rs`](../../src/ui/workspace.rs) |
| 左连接列表 | 默认收起；rail「连接」展开 | [`sidebar.rs`](../../src/ui/sidebar.rs) |
| 右单抽屉 | `open_right_dock_panel` 互斥 | [`app.rs`](../../src/ui/app.rs) |
| Tab 状态点 | `session_tab_chip` online 圆点 | [`chrome.rs`](../../src/ui/chrome.rs) |

---

## 四、间距与尺寸（theme）

| Token | 值 | 用途 |
|-------|-----|------|
| `activity_rail_width` | 48px | 左侧导航轨（显示时） |
| `activity_rail_collapsed_strip_width` | 8px | 隐藏时左缘恢复条 |
| `size_activity_rail_btn` | 40px | Rail 图标按钮边长 |
| `status_bar_height` | 0 | 已无底栏（兼容旧 clamp API） |
| `spacing_work_area_pad` | 4px | 工作区外圈留白 |
| `spacing_region_gap` | 6px | 列间缝 |
| `menu_bar_height` / `frame_top_chrome` | 32px / 垂直 margin 0 | 顶栏菜单；勿用带垂直 padding 的 `frame_chrome_bar`，否则 egui 会把面板撑出空行 |
| `toast_max_text_width` | 320 | Toast 正文最大宽 |
| `toast_screen_margin` | 16 | Toast 距屏边 |
| `toast_min_width` | 200 | Toast 最小宽 |
| `toast_action_btn_h` | 24 | Toast 操作行高 |

---

## 五、行为约定

1. **连接列表**：启动默认收起；由 Activity Rail「连接」开关；窄屏（&lt;800px）强制收起。文案称「连接列表」，与「活动栏」区分。
2. **Activity Rail**：启动默认显示；**⌘/Ctrl+B**、「视图」菜单（带快捷键标注）、或左缘窄恢复条可完全隐藏/恢复；隐藏时同步收起连接列表。
3. **右面板**：一次只开一个；再点同一入口关闭。关闭 tip 指向活动栏 / 视图菜单（不写「底栏」）。
4. **临时提示（Toast）**：
   - 瞬时：Info/Success **5s**，Warn **7s**，Error **8s**
   - Warn：左边条与正文用 amber；Error/Success：强调色留给左边条，正文主文字色
   - Error/Warn 与需确认 Toast 可点 × 关闭
   - 需确认：不自动消失，主按钮 + ×；不被瞬时 Toast 覆盖
   - SSH 待导入走需确认 Toast（菜单仍可导入）；侧栏横幅 / 顶栏 chip 已移除
   - 启动「就绪」不弹 Toast；诊断 / CJK 字体警告走 Warn
5. **macOS**：系统 NSMenu 保留（含 Cmd+Q 退出、Cmd+B 切换活动栏）；窗口内与 Win/Linux 同为 Rail + 窄恢复条 + 无底栏。快捷键修饰键走平台 `accel*`，禁止写死 Ctrl。
