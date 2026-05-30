# CJK 字体（内置回退）

`NotoSansSC-Regular.otf` 来自 [Noto CJK](https://github.com/googlefonts/noto-cjk)（OFL-1.1），
编译时通过 `include_bytes!` 嵌入，避免 Windows/Linux 无系统中文字体时出现方框。

重新下载：

```bash
./scripts/fetch-cjk-font.sh
```
