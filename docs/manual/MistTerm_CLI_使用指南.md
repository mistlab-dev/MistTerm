# MistTerm 命令行工具 (mist CLI) 使用指南

`mist` 是 MistTerm 客户端配套的命令行工具。它直接复用 MistTerm GUI 的本地加密凭据、已保存会话配置及团队片段库，为终端重度用户提供高效的 SSH 运维与传输能力。

---

## 快速安装与环境

`mist` 与 MistTerm 共享同一本地存储（`~/.config/mistterm` 或平台等效路径）。无需重复输入密码或导入密钥。

```bash
# 查看帮助
mist --help

# 全局参数：
#   --json      输出结构化 JSON（支持 ls / exec / rls / frag 等命令，方便脚本解析）
#   -v, -vv     详细日志输出 (-v INFO，-vv DEBUG)
```

---

## 命令参考

### 1. 会话列表 (`mist ls`)

列出所有已保存的主机/会话，支持分组过滤和 JSON 输出。

```bash
# 列出全部会话
mist ls

# 过滤指定分组
mist ls --group prod

# 以 JSON 格式输出（供自动化脚本消费）
mist --json ls
```

---

### 2. 交互式 Shell (`mist ssh`)

一键直连已保存的会话，或通过快速地址连接。

- 完整支持终端 Raw 模式与 PTY 宽高动态自适应；
- 支持 `~.` 退出转义序列（在远端挂死时强行断开）；
- 自动继承会话中配置的跳板机、证书及认证信息。

```bash
# 通过会话名称或 ID 连接
mist ssh web-prod-01

# 通过快速地址连接（user@host:port）
mist ssh root@192.168.1.50:2202
```

---

### 3. 命令执行与批量运维 (`mist exec`)

在单台或多台机器上快速执行命令，完整透传远端退出码并保护复杂引号命令。

#### 单机执行
```bash
# 在指定目标上运行命令
mist exec web-01 uptime

# 执行复杂复合命令（内部自动做 POSIX 引号转义与安全包裹）
mist exec web-01 -- bash -c 'echo "hello from $HOSTNAME"; uptime'
```

#### 批量执行 (`--group` / `--all`)
```bash
# 对 prod 分组下的所有主机并行执行
mist exec --group prod systemctl status nginx

# 对所有已保存主机执行
mist exec --all uptime

# 控制并行度（默认 8 并发，范围 1-16）
mist exec --group prod --parallel 4 "df -h /"

# 串行执行并在首次失败时熔断
mist exec --group prod --serial "systemctl reload nginx"
```

---

### 4. 端口转发 (`mist fwd`)

将本地、远程端口或 SOCKS5 代理挂载到指定 SSH 隧道，前台运行，支持 `Ctrl+C` 优雅退出。

```bash
# 自动拉起会话中已配置的所有转发规则
mist fwd db-jump-server

# 本地端口转发 (-L [bind_addr:]local_port:remote_host:remote_port)
# 将本地 13306 转发到跳板机内网的 10.0.0.5:3306
mist fwd jump-host -L 13306:10.0.0.5:3306

# 远程端口转发 (-R [bind_addr:]remote_port:target_host:target_port)
# 将远端 8080 转发到本地 8080
mist fwd jump-host -R 8080:127.0.0.1:8080

# 动态 SOCKS5 代理 (-D [bind_addr:]local_port)
# 在本地 1080 端口启动 SOCKS5 代理隧道
mist fwd jump-host -D 1080

# 组合多个转发规则
mist fwd jump-host -L 13306:10.0.0.5:3306 -D 1080
```

---

### 5. SFTP 远程文件操作 (`rls` / `get` / `put`)

通过 SFTP 协议安全查看与传输文件。

#### 列出远端目录 (`mist rls`)
```bash
# 列出远端目录文件
mist rls web-01:/var/log/nginx/

# JSON 格式输出文件列表属性（大小、权限、修改时间）
mist --json rls web-01:/tmp/
```

#### 文件下载 (`mist get`)
```bash
# 下载远端文件到本地文件或目录
mist get web-01:/var/log/nginx/access.log ./access.log
mist get web-01:/etc/hosts ./downloads/
```

#### 文件上传 (`mist put`)
```bash
# 上传本地文件到远端指定文件
mist put ./app.tar.gz web-01:/opt/app/app.tar.gz

# 上传并保留本地文件名（远端目标以 / 结尾）
mist put ./config.json web-01:/opt/app/
```

---

### 6. 命令片段库执行 (`mist frag`)

直接复用 MistTerm 的 Snippet 命令片段，支持参数化变量插值。

#### 列出片段
```bash
mist frag list
mist --json frag list
```

#### 运行片段
```bash
# 在指定目标执行片段
mist frag run check-disk web-01

# 带模板变量执行 (使用 -v 或 --var 传递 key=value)
mist frag run deploy-service web-01 -v env=production -v version=v1.2.0

# 批量执行片段
mist frag run health-check --group prod --parallel 8
```

---

### 7. 从 OpenSSH 配置导入 (`mist import-ssh-config`)

快速将本机的 `~/.ssh/config` 导入 MistTerm 会话库中。

```bash
# 预检模式：仅打印解析结果，不写入本地数据库
mist import-ssh-config --dry-run

# 执行导入（默认跳过同名会话）
mist import-ssh-config

# 指定自定义 ssh config 文件路径
mist import-ssh-config -f /path/to/custom_ssh_config

# 强制覆盖已存在的同名会话
mist import-ssh-config --overwrite
```
