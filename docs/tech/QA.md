# MistTerm 质量验收

---

## 一、手工冒烟（多平台）

在对应平台执行 `cargo build --release` 后，用 `./target/release/Mist`（或平台等价路径）逐项验证。也可先跑 `scripts/smoke.sh` 做构建与单元测试。

### 1. 会话与 SSH

- 新建会话并连接：超时约 30s 内应成功或给出中文错误（非无限卡住）。
- 断开后：状态与侧栏圆点一致；可选开启「偏好 → 网络断开后自动重连」验证非主动断开时的退避重连（最多 5 次）。

### 2. 终端

- 多行输出、滚动条在内容溢出时出现（约 4px 宽）。
- 非当前 Tab 有输出时切换回来内容应已更新。
- 拖放小文件（小于 10MB）到终端区：应触发与「上传」相同的 SCP 流程；达到或超过 10MB 应弹出 SCP / ZMODEM 选择。

### 3. 片段与快捷键

- 插入片段后 `sessions`/片段统计有写入；关于页（⌘H / Ctrl+H）展示的快捷键表与 `FUNCTIONAL_SPEC §7` 一致。

### 4. Git 同步

- 在已是 Git 仓库的会话目录打开 Git 面板：快进拉取成功；若本地与远程分叉，拉取应提示 §6.4 文案且不强切工作区。

### 5. ZMODEM（可选）

- 远端已装 `lrzsz` 时：`rz` / 工具栏大文件 ZMODEM 路径可完成握手（视网络环境）。

---

## 二、跨平台 UI 验收

发布前在 macOS、Windows、Linux 各执行一轮（建议含 100%/125%/150% 显示缩放，Windows 重点）。

### 图标（图集，非字体符号）

- [ ] 底栏右：片段 / 上传 / 搜索 / 监控 四个图标清晰、可点
- [ ] 底栏左：折叠后「连接」「命令片段」复原 chip 带三角图标
- [ ] 侧栏：收起、新建、搜索框左侧放大镜
- [ ] 顶栏/菜单：主题子菜单选中勾、SSH 待导入告警图标
- [ ] 终端搜索条：上一条/下一条为图标（非 ◀ ▶ 字符）
- [ ] 右 dock：Git / SFTP / 监控 / 凭证 / 云同步 标题旁图标
- [ ] 命令片段面板：× / 次数 / 新建 与底栏工具图标**同尺寸**（约 18pt），Retina 下不糊、不放大一倍
- [ ] 监控标题告警：警告图标 + 文案（非 `!` 字符）
- [ ] SFTP「连接建立中…」、监控「远程采集中…」、Git「操作中…」显示旋转指示（`busy_row`）

### 中文

- [ ] 菜单、侧栏、弹窗中文无方框（含无微软雅黑的 Windows 环境）
- [ ] 启动时底栏无「未加载中文字体」警告（正常构建应已嵌入 Noto）

### 快捷键文案

- [ ] macOS 菜单显示 ⌘ 修饰键；Windows/Linux 显示 Ctrl
- [ ] 命令历史覆盖层显示 `Ctrl+R`（各平台一致）

### 系统 shell / 文案

- [ ] 「打开文档文件夹」：macOS 提示含 Finder；Windows 含资源管理器；Linux 为文件管理器
- [ ] 帮助弹窗底部说明中的菜单路径与当前平台一致

### 主题视觉（四套：暗夜 / 晨曦 / 海洋 / 森林）

每套主题各截一张：**左栏**、**终端**、**右 dock**（监控 + 命令片段）、**底栏**、**弹窗**（偏好或新建会话）。

- [ ] 侧栏 / 终端 / 右 dock 外框 **1px 面板线**肉眼可辨（`panel_stroke()`，非发糊灰雾）
- [ ] 顶栏 / 底栏 / 面板内 **分隔线**弱于外框、但仍可见（`divider_stroke()`）
- [ ] 输入框 hint、统计 caption **可读**（次要字对比 ≥ 约 4.5:1 观感）
- [ ] Tab 标签、片段标题、底栏按钮 **无裁切**、换行正常
- [ ] 右 dock 与屏右缘 **无黑缝**（`paint_right_dock_screen_gutter` 铺 `surface_body`）
- [ ] **晨曦**浅色：灰边不与 `#f5f5f5` 融在一起；**Retina / 125% 缩放**下小字不发虚
- [ ] 切换主题后整窗 **一次重绘**，图标图集与 `pixels_per_point` 一致

### 控件样式（`chrome.rs` 统一入口）

| 类型 | 使用 | 勿用 |
|------|------|------|
| 表单标签 | `form_field_label` / `rich_form_label` | 裸 `ui.label` / `.small()` |
| 单行输入 | `form_singleline_field` | 裸 `TextEdit::singleline` |
| 多行输入 | `form_multiline_field` | 裸 `TextEdit::multiline` |
| 搜索行 | `panel_search_row` / `search_field` | 裸搜索框 |
| 菜单项 | `popup_menu_button` / `menu_item_label_accel` | 裸 `ui.button` |
| 数字 | `form_drag_value_field` + `DragValue` | 裸 `DragValue` |
| 筛选 | `filter_chip_row` / `filter_chip_row_with_sort` | 裸 `Button` |
| 排序 | `panel_sort_chip` | 标题栏大灰钮 |
| 标题新建 | `panel_header_new_button` | `panel_toolbar_primary` 宽条 |
| 面板操作 | `panel_action_button` / `panel_action_primary_button` | `ui.button` |
| 弹窗底栏 | `modal_primary_button` / `modal_secondary_button` | 裸 `Button` |
| 弹窗标题 | `modal_header` | 系统标题栏 |

**已对齐**：连接侧栏、命令片段、凭证、SFTP、Git、云同步、片段库、审计/会话日志、偏好设置、变量弹窗、顶栏/右键/Tab 菜单、命令历史浮层、SSH 导入分页、快速片段选择器。

**有意保留裸控件**：终端仿真区 `TextEdit::multiline`（只读缓冲）、命令历史列表行 `row_button`（选中高亮列表项）。

### 功能抽测

- [ ] 终端 Delete/Backspace（mac 与 Win 行为符合平台习惯）
- [ ] 打开文档目录 / 克隆仓库等系统 shell 调用
