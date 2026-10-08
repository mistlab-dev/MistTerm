# 从 Xshell、FinalShell 导入会话

把 Xshell、FinalShell 里存的服务器一次搬进 MistTerm：主机地址、端口、用户名、分组都会带过来；能读出的密码也一起带过来，读不出的导入后在会话里填上。

## 在桌面版里导入

1. 菜单「终端 → 从 Xshell / FinalShell 导入…」（也可以在底部状态栏点右键找到）。
2. 选上面的「Xshell」或「FinalShell」。
3. 选位置：
   - **Xshell**：选 Sessions 文件夹，或者在 Xshell 里用「文件 → 导出」得到的 `.xts` 文件。
     Sessions 文件夹一般在：`文档\NetSarang Computer\7\Xshell\Sessions`（6、8 版把 7 换成对应版本号；Xshell 5 是 `文档\NetSarang\Xshell\Sessions`）。
   - **FinalShell**：选 FinalShell 的数据目录（里面有 `conn` 文件夹）。先关掉 FinalShell，保证设置都已保存。
     一般在：Windows `C:\Users\你的用户名\AppData\Local\finalshell`，macOS `~/Library/FinalShell`，Linux `~/.finalshell`。
   如果本机能找到默认位置，打开时会直接列出来。
4. 看一眼列表，勾选要导入的，点「导入所选」。

导入的会话放在原来的分组里；多级分组写成「生产/数据库」。没有分组的放在「Xshell」或「FinalShell」分组。同一个会话再次导入会自动跳过。

## 密码能不能带过来

| 来源 | 密码 |
|------|------|
| FinalShell | 能带过来（任何电脑上都能读出） |
| Xshell 5.0 及更早 | 能带过来 |
| Xshell 5.1 及以后 | 密码和**原来那台电脑的 Windows 账号**绑定：在原来那台电脑上导入，或用 Xshell「文件 → 导出」的 `.xts` 文件，通常能带过来；换了电脑直接拷 Sessions 文件夹就读不出 |
| Xshell 设了主密码 | 读不出（命令行可以提供主密码，见下文） |

读不出的会话照样导入，列表里会写「密码没能读出来，导入后请在会话里重新输入」。导入后在左侧会话列表里右键这个会话 →「编辑」，填上密码再连接。

## 不会带过来的

- Telnet、串口、远程桌面（RDP）这类不是 SSH 的会话：列表里会标出来，不导入。
- 私钥：Xshell 的私钥存在它自己的密钥库里，FinalShell 的私钥存在它的主配置里；导入后请在会话里选择私钥文件。
- 代理、端口转发、终端外观等设置：需要的话导入后在会话里重新设置（列表里会提醒哪些会话用过代理或端口转发）。

## 用命令行导入

```bash
# 先预览，不保存
mist import xshell "C:\Users\me\Documents\NetSarang Computer\7\Xshell\Sessions" --dry-run

# 导入
mist import xshell ~/Downloads/export.xts
mist import finalshell ~/.finalshell

# 不确定是哪种，让 mist 自己判断
mist import auto ./某个文件夹
```

换了电脑、又想读出 Xshell 5.1 以后保存的密码：在**原来那台 Windows 电脑**上运行 `whoami /user`，记下用户名和以 `S-1-5-21-` 开头的那串 SID，然后：

```bash
mist import xshell ./Sessions --windows-user 张三 --windows-sid S-1-5-21-xxxxxxxxxx-xxxxxxxxxx-xxxxxxxxxx-1001
```

Xshell 设了主密码时，用环境变量提供（不会留在命令历史里）：

```bash
MIST_XSHELL_MASTER_PASSWORD='你的主密码' mist import xshell ./Sessions
```

导入完可以直接用：`mist ls` 看列表，`mist exec <会话名> -- uptime` 试连。

## 格式依据和自测（给开发者）

没有官方格式文档，下面是实现依据的公开资料，测试里用了其中的公开样例：

- Xshell `.xsh`：INI 格式（`[CONNECTION] Host/Port/Protocol`、`[CONNECTION:AUTHENTICATION] UserName/Password/UserKey`、`[SessionInfo] Version`），Xshell 6/7 多为 UTF-16；分组就是子文件夹。密码加密方式见 [HyperSine/how-does-Xmanager-encrypt-password](https://github.com/HyperSine/how-does-Xmanager-encrypt-password)（5.0 及以前、5.1/5.2、5.2 以后、主密码；`.xts` 的 `xts.zcf`），7.x 的密钥拼法见 [JDArmy/SharpXDecrypt](https://github.com/JDArmy/SharpXDecrypt)。
- FinalShell：`conn/**/<id>_connect_config.json`（`host`、`port`、`user_name`、`password`、`parent_id`、`conection_type` 100=SSH / 101=远程桌面、`authentication_type` 1=密码 / 2=私钥）和 `conn/**/folder.json`（`id`、`name`、`parent_id`，顶层是 `root`）；`backup/`、`deleted/` 下是旧版本和已删除的，不导入。密码算法见 [XLevon/FinalshellDecoderX](https://github.com/XLevon/FinalshellDecoderX)，目录结构见 [final2halo](https://github.com/final2halo/final2halo)。

测试样例在 `tests/fixtures/foreign_import/`，由 `scripts/gen-foreign-import-fixtures.py` 生成（用 Python 独立实现加密，和 Rust 的解密交叉验证）。指向一台测试 sshd 生成样例、导入后试连：

```bash
python3 scripts/gen-foreign-import-fixtures.py --out /tmp/fx --host 127.0.0.1 --port 2299 --user test --password '测试密码'
mist import xshell /tmp/fx/xshell/Sessions --windows-user tian --windows-sid S-1-5-21-917267712-1342860078-1792151419-512
mist import finalshell /tmp/fx/finalshell
mist exec --group 测试组 -- uptime
```
