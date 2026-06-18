# MistTerm 操作手册

> 完整用户手册已移至 **`docs/manual/`**，与截图放在同一目录，便于正确显示图片。

## 推荐阅读方式

| 方式 | 文件 | 说明 |
|------|------|------|
| **浏览器（推荐）** | [docs/manual/MistTerm_操作手册.html](docs/manual/MistTerm_操作手册.html) | 双击用浏览器打开，**截图一定能显示** |
| Markdown | [docs/manual/MistTerm_操作手册.md](docs/manual/MistTerm_操作手册.md) | 在 Cursor 中打开后按 `Ctrl+Shift+V` 预览；图片路径为 `screenshots/xx.png` |

## Markdown 预览看不到图？

Markdown **可以**显示图片，语法是 `![](screenshots/01-main-connected.png)`，需满足：

1. **手册与 `screenshots/` 在同一文件夹**（已在 `docs/manual/` 下整理好）
2. **Cursor 允许本地图片预览** — 本仓库已配置 `.vscode/settings.json` 中的 `markdown.preview.securityLevel`。若仍不显示，请在 Cursor 设置里搜索 **Markdown Preview Security**，改为 **Allow insecure local preview**
3. 或使用上面的 **HTML 版本**，不依赖编辑器预览

## 重新导出 HTML

```powershell
python scripts\export-manual-html.py
```

截图目录：`docs/manual/screenshots/`（共 20 张）
