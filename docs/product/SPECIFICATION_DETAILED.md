# MistTerm — 设计标注文档 (研发版)

> 版本：v1.0 | 技术栈：egui (Rust) | 设计稿：`proto-terminal-embedded.html`
> 研发直接按此文档实现，有歧义处以本标注为准

---

## 0. 通用规则

### 0.1 颜色常量

```rust
// === 背景色 ===
const BG_BODY: Color32 =          Color32::from_rgb(13, 13, 20);      // #0d0d14 — 窗口外背景
const BG_WINDOW: Color32 =       Color32::from_rgb(19, 19, 28);       // #13131c — 面板/窗口底色
const BG_TERMINAL: Color32 =     Color32::from_rgb(10, 10, 18);       // #0a0a12 — 终端区域/激活 Tab
const BG_TAB_BAR: Color32 =      Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.02)
    5, 5, 5, 5);
const BG_HOVER: Color32 =        Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.03)
    8, 8, 8, 8);
const BG_SELECTED: Color32 =     Color32::from_rgba_premultiplied(    // rgba(102,126,234,0.05)
    5, 6, 12, 13);

// === 边框 ===
const BORDER_WINDOW: Color32 =   Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.06)
    15, 15, 15, 15);
const BORDER_PANEL: Color32 =    Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.04)
    10, 10, 10, 10);
const BORDER_DIVIDER: Color32 =  Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.03)
    8, 8, 8, 8);

// === 文字 ===
const TXT_HIGH: Color32 =        Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.9)
    229, 229, 229, 230);
const TXT_MEDIUM: Color32 =      Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.5)
    128, 128, 128, 128);
const TXT_LOW: Color32 =         Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.25)
    64, 64, 64, 64);
const TXT_DIM: Color32 =         Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.12)
    31, 31, 31, 31);
const TXT_MUTED: Color32 =       Color32::from_rgba_premultiplied(    // rgba(255,255,255,0.08)
    20, 20, 20, 20);

// === 主色调 ===
const ACCENT: Color32 =          Color32::from_rgb(102, 126, 234);    // #667eea
const ACCENT_DIM: Color32 =      Color32::from_rgba_premultiplied(    // rgba(102,126,234,0.35)
    36, 44, 82, 89);
const ACCENT_BG: Color32 =       Color32::from_rgba_premultiplied(    // rgba(102,126,234,0.05)
    5, 6, 12, 13);

// === 状态色 ===
const GREEN: Color32 =           Color32::from_rgb(76, 175, 80);      // #4CAF50 — 成功/连接
const GREEN_DIM: Color32 =       Color32::from_rgba_premultiplied(    // rgba(76,175,80,0.25)
    19, 44, 20, 64);
```

### 0.2 字号与字体映射

```rust
/// 界面字体
const FONT_INTER: &str = "Inter";
/// 等宽字体（终端输出/命令）
const FONT_MONO: &str = "JetBrains Mono";

/// 按用途获取字号
enum TextSize {
    TitleBar,        // 13px
    TitleBarInfo,    // 11px
    PanelTitle,      // 10px (uppercase, letter_spacing: 0.5)
    ConnectionName,  // 12px
    ConnectionMeta,  // 10px
    Terminal,        // 13px
    FragmentTitle,   // 12px
    FragmentCmd,     // 10px
    FragmentStats,   // 10px
    TabLabel,        // 11px
    SearchInput,     // 11px
    StatusBar,       // 11px
    StatusBarStats,  // 10px
    RestoreBtn,      // 10px
    ToolBtn,         // 12px
    Tag,             // 9px (medium 500)
    CategoryLabel,   // 10px
}
```

---

## 1. 窗口布局

### 1.1 整体尺寸

| 属性 | 值 | 说明 |
|---|---|---|
| 窗口最大宽度 | 1440px | 可缩放 |
| 窗口高度 | 820px | 初始高度 |
| 窗口圆角 | 10px | 仅 macOS，Windows/Linux 全屏无圆角 |
| 窗口内边距 (body) | 16px | 窗口最外层 padding |
| 窗口阴影 | 0 20px 60px rgba(0,0,0,0.6) | macOS 窗口阴影 |

### 1.2 三层结构

```
┌────────────────────────────────────────────────────────────┐
│  标题栏 (TitleBar) — 36px 高                               │
├────────────────────────────────────────────────────────────┤
│  ┌─────────┐    ┌──────────────────────┐  ┌────────────┐   │
│  │ 左侧面板 │    │    终端区域           │  │ 右侧面板    │   │
│  │ 200px    │    │    flex: 1           │  │ 260px      │   │
│  │ 可折叠   │    │                      │  │ 可折叠      │   │
│  └─────────┘    └──────────────────────┘  └────────────┘   │
│  gap: 6px, 面板圆角: 6px, padding: 8px                      │
├────────────────────────────────────────────────────────────┤
│  状态栏 (StatusBar) — 28px 高                               │
└────────────────────────────────────────────────────────────┘
```

| 区域 | 尺寸 |
|---|---|
| 标题栏高度 | **36px** (含底部 1px 分割线) |
| 主内容区 padding | **8px** |
| 面板间距 (gap) | **6px** |
| 左侧面板宽度 | **200px** |
| 右侧面板宽度 | **260px** |
| 状态栏高度 | **28px** (含顶部 1px 分割线) |

---

## 2. 标题栏 (TitleBar)

### 2.1 尺寸与布局

```
高: 36px, padding: 10px 16px
┌─────────────────────────────────────────────────────┐
│ ● ● ●            MistTerm              SSH · 2h 34m│
└─────────────────────────────────────────────────────┘
1px 底部: rgba(255,255,255,0.04)
```

### 2.2 元素规格

| 元素 | 属性 | 值 |
|---|---|---|
| **红绿灯** | 间距 (gap) | 7px |
|  | 大小 | 11px × 11px |
|  | 关闭 | `#ff5f56` |
|  | 最小化 | `#ffbd2e` |
|  | 最大化 | `#27c93f` |
| **应用名** | 字号 | 13px |
|  | 色值 | `rgba(255,255,255,0.3)` |
|  | 对齐 | 水平居中 (flex 区域居中) |
| **右侧信息** | 字号 | 11px |
|  | 色值 | `rgba(255,255,255,0.2)` |

> **egui 实现提示**：红绿灯由 `egui::Frame` 窗口装饰提供，无需自己绘制，除非自定义标题栏。若用自定义标题栏，用三个 `egui::Button` 绘制圆形。

---

## 3. 左侧面板 (连接管理) — 200px

### 3.1 展开状态

```
┌──────────────────────┐
│ 连接               − │  ← 面板标题: 10px bold uppercase
├──────────────────────┤
│ 🔍 搜索连接…          │  ← 搜索框: 11px
│ ──────────────────── │
│ 全部 │ 在线 │ 离线     │  ← 分类标签: 10px
│ ──────────────────── │
│ 🖥 生产服务器  2h 34m  │  ← 连接条目: 12px name + 10px meta
│ 🖥 测试服务器   45m    │
│ 🖥 数据库      离线     │
│ 🖥 预发布       10m    │
└──────────────────────┘
```

### 3.2 面板头部

| 属性 | 值 |
|---|---|
| 标题 padding | 9px 10px |
| 标题字号 | 10px |
| 标题字重 | **Bold (600)** |
| 标题颜色 | `rgba(255,255,255,0.2)` |
| 标题字母间距 | 0.5px |
| 标题大写 | 是 |
| `−` 按钮字号 | 14px |
| `−` 按钮行高 | 1 |
| `−` 按钮颜色 | 同面板标题 |

### 3.3 搜索框

| 属性 | 值 |
|---|---|
| 搜索框区域 padding | 4px 8px 6px |
| 输入框 padding | 5px 8px |
| 输入框圆角 | 4px |
| 输入框字体 | 11px Inter |
| 输入框文字颜色 | `rgba(255,255,255,0.5)` |
| 输入框 placeholder | `rgba(255,255,255,0.12)` |
| 输入框背景 | 同面板底色 (`#13131c`) |
| 输入框边框 | 1px `rgba(255,255,255,0.03)` |
| 输入框 focus 边框 | `rgba(102,126,234,0.2)` |

### 3.4 分类标签

| 属性 | 值 |
|---|---|
| 标签栏 | 水平等分三列 |
| 标签字号 | 10px |
| 标签颜色 (默认) | `rgba(255,255,255,0.18)` |
| 标签颜色 (激活) | `rgba(102,126,234,0.5)` |
| 标签 hover | 30% 透明度 |
| 标签圆角 | 3px |
| 标签间距 | 0 (直接相邻) |
| 激活指示条 | 无下划线，仅颜色变化 |

### 3.5 连接条目

| 属性 | 值 |
|---|---|
| 条目 padding | 8px 10px |
| 条目圆角 | 4px |
| 条目间距 | 1px |
| 图标 (🖥) | 11px, 35% 透明度 |
| 名称字号 | 12px |
| 名称颜色 | `rgba(255,255,255,0.5)` |
| 名称 hover 颜色 | `rgba(255,255,255,0.7)` |
| 名称激活颜色 | `rgba(255,255,255,0.75)` |
| 时长字号 | 10px |
| 时长在线颜色 | `rgba(76,175,80,0.25)` |
| 时长离线颜色 | `rgba(255,255,255,0.12)` |
| Hover 背景 | `rgba(255,255,255,0.03)` |
| 激活背景 | `rgba(102,126,234,0.05)` |

### 3.6 折叠状态

```
面板隐藏 (width: 0)，状态栏左侧出现:
▸ 连接 · 3  ⚡ ubuntu@prod-server-01
```

---

## 4. 终端区域

### 4.1 整体结构

```
┌────────────────────────────────────────────┐
│ ● ubuntu@prod-server-01 × │ ● test × │ +  │  ← Tab栏: 36px
├────────────────────────────────────────────┤
│                                            │
│ Last login: Wed Apr 29 21:20:34            │
│ • Linux prod-server-01 · 5.15.0            │
│                                            │
│ ➜ ~ systemctl status nginx                 │  ← 输出13px
│ ● nginx.service — A high ...               │
│    Loaded: loaded (/lib/systemd/...        │
│    Active: active (running) since          │
│    Tasks: 2 · Memory: 8.2M                 │
│                                            │
│ ➜ ~ kubectl █                              │  ← 当前输入行
│                                            │
└────────────────────────────────────────────┘
```

### 4.2 终端区域属性

| 属性 | 值 |
|---|---|
| 背景色 | `#0a0a12` |
| 圆角 | 6px |
| 滚动区 padding | 10px 16px |

### 4.3 Tab 栏

| 属性 | 值 |
|---|---|
| Tab 栏背景 | `rgba(255,255,255,0.02)` |
| Tab padding | 7px 14px |
| Tab 圆点/文字间距 | 6px |
| Tab 字号 | 11px |
| Tab 名称前圆点 | 5px × 5px, `#4CAF50` |
| 激活 Tab 文字 | 80% 亮度 |
| 激活 Tab 背景 | `#0a0a12` (与终端输出区一致) |
| 非激活 Tab 文字 | 25% 亮度 |
| 关闭按钮 `×` | 默认 `opacity: 0`，hover Tab 时 `0.3` |
| 新建按钮 `+` | 25% 透明度 |

### 4.4 终端输出/输入

| 属性 | 值 |
|---|---|
| 字体 | JetBrains Mono |
| 字号 | 13px |
| 行高 | 1.7 |
| 输出与输入 | 在同一个滚动容器内，无分割 |

**行内语法标注颜色：**

| 元素 | CSS 类 | 色值 |
|---|---|---|
| Prompt (`➜`) | `.prompt` | `#4CAF50` |
| 路径 (`~`) | `.path` | `#667eea` |
| 命令 | `.cmd` | `rgba(255,255,255,0.9)` |
| 输出行 | `.out` | `rgba(255,255,255,0.4)` |

> **特别注意**：终端没有独立的输入框行，没有命令建议条，没有快捷工具栏。输出和输入在同一滚动区域中，输入行就是滚动区底部可编辑的内容。

---

## 5. 右侧面板 (命令片段) — 260px

### 5.1 展开状态

```
┌─────────────────────────────┐
│ 命令片段                  − │  ← 面板标题: 10px bold uppercase
├─────────────────────────────┤
│ 🔍 搜索片段…                 │  ← 搜索框: 11px
│ ─────────────────────────── │
│ 常用 │ Docker │ K8s │ 全部   │  ← 分类标签: 10px
│ ─────────────────────────── │
│ 查看 Pod 日志       [团队]   │  ← 片段卡片
│ kubectl logs -f ...          │
│ 320次 · 95%成功 · 1.2s      │
│ ─────────────────────────── │
│ 重启 Nginx          [团队]   │
│ systemctl restart nginx...   │
│ 500次 · 98%成功 · 0.8s      │
│ ─────────────────────────── │
│ 访问日志            [个人]   │
│ tail -f /var/log/nginx...   │
│ 156次 · 100%成功 · 0.3s     │
│ ─────────────────────────── │
│ Docker 健康检查      [模板]  │
│ docker ps --format ...       │
│ 1,200次 · 96%成功 · 0.5s    │
└─────────────────────────────┘
```

### 5.2 搜索框

规格同左侧面板搜索框（见 3.3）。

### 5.3 分类标签

| 属性 | 值 |
|---|---|
| 默认标签 | 常用 / Docker / K8s / 全部 |
| 字号 | 10px |
| 默认色 | `rgba(255,255,255,0.18)` |
| 激活色 | `rgba(102,126,234,0.5)` |
| 圆角 | 3px |

### 5.4 片段卡片

| 属性 | 值 |
|---|---|
| 卡片 padding | 7px 8px |
| 卡片圆角 | 4px |
| 卡片间距 | 0 (用分隔线间隔) |
| 标题字号 | 12px |
| 标题颜色 | `rgba(255,255,255,0.5)` |
| 标题 hover 颜色 | `rgba(255,255,255,0.7)` |
| 标签(tag)字号 | 9px |
| 标签字重 | **Medium (500)** |
| 标签圆角 | 3px |
| 团队标签 | `rgba(76,175,80,0.05)` bg + `rgba(76,175,80,0.35)` text |
| 个人标签 | `rgba(102,126,234,0.05)` bg + `rgba(102,126,234,0.35)` text |
| 模板标签 | `rgba(255,152,0,0.05)` bg + `rgba(255,152,0,0.35)` text |
| 命令原文字号 | 10px JetBrains Mono |
| 命令原文颜色 | `rgba(255,255,255,0.12)` |
| 命令原文 | 单行截断 (ellipsis) |
| 统计字号 | 10px |
| 统计数字颜色 | `rgba(255,255,255,0.25)` |
| 统计标签颜色 | `rgba(255,255,255,0.10)` |
| 卡片 hover 背景 | `rgba(255,255,255,0.03)` |
| 分隔线 | `rgba(255,255,255,0.03)` |

**统计格式**: `{次数}次 · {成功率}%成功 · {耗时}s`

### 5.5 折叠状态

```
面板隐藏 (width: 0)，状态栏右侧出现:
▸ 命令片段 · 5  📋 📤 🔍 📊 · 1,234次 ↑8%
```

---

## 6. 状态栏 — 28px

### 6.1 布局

```
┌───────────────────────────────────────────────────────────────┐
│ ⚡ ubuntu@prod-server-01     │     📋 📤 🔍 📊 · 1,234次 ↑8% │
│          左侧                         右侧                     │
└───────────────────────────────────────────────────────────────┘
顶部 1px 分割线: rgba(255,255,255,0.03)
```

| 属性 | 值 |
|---|---|
| 高度 | 28px |
| padding | 4px 14px |
| 分割线 | 顶部 1px `rgba(255,255,255,0.03)` |
| 左/右间距 | 左 gap: 8px, 右 gap: 4px |

### 6.2 元素规格

| 元素 | 值 |
|---|---|
| **连接信息** `⚡ ubuntu@…` — 字号 11px, 颜色 `rgba(255,255,255,0.12)` |
| **复原按钮** `▸ 连接 · N` — 字号 10px, 颜色 `rgba(102,126,234,0.25)`, padding 1px 5px, 圆角 3px |
| 复原按钮 hover | 颜色 `rgba(102,126,234,0.45)`, 背景 `rgba(102,126,234,0.04)` |
| **工具按钮** 📋📤🔍📊 — 字号 12px, 颜色 `rgba(255,255,255,0.08)`, padding 0 3px, 行高 1 |
| 工具按钮 hover | 颜色 `rgba(255,255,255,0.25)` |
| 工具按钮间距 | 3px + 细空格 ` ` |
| **分隔符** `·` — 颜色 `rgba(255,255,255,0.04)` |
| **统计数字** — 字号 10px, 颜色 `rgba(255,255,255,0.1)` |
| **增长指标** `↑8%` — 字号 11px, 颜色 `rgba(76,175,80,0.3)` |

### 6.3 面板折叠时变化

| 状态 | 左侧 | 右侧 |
|---|---|---|
| 正常 | `⚡ ubuntu@prod-server-01` | 📋📤🔍📊 · 1,234次 ↑8% |
| 左面板收起 | `▸ 连接 · 3  ⚡ ubuntu@...` | 同上 |
| 右面板收起 | 同上 (不变) | `▸ 命令片段 · 5` 📋📤🔍📊 · 1,234次 ↑8% |
| 两侧收起 | `▸ 连接 · 3  ⚡ ubuntu@...` | `▸ 命令片段 · 5` 📋📤🔍📊 · 1,234次 ↑8% |

---

## 7. 圆角汇总

| 元素 | 圆角值 |
|---|---|
| 窗口 | 10px |
| 面板 (侧栏/终端背景) | 6px |
| 连接条目 | 4px |
| 片段卡片 | 4px |
| 搜索框 | 4px |
| 状态栏按钮 | 3px |
| 标签 (团队/个人/模板) | 3px |
| 分类标签 | 3px |
| 复原按钮 | 3px |
| 红绿灯圆点 | 50% (圆形) |

---

## 8. 间距汇总

| 间距 | 值 |
|---|---|
| 面板间 gap | 6px |
| 面板标题 padding | 9px 10px |
| 面板内容 padding | 0 4px 4px |
| 搜索框区域 padding | 4px 8px 6px |
| 搜索框 input padding | 5px 8px |
| 连接条目 padding | 8px 10px |
| 连接条目间距 | 1px |
| Tab padding | 7px 14px |
| Tab 圆点/文字间距 | 6px |
| 终端滚动区 padding | 10px 16px |
| 片段卡片 padding | 7px 8px |
| 状态栏左侧 gap | 8px |
| 状态栏右侧 gap | 4px |
| 工具按钮间距 | 3px |
| 标题栏 padding | 10px 16px |
| 主区域 body padding | 8px |

---

## 9. 交互状态清单

### 9.1 面板折叠/展开

```
点击面板标题栏 `−` 按钮
  → 面板 width → 0, overflow: hidden, opacity: 0
  → 状态栏出现对应 ▸ 按钮
  → 动画: 0.2s ease

点击状态栏 `▸ 名称 · N` 按钮
  → 面板 width → 原值 (200px/260px), opacity: 1
  → 状态栏 ▸ 按钮消失
  → 动画: 0.2s ease
```

### 9.2 连接条目

```
点击条目 → 切换激活态
  → 背景: rgba(102,126,234,0.05)
  → 文字: rgba(255,255,255,0.75)
  → 如果未连接 → 发起 SSH 连接

Hover 条目
  → 背景: rgba(255,255,255,0.03)
  → 名称文字: rgba(255,255,255,0.7)
```

### 9.3 Tab

```
点击 Tab → 切换到对应终端会话
  → 激活: 背景 #0a0a12 + 文字 80% 亮度
  → 非激活: 背景 rgba(255,255,255,0.02) + 文字 25% 亮度

Hover Tab (非激活)
  → 关闭按钮 × 出现 (opacity: 0.3)

点击关闭按钮 × → 关闭 Tab

点击 `+` → 新建 Tab (新建连接选择)
```

### 9.4 片段卡片

```
点击卡片 → 命令填入当前终端输入行末尾

Hover 卡片
  → 背景: rgba(255,255,255,0.03)
  → 标题: rgba(255,255,255,0.7)

点击分类标签
  → 按标签过滤片段列表
  → 标签激活色: rgba(102,126,234,0.5)
```

### 9.5 状态栏按钮

```
Hover 工具按钮
  → 颜色: rgba(255,255,255,0.08) → rgba(255,255,255,0.25)

Hover 复原按钮
  → 文字: rgba(102,126,234,0.25) → rgba(102,126,234,0.45)
  → 背景: rgba(102,126,234,0.04)
```

---

## 10. 动画/过渡

| 场景 | 时长 | 缓动 |
|---|---|---|
| 面板折叠/展开 (width + opacity) | 0.2s | ease |
| Hover 背景/文字色变化 | 0.1s | ease |
| Tab 关闭按钮显示/隐藏 | 0.1s | ease |
| 搜索框 focus 边框 | 0.1s | ease |

**折叠 CSS 等效 (egui 实现)：**
```rust
// 折叠时强制覆盖
if panel_collapsed {
    ui.set_min_width(0.0);
    ui.set_max_width(0.0);
    ui.set_visible(false);
}
```

---

## 11. 滚动条

| 属性 | 值 |
|---|---|
| 宽度 | 4px |
| 颜色 | `rgba(255,255,255,0.06)` |

---

## 12. egui 实现对照

### 12.1 组件 → egui Widget 映射

| 设计组件 | egui 实现 |
|---|---|
| 标题栏 | `TopBottomPanel::top("title_bar", 36.0)` |
| 面板头部 | `egui::Frame` + `egui::Label` (panel_title style) |
| 左侧面板 | `egui::SidePanel::left("sidebar").resizable(false).min_width(0)` |
| 右侧面板 | `egui::SidePanel::right("fragments").resizable(false).min_width(0)` |
| 分类标签 | 一组 `egui::SelectableLabel` |
| 连接条目 | 自定义 widget (egui::Frame + egui::Label + egui::sense) |
| 片段卡片 | 自定义 widget |
| 搜索框 | `egui::TextEdit::singleline(&mut query)` |
| 终端区域 | `egui::Frame` 包裹 TerminalView.show() |
| Tab 栏 | `egui::containers::panel::CentralPanel` 顶部 Button 行 |
| 状态栏 | `TopBottomPanel::bottom("status_bar", 28.0)` |
| 工具按钮 | `egui::Button` (label as emoji) |

### 12.2 折叠面板的 egui 实现技巧

```rust
// 左侧面板折叠
SidePanel::left("sidebar")
    .resizable(false)
    .default_width(200.0)
    .min_width(0.0)
    .show_separator_line(false)
    .show_animated(ctx, !self.sidebar_collapsed, |ui| {
        // 面板内容
    });
// 折叠时 SidePanel 自动收缩到 0

// 状态栏复原按钮 — 仅在折叠时可见
if self.sidebar_collapsed {
    if ui.add(restore_button).clicked() {
        self.sidebar_collapsed = false;
    }
}
```