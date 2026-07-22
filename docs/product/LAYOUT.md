# MistTerm 布局

---

## 一、窗口结构（ASCII）— 2026 统一 chrome

全平台同一套窗口 chrome（macOS 仍可用系统菜单栏，但**无常驻底栏**）。

```
┌─ 系统窗口标题（macOS 原生 / Win·Linux 自绘）──────────────┐
├─ [非 macOS] top_chrome 菜单行 ────────────────────────────┤
├─▮ activity_rail 48px ─┬─ 主工作区 ───────────────────────┤
│ 连接 / 片段 / 文件     │ Tab + 终端（全高）                 │
│ 转发 / 监控 / AI       │ [可选] 连接抽屉（rail 打开时）     │
│ 设置（底）             │ [可选] 右侧单抽屉（互斥）          │
└────────────────────────┴──────────────────────────────────┘
右下角：status Toast（临时提示，非底栏）
```

---

## 二、egui 注册顺序（禁止随意调整）

| 步骤 | 类型 | ID / 说明 |
|------|------|-----------|
| 1 | `TopBottomPanel::top` | `"top_chrome"`（macOS 通常高度 0） |
| 2 | `SidePanel::left` | `"activity_rail"` 固定 ~48px |
| 3 | `SidePanel::right` | **至多一个**右抽屉（片段 / 凭证 / 云同步 / SFTP / 监控 / 转发 / AI） |
| 4 | `CentralPanel` | 连接抽屉（可关）+ Tab + 终端 |
| 5 | `Area` Foreground | 右抽屉正文重绘、Toast、弹窗 |

> 旧版 `bottom_chrome` 状态栏已移除；布局底缘不再预留 `status_bar_height`（token 现为 `0`）。

---

## 三、区域 → 代码映射

| 区域 | 职责 | 主文件 |
|------|------|--------|
| Activity Rail | 导航：连接抽屉 + 各面板 + 偏好 | [`src/ui/app.rs`](../src/ui/app.rs) `show_activity_rail` |
| Toast | 临时 `status_message` | `tick_status_toast` / `show_status_toast` + [`chrome.rs`](../src/ui/chrome.rs) `paint_status_toast` |
| 顶栏 | 非 macOS 菜单；连接信息在 Tab | [`chrome.rs`](../src/ui/chrome.rs) `render_top_chrome_panel` |
| 工作区编排 | 注册顺序、三列 | [`workspace.rs`](../src/ui/workspace.rs) |
| 左连接抽屉 | 默认收起；rail「连接」展开 | [`sidebar.rs`](../src/ui/sidebar.rs) |
| 右单抽屉 | `open_right_dock_panel` 互斥 | [`app.rs`](../src/ui/app.rs) |
| Tab 状态点 | `session_tab_chip` online 圆点 | [`chrome.rs`](../src/ui/chrome.rs) |

---

## 四、间距与尺寸（theme）

| Token | 值 | 用途 |
|-------|-----|------|
| `activity_rail_width` | 48px | 左侧导航轨 |
| `status_bar_height` | 0 | 已无底栏（兼容旧 clamp API） |
| `spacing_work_area_pad` | 8px | 工作区外圈留白 |
| `spacing_region_gap` | 6px | 列间缝 |

---

## 五、行为约定

1. **连接列表**：启动默认收起；由 Activity Rail「连接」开关；窄屏（&lt;800px）强制收起。
2. **右面板**：一次只开一个；再点同一入口关闭。
3. **临时提示**：写入 `status_message` → 右下 Toast，约 4 秒消失。
4. **macOS**：系统 NSMenu 保留；窗口内与 Win/Linux 同为 Rail + 无底栏。
