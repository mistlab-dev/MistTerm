# MistTerm — 终端应用设计文档

> 版本：v1.0  
> 原型地址：`docs/product/proto-terminal-embedded.html`  
> 技术栈建议：Electron / Tauri + React + xterm.js  
> 更新日期：2026-04-30

---

## 一、整体结构

窗口采用**三层布局**，从上到下依次为：

```
┌────────────────────────────────────────────────────────────┐
│  标题栏 (Title Bar) — 10px padding                        │
├────────────────────────────────────────────────────────────┤
│  ┌─────────┐  ┌────────────────────┐  ┌───────────────┐   │
│  │左侧面板  │  │    终端区域        │  │  右侧面板     │   │
│  │200px    │  │   flex:1          │  │  260px        │   │
│  │         │  │  Tabs + 输出+输入  │  │  命令片段     │   │
│  │可折叠   │  │                   │  │  可折叠       │   │
│  └─────────┘  └────────────────────┘  └───────────────┘   │
│  gap:6px, padding:8px, 圆角面板:6px                        │
├────────────────────────────────────────────────────────────┤
│  状态栏 (Status Bar) — 4px padding                        │
└────────────────────────────────────────────────────────────┘
```

### 布局参数明细

| 区域 | 尺寸 | 值 |
|---|---|---|
| 窗口 | 最大宽度 | 1440px |
| 窗口 | 高度 | 820px |
| 窗口 | 圆角 | 10px |
| 窗口 | 内边距（body） | 16px |
| 标题栏 | padding | 10px 16px |
| 主区域 | padding | 8px |
| 面板间距 | gap | 6px |
| 状态栏 | padding | 4px 14px |

---

## 二、配色方案

### 2.1 基础色板

| 用途 | 色值 | CSS |
|---|---|---|
| 页面背景 (body) | `#0d0d14` | `background: #0d0d14` |
| 窗口/面板底色 | `#13131c` | `background: #13131c` |
| 终端区域底色 | `#0a0a12` | `background: #0a0a12` |
| 激活 Tab 背景 | `#0a0a12` | `background: #0a0a12` |
| 面板半透明底 | `rgba(255,255,255,0.04)` | 面板/状态栏背景变体 |
| Tab 栏/输入栏底 | `rgba(255,255,255,0.02)` | 区分层级 |

### 2.2 边框

| 用途 | 色值 |
|---|---|
| 窗口边框 | `rgba(255,255,255,0.06)` |
| 面板/终端边框 | `rgba(255,255,255,0.04)` |
| 标题栏底部分割 | `rgba(255,255,255,0.04)` |
| 状态栏顶部分割 | `rgba(255,255,255,0.03)` |
| Tab 分割线 | `rgba(255,255,255,0.03)` ~ `0.04` |
| 输入框边框 | `rgba(255,255,255,0.03)` |

### 2.3 文字色彩

| 用途 | 色值 | 透明度 | 字号 |
|---|---|---|---|
| 终端命令文字 | `rgba(255,255,255,0.9)` | 90% | 13px |
| 终端输出文字 | `rgba(255,255,255,0.4)` | 40% | 13px |
| 面板标题/名称 | `rgba(255,255,255,0.5)` | 50% | 12px |
| 面板标题 hover | `rgba(255,255,255,0.7)` | 70% | — |
| 面板标题激活 | `rgba(255,255,255,0.75)` | 75% | — |
| 弱化文字（元信息）| `rgba(255,255,255,0.12)` | 12% | 10px |
| 标题栏文字 | `rgba(255,255,255,0.3)` | 30% | 13px |
| 标题栏右侧 | `rgba(255,255,255,0.2)` | 20% | 11px |
| 面板标题（大写）| `rgba(255,255,255,0.2)` | 20% | 10px |
| 搜索框文字 | `rgba(255,255,255,0.5)` | 50% | 11px |
| 搜索框 placeholder | `rgba(255,255,255,0.12)` | 12% | — |
| 分类标签文字 | `rgba(255,255,255,0.18)` | 18% | 10px |
| 分类标签激活 | `rgba(255,255,255,0.18)` 改为 `rgba(102,126,234,0.5)` | — | — |
| 状态栏文字 | `rgba(255,255,255,0.12)` | 12% | 11px |
| 统计数字 | `rgba(255,255,255,0.25)` | 25% | 10px |
| 统计标签 | `rgba(255,255,255,0.1)` | 10% | 10px |
| 片段命令原文 | `rgba(255,255,255,0.12)` | 12% | 10px |
| 输入框文字 | `rgba(255,255,255,0.9)` | 90% | 13px |

### 2.4 功能色

| 用途 | 色值 |
|---|---|
| **主色调** (路径/选中态/标签激活) | `#667eea` |
| 主色调减弱态 (tag个人/mini-btn激活) | `rgba(102,126,234,0.35 - 0.5)` |
| **成功/连接** (prompt/绿点/↑增长) | `#4CAF50` |
| 成功增强态 (连接时长) | `rgba(76,175,80,0.25)` |
| 增长指标 | `rgba(76,175,80,0.3)` |
| **团队标签** | `rgba(76,175,80,0.05)` bg + `rgba(76,175,80,0.35)` text |
| **个人标签** | `rgba(102,126,234,0.05)` bg + `rgba(102,126,234,0.35)` text |
| **模板标签** | `rgba(255,152,0,0.05)` bg + `rgba(255,152,0,0.35)` text |
| 红绿灯 - 关闭 | `#ff5f56` |
| 红绿灯 - 最小化 | `#ffbd2e` |
| 红绿灯 - 最大化 | `#27c93f` |
| 窗口阴影 | `0 20px 60px rgba(0,0,0,0.6)` |
| 搜索框 focus 边框 | `rgba(102,126,234,0.2)` |

### 2.5 状态栏颜色

| 元素 | 色值 |
|---|---|
| 复原按钮 (▸) | `rgba(102,126,234,0.25)` |
| 复原按钮 hover | `rgba(102,126,234,0.45)` + `rgba(102,126,234,0.04)` bg |
| 工具按钮 (📋📤🔍📊) | `rgba(255,255,255,0.08)` |
| 工具按钮 hover | `rgba(255,255,255,0.25)` |
| 分隔符 | `rgba(255,255,255,0.04)` |
| 统计数字 | `rgba(255,255,255,0.1)` (注：较小字号 10px) |

---

## 三、字体与排版

### 3.1 字体栈

| 用途 | 字体 |
|---|---|
| 界面文字 | `'Inter', sans-serif` |
| 终端/片段命令 | `'JetBrains Mono', monospace` |
| 按钮图标 | `system-ui`（保证 emoji 兼容）|

### 3.2 字号对照表

| 用途 | 字号 | 字重 | 其他 |
|---|---|---|---|
| 应用名称 (标题栏) | 13px | — | 居中 |
| 标题栏右侧信息 | 11px | — | — |
| 面板标题 (连接/命令片段) | 10px | **600 (bold)** | 大写，字母间距 0.5px |
| 面板 `−` 按钮 | 14px | — | 行高 1 |
| 连接名称 | 12px | — | — |
| 连接时长 | 10px | — | — |
| 终端输出/输入 | 13px | — | 行高 1.7 |
| 片段标题 | 12px | — | — |
| 片段命令原文 | 10px | — | 单行截断 |
| 片段统计 | 10px | — | — |
| 标签文字 | 9px | **500 (medium)** | — |
| 分类标签 | 10px | — | — |
| 搜索框 | 11px | — | — |
| 状态栏 | 11px | — | — |
| 状态栏统计 | 10px | — | — |
| 复原按钮 | 10px | — | — |
| 工具按钮 (emoji) | 12px | — | 行高 1 |

### 3.3 排版参数

| 参数 | 值 |
|---|---|
| 终端输出行高 | 1.7 |
| 字母间距 (标题) | 0.5px |
| 连接条目 padding | 8px 10px |
| 片段卡片 padding | 7px 8px |
| 面板 body padding | 0 4px 4px |
| 滚动条宽度 | 4px |
| 滚动条颜色 | `rgba(255,255,255,0.06)` |

---

## 四、圆角与间距

### 4.1 圆角

| 元素 | 圆角 |
|---|---|
| 窗口 | 10px |
| 面板 | 6px |
| 终端区域 | 6px |
| 状态栏按钮 | 3px |
| 标签 | 3px |
| 片段卡片 | 4px |
| 连接条目 | 4px |
| 搜索框 | 4px |
| 分类标签 | 3px |
| 复原按钮 | 3px |
| 红绿灯圆点 | 50% (圆形) |

### 4.2 间距

| 间距 | 值 |
|---|---|
| 面板/面板 - 面板间距 | 6px |
| 面板内 - 标题 padding | 9px 10px |
| 面板内 - 内容 padding | 0 4px 4px |
| 面板内 - 搜索框 padding | 4px 8px 6px |
| 搜索框 input padding | 5px 8px |
| 连接条目间距 | 1px |
| Tab padding | 7px 14px |
| Tab 圆点/文字间距 | 6px |
| 终端滚动区 padding | 10px 16px |
| 输入行与输出间距 | 0（在同一个滚动区内） |
| 状态栏左侧间距 | gap: 8px |
| 状态栏右侧间距 | gap: 4px |
| 工具按钮间距 | 3px + 细空格 ` ` |

---

## 五、各面板详细设计

### 5.1 标题栏

```
[●●●]           MistTerm                SSH · 2h 34m
```

- 高度：10px padding top/bottom
- 三个红绿灯点：间距 7px，大小 11px
- 居中的应用名：13px，30% 透明度
- 右侧信息：11px，20% 透明度
- 底部边框：`1px solid rgba(255,255,255,0.04)`

### 5.2 左侧面板 — 连接管理

**展开状态：**

```
┌────────────────────────┐
│ 连接                 − │  ← 10px bold uppercase
├────────────────────────┤
│ 🔍 搜索连接…           │  ← 11px input
│ ─────────────────────  │
│ 全部  │ 在线  │ 离线    │  ← 10px 等分三列
│ ─────────────────────  │
│ 🖥 生产服务器  2h 34m   │  ← 12px name + 10px meta
│ 🖥 测试服务器   45m     │
│ 🖥 数据库      离线     │
│ 🖥 预发布      10m      │
└────────────────────────┘
```

**折叠状态：**
面板消失，状态栏左侧出现 `▸ 连接 · 3`

**交互细节：**
- 分类标签：hover 显示 30% 透明度，点击激活为紫色
- 连接条目：hover 显示浅色底 `rgba(255,255,255,0.03)`，激活选中显示紫色底 `rgba(102,126,234,0.05)`
- 连接图标：11px，35% 透明度
- 连接时长绿色：`rgba(76,175,80,0.25)`

### 5.3 终端区域

**结构：**

```
┌────────────────────────────────────┐
│ ● ubuntu@prod-server-01  ×  │ ● root@test-server  ×  │ + │  ← Tab 栏
├────────────────────────────────────┤
│ Last login: Wed Apr 29 21:20:34    │
│ • Linux prod-server-01 · 5.15.0    │
│                                    │
│ ➜ ~ systemctl status nginx         │  ← 绿色prompt + 紫色路径 + 白色命令
│ ● nginx.service — A high ...       │  ← 输出灰色
│    Loaded: loaded (/lib/systemd…   │
│    Active: active (running) since  │
│    Tasks: 2 · Memory: 8.2M         │
│                                    │
│ ➜ ~ kubectl █                      │  ← 当前输入行 (contenteditable)
└────────────────────────────────────┘
```

**Tab 栏细节：**
- 背景：`rgba(255,255,255,0.02)`
- Tab 名称前绿色圆点：5px，`#4CAF50`
- 激活 Tab：文字 80% 亮度，背景 `#0a0a12`（与终端输出区一致）
- 非激活 Tab：文字 25% 亮度
- 关闭按钮 `×`：默认隐藏 (`opacity: 0`)，hover Tab 时出现 30% 透明度
- 新建按钮 `+`：25% 透明度

**终端输出区细节：**
- 整体为一个 `div.scroll`，`contenteditable="true"` 表示可编辑
- 输出与输入在同一个滚动容器内，没有分割
- 语法标注：
  - `.prompt` (`➜`) → `color: #4CAF50`
  - `.path` (`~`) → `color: #667eea`
  - `.cmd` (`systemctl status nginx`) → `color: rgba(255,255,255,0.9)`
  - `.out` (输出行) → `color: rgba(255,255,255,0.4)`
- 无独立的输入框行
- 无命令建议条
- 无快捷工具栏

### 5.4 右侧面板 — 命令片段

**展开状态：**

```
┌──────────────────────────────┐
│ 命令片段                   − │
├──────────────────────────────┤
│ 🔍 搜索片段…                 │
│ ──────────────────────────── │
│ 常用 │ Docker │ K8s │ 全部   │
│ ──────────────────────────── │
│ 查看 Pod 日志       [团队]   │  ← 标题12px + 标签9px
│ kubectl logs -f ...          │  ← 命令原文10px
│ 320次 · 95%成功 · 1.2s      │  ← 统计10px
│ ──────────────────────────── │
│ 重启 Nginx          [团队]   │
│ systemctl restart nginx...   │
│ 500次 · 98%成功 · 0.8s      │
│ ──────────────────────────── │
│ 访问日志           [个人]    │
│ tail -f /var/log/nginx...   │
│ 156次 · 100%成功 · 0.3s     │
│ ──────────────────────────── │
│ Docker 健康检查     [模板]   │
│ docker ps --format ...       │
│ 1,200次 · 96%成功 · 0.5s    │
└──────────────────────────────┘
```

**折叠状态：**
面板消失，状态栏右侧出现 `▸ 命令片段 · 5`

**片段卡片细节：**
- 标题（`title`）：12px，50%透明度，hover 时 70%
- 标签（`tag`）：9px，**500 medium**，3px 圆角
  - `.team` → 绿色 bg/text
  - `.personal` → 紫色 bg/text
  - `.market` → 橙色 bg/text
- 命令原文（`cmd-text`）：10px JetBrains Mono，12%透明度，单行截断 `text-overflow: ellipsis`
- 统计行（`stats`）：10px
  - 数字（`.n`）：25%透明度
  - 标签（`.l`）：10%透明度
- 卡片 hover：`rgba(255,255,255,0.03)` 背景

---

## 六、状态栏详细设计

### 6.1 布局

| 侧 | 正常态 | 左面板收起时 | 右面板收起时 | 两侧都收起时 |
|---|---|---|---|---|
| **左侧** | `⚡ ubuntu@prod-server-01` | `▸ 连接 · 3  ⚡ ubuntu...` | `⚡ ubuntu@prod-server-01` | `▸ 连接 · 3  ⚡ ubuntu...` |
| **右侧** | 📋 📤 🔍 📊 · 1,234次 ↑8% | 📋 📤 🔍 📊 · 1,234次 ↑8% | `▸ 命令片段 · 5` 📋 📤 🔍 📊 · 1,234次 ↑8% | `▸ 命令片段 · 5` 📋 📤 🔍 📊 · 1,234次 ↑8% |

### 6.2 元素规格

| 元素 | 字号 | 颜色 | 间距 |
|---|---|---|---|
| 复原按钮 `▸ 连接 · N` | 10px | `rgba(102,126,234,0.25)` | padding: 1px 5px |
| 复原按钮 hover | — | `rgba(102,126,234,0.45)` + bg: `rgba(102,126,234,0.04)` | — |
| 连接信息 `⚡ ubuntu@…` | 11px | `rgba(255,255,255,0.12)` | — |
| 工具按钮 📋📤🔍📊 | 12px | `rgba(255,255,255,0.08)` | padding: 0 3px |
| 工具按钮 hover | — | `rgba(255,255,255,0.25)` | — |
| 统计数字 | 10px | `rgba(255,255,255,0.1)` | — |
| 增长指标 ↑8% | 11px | `rgba(76,175,80,0.3)` | — |

### 6.3 交互逻辑

```javascript
// 折叠面板
function toggleLeft() {
    sideLeft.classList.toggle('collapsed');
    restoreLeft.style.display = isCollapsed ? 'inline' : 'none';
}

// 展开面板（从状态栏）
function expandLeft() {
    sideLeft.classList.remove('collapsed');
    restoreLeft.style.display = 'none';
}
```

---

## 七、过渡与动画

| 场景 | 属性 | 时长 | 缓动函数 |
|---|---|---|---|
| 面板折叠/展开 | width, padding, opacity | 0.2s | ease |
| 通用 hover 效果 | 背景色/文字色 | 0.1s | ease |
| Tab hover 关闭按钮 | opacity | 0.1s | ease |
| 搜索框 focus | border-color, background | 0.1s | ease |
| 窗口弹入 | box-shadow | — | 无动画 |

面板折叠时使用 `!important` 覆盖保证折叠态不被 flex 布局撑开：
```css
.side.collapsed {
    width: 0 !important;
    min-width: 0 !important;
    padding: 0 !important;
    margin: 0 !important;
    border: none !important;
    overflow: hidden !important;
    opacity: 0;
}
```

---

## 八、交互逻辑对照表

| # | 操作 | 触发元素 | 效果 |
|---|---|---|---|
| 1 | 点击标题 `−` | 面板 header.add | 对应面板折叠（width→0，隐藏），状态栏出现 ▸ 按钮 |
| 2 | 点击 `▸ 名称 · N` | 状态栏 restore-btn | 对应面板展开（还原宽度），▸ 按钮消失 |
| 3 | 点击连接条目 | .sess-item | 切换激活态，连接到对应服务器 |
| 4 | 点击分类标签 | .frag-cat | 激活选中标签，按分类过滤列表 |
| 5 | 点击片段卡片 | .frag-card | 将命令填入终端输入行末尾 |
| 6 | 搜索框输入 | input | 实时模糊过滤连接/片段列表 |
| 7 | Hover 面板条目 | .sess-item / .frag-card | 显示浅色背景 `rgba(255,255,255,0.03)` |
| 8 | Hover Tab 关闭按钮 | .term-tab .close | 关闭按钮 opacity: 0 → 0.3 |
| 9 | 点击 Tab `+` | .term-tab-new | 新建终端 Tab |
| 10 | 点击 Tab | .term-tab | 切换到对应终端会话 |
| 11 | Hover 工具按钮 | .status-btn | 颜色 `rgba(255,255,255,0.08)` → `0.25` |
| 12 | 点击工具按钮 | 📋📤🔍📊 | 复制/上传/搜索/统计（具体功能待定） |
| 13 | Hover 复原按钮 | .restore-btn | 颜色 `rgba(102,126,234,0.25)` → `0.45` + 紫色背景 |

---

## 九、技术实现建议

### 9.1 技术路线

*（已确认：纯 Rust 实现，无需 Web 技术栈）*

| 模块 | 推荐 Rust 方案 |
|---|---|
| GUI 框架 | **egui** / **Druid** / **GPUI**（根据团队技术积累选择） |
| 终端引擎 | **Portable-pty** (跨平台 pty fork/exec) 或自研 pty 封装 |
| 终端渲染 | 自研 terminal widget 或基于 alacritty 的 vte 解析 + 自定义渲染 |
| 状态管理 | Rust 原生 struct + enum 模式 |
| 主题/样式 | CSS-in-Rust 风格或硬编码颜色常量 |
| SSH 连接 | **ssh2** crate 或直接封装 libssh |

### 9.2 核心数据结构（Rust）

```rust
// 连接
#[derive(Debug, Clone)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub status: ConnectionStatus,
    pub connected_at: Option<i64>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Online,
    Offline,
}

// 命令片段
#[derive(Debug, Clone)]
pub struct Fragment {
    pub id: String,
    pub title: String,
    pub command: String,
    pub tag: FragmentTag,
    pub categories: Vec<String>,
    pub usage_count: u32,
    pub success_rate: f32,    // 0.0 - 100.0
    pub avg_time_ms: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FragmentTag {
    Personal,
    Team,
    Market,
}

// 终端 Tab
#[derive(Debug, Clone)]
pub struct TerminalTab {
    pub id: String,
    pub title: String,
    pub session_id: String,
    pub active: bool,
    pub connected_at: i64,
}

// 全局状态
pub struct AppState {
    pub panels: PanelState,
    pub left_category: LeftCategory,
    pub right_category: RightCategory,
    pub left_search: String,
    pub right_search: String,
    pub tabs: Vec<TerminalTab>,
    pub active_tab_id: String,
    pub connections: Vec<Connection>,
    pub fragments: Vec<Fragment>,
}

impl Default for AppState {
    fn default() -> Self { /* ... */ }
}
```

### 9.3 状态管理

```
App
├── panels: PanelState
│   ├── left_collapsed: bool
│   └── right_collapsed: bool
├── connections: Vec<Connection>    // 连接列表
├── fragments: Vec<Fragment>        // 片段列表
├── filter: FilterState
│   ├── left_category: LeftCategory
│   ├── right_category: RightCategory
│   ├── left_query: String
│   └── right_query: String
├── tabs: TabManager
│   ├── tabs: Vec<TerminalTab>
│   └── active_id: String
└── stats: StatsState               // 状态栏统计
    ├── total_commands: u64
    └── weekly_growth: f32
```

### 9.4 组件树建议

```
App
├── TitleBar                    // 标题栏（纯静态）
├── MainLayout (flex row)       // 主区域
│   ├── SidePanel (左)          // 连接面板
│   │   ├── PanelHeader         // "连接" + "−" 按钮
│   │   ├── SearchBox           // 搜索输入框
│   │   ├── CategoryTabs        // 全部/在线/离线
│   │   └── ConnectionList      // 连接条目列表
│   ├── TerminalPanel           // 终端
│   │   ├── TabBar              // Tab 栏
│   │   └── TerminalView        // 终端渲染区 (contenteditable)
│   │       ├── OutputBlock     // 历史输出
│   │       └── InputLine       // 当前输入行
│   └── SidePanel (右)          // 片段面板
│       ├── PanelHeader         // "命令片段" + "−" 按钮
│       ├── SearchBox           // 搜索输入框
│       ├── CategoryTabs        // 常用/Docker/K8s/全部
│       └── FragmentList        // 片段卡片列表
└── StatusBar                   // 状态栏
    ├── LeftGroup               // 复原按钮 + 连接信息
    └── RightGroup              // 复原按钮 + 工具按钮 + 统计
```

---

## 十、组件树结构

### 10.1 完整组件嵌套

```
App (主窗口)
│
├── TitleBar (标题栏)
│   ├── [红绿灯点]           // 窗口控制 (dots/close/min/max)
│   ├── [应用名] "MistTerm"
│   └── [右侧信息] "SSH · 2h 34m"
│
├── MainPanel (主区域 — flex row, padding:8px, gap:6px)
│   │
│   ├── SidePanel (左 — 连接面板, width:200px)
│   │   │
│   │   ├── PanelHeader
│   │   │   ├── [标题] "连接"
│   │   │   └── [折叠按钮] "−"
│   │   │
│   │   ├── SearchBox "🔍 搜索连接…"
│   │   │
│   │   ├── CategoryTabs
│   │   │   ├── [标签] "全部"
│   │   │   ├── [标签] "在线"
│   │   │   └── [标签] "离线"
│   │   │
│   │   └── ScrollList "连接列表"
│   │       ├── ConnectionItem (激活态)
│   │       │   ├── [图标] 🖥
│   │       │   ├── [名称] "生产服务器"
│   │       │   └── [时长] "2h 34m"
│   │       ├── ConnectionItem
│   │       ├── ConnectionItem
│   │       └── ConnectionItem
│   │
│   ├── TerminalPanel (终端 — flex:1)
│   │   │
│   │   ├── TabBar
│   │   │   ├── Tab (激活态)
│   │   │   │   ├── [圆点] ● (连接状态)
│   │   │   │   ├── [名称] "ubuntu@prod-server-01"
│   │   │   │   └── [关闭] × (hover 可见)
│   │   │   ├── Tab (非激活)
│   │   │   └── [新建] +
│   │   │
│   │   └── TerminalView (contenteditable 滚动区)
│   │       ├── OutputLine "output text"
│   │       │   ├── [prompt] ➜ 绿色
│   │       │   ├── [path] ~ 紫色
│   │       │   ├── [cmd] systemctl status nginx 白色
│   │       │   └── [output] 灰色
│   │       ├── OutputLine
│   │       └── InputLine "当前输入行"
│   │
│   └── SidePanel (右 — 片段面板, width:260px)
│       │
│       ├── PanelHeader
│       │   ├── [标题] "命令片段"
│       │   └── [折叠按钮] "−"
│       │
│       ├── SearchBox "🔍 搜索片段…"
│       │
│       ├── CategoryTabs
│       │   ├── [标签] "常用"
│       │   ├── [标签] "Docker"
│       │   ├── [标签] "K8s"
│       │   └── [标签] "全部"
│       │
│       └── FragmentList
│           ├── FragmentCard
│           │   ├── [标题] "查看 Pod 日志"
│           │   ├── [Tag] "团队" (绿色)
│           │   ├── [命令原文] "kubectl logs -f..."
│           │   └── [统计] "320次 · 95% · 1.2s"
│           ├── FragmentCard
│           ├── FragmentCard
│           ├── FragmentCard
│           └── FragmentCard
│
└── StatusBar (状态栏 — 4px padding)
    ├── LeftGroup
    │   ├── [复原按钮] "▸ 连接 · 3" (面板收起时显示)
    │   └── [连接信息] "⚡ ubuntu@prod-server-01"
    └── RightGroup
        ├── [复原按钮] "▸ 命令片段 · 5" (面板收起时显示)
        ├── [工具] 📋 📤 🔍 📊
        ├── [分隔] "·"
        ├── [统计] "1,234次"
        └── [增长] "↑8%"
```

---

## 十一、后续可扩展方向

1. **多 Tab 拖拽排序** — 支持终端 Tab 拖拽重排和独立拖出窗口
2. **片段收藏/评分/自定义标签** — 用户可增删改分类和标签
3. **命令执行统计面板** — 点击 📊 在终端区域展开半屏统计视图
4. **连接分组/文件夹** — 左侧面板支持嵌套分组
5. **暗色/亮色主题** — 抽离所有颜色为 CSS 变量，一键切换
6. **片段导入/导出** — JSON/YAML 格式批量导入导出
7. **SSH 连接配置保存** — 连接配置持久化，支持密钥/AWS SSM
8. **命令输出搜索高亮** — 🔍 激活后终端内搜索关键字并高亮
9. **命令历史** — 保留历史命令记录，支持 ↑↓ 回看
10. **多语言终端** — 支持 zsh/bash/PowerShell 语法高亮规则