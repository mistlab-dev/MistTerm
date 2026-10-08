#!/usr/bin/env python3
"""生成 tests/fixtures/foreign_import 下的 Xshell / FinalShell 样例文件（测试用，不含真实密码）。

加密方式按公开资料实现（独立于 Rust 代码，用来交叉验证）：
- Xshell：https://github.com/HyperSine/how-does-Xmanager-encrypt-password
  以及 7.x 的密钥拼法 https://github.com/JDArmy/SharpXDecrypt（C#/XClass.cs）
- FinalShell：https://github.com/XLevon/FinalshellDecoderX（src/core.py）

用法：python3 -m venv v && v/bin/pip install pycryptodome && v/bin/python scripts/gen-foreign-import-fixtures.py [--host H --port P --user U --password W]
（--host 等参数用于生成指向测试 sshd 的样例，见 docs/manual/从Xshell和FinalShell导入.md 的「自测」一节。）
"""
import argparse
import base64
import hashlib
import json
import os
import struct
import zipfile

from Crypto.Cipher import ARC4, DES

SID = "S-1-5-21-917267712-1342860078-1792151419-512"


def xshell_enc(version: str, pw: str, user: str = "", sid: str = SID, master: str = "") -> str:
    v = tuple(int(x) for x in version.split(".")[:2])
    if v < (5, 1):
        return base64.b64encode(ARC4.new(hashlib.md5(b"!X@s#h$e%l^l&").digest()).encrypt(pw.encode())).decode()
    if master:
        material = master
    elif v <= (5, 2):
        material = sid
    elif v < (7, 1):
        material = user + sid
    else:
        material = (user[::-1] + sid)[::-1]
    key = hashlib.sha256(material.encode()).digest()
    ct = ARC4.new(key).encrypt(pw.encode())
    return base64.b64encode(ct + hashlib.sha256(pw.encode()).digest()).decode()


def xts_field(s: str) -> str:
    ct = ARC4.new(hashlib.md5(b"!X@s#c$e%l^l&").digest()).encrypt(s.encode())
    return base64.b64encode(ct + hashlib.md5(s.encode()).digest()).decode()


def xsh(version, host, port, user, password="", protocol="SSH", user_key="", method="0"):
    return (
        f"[SessionInfo]\r\nVersion={version}\r\nDescription=Xshell session file\r\n"
        f"[CONNECTION]\r\nHost={host}\r\nPort={port}\r\nProtocol={protocol}\r\nAutoReconnect=0\r\n"
        f"[CONNECTION:AUTHENTICATION]\r\nMethod={method}\r\nUserName={user}\r\nPassword={password}\r\nUserKey={user_key}\r\nPassphrase=\r\n"
        f"[TERMINAL]\r\nType=xterm\r\n"
    )


# ---- FinalShell：Java Random + DES（与公开解密工具相同的推导）
class JavaRandom:
    def __init__(self, seed):
        self.seed = (seed ^ 0x5DEECE66D) & ((1 << 48) - 1)

    def next(self, bits):
        self.seed = (self.seed * 0x5DEECE66D + 0xB) & ((1 << 48) - 1)
        v = self.seed >> (48 - bits)
        v &= 0xFFFFFFFF
        return v - (1 << 32) if v >= (1 << 31) else v

    def next_int(self, bound):
        if bound & -bound == bound:
            return (bound * self.next(31)) >> 31
        while True:
            bits = self.next(31)
            val = bits % bound
            if bits - val + (bound - 1) < (1 << 31):
                return val

    def next_long(self):
        x = (self.next(32) << 32) + self.next(32)
        x &= (1 << 64) - 1
        return x - (1 << 64) if x >= (1 << 63) else x


def sb(b):
    return b - 256 if b > 127 else b


def fs_key(head: bytes) -> bytes:
    ks = 3680984568597093857 // JavaRandom(sb(head[5])).next_int(127)
    r = JavaRandom(ks)
    for _ in range(max(sb(head[0]), 0)):
        r.next_long()
    r2 = JavaRandom(r.next_long())
    ld = [sb(head[4]), r2.next_long(), sb(head[7]), sb(head[3]), r2.next_long(), sb(head[1]), r.next_long(), sb(head[2])]
    return hashlib.md5(b"".join(struct.pack(">q", v) for v in ld)).digest()[:8]


def fs_enc(pw: str, head: bytes) -> str:
    data = pw.encode()
    pad = 8 - len(data) % 8
    data += bytes([pad]) * pad
    return base64.b64encode(head + DES.new(fs_key(head), DES.MODE_ECB).encrypt(data)).decode()


def fs_host(id_, name, host, port, user, password="", parent="root", auth=1, conn_type=100, proxy="0"):
    return {
        "id": id_, "name": name, "host": host, "port": port, "user_name": user, "password": password,
        "parent_id": parent, "authentication_type": auth, "conection_type": conn_type, "secret_key_id": "",
        "proxy_id": proxy, "port_forwarding_list": [], "remote_port_forwarding": {}, "description": "",
        "terminal_encoding": "UTF-8", "order": 0, "delete_time": 0,
    }


def write(path, data: bytes):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=os.path.join(os.path.dirname(__file__), "..", "tests", "fixtures", "foreign_import"))
    ap.add_argument("--host", default="")
    ap.add_argument("--port", type=int, default=22)
    ap.add_argument("--user", default="")
    ap.add_argument("--password", default="")
    a = ap.parse_args()
    out = os.path.abspath(a.out)

    if a.host:
        # 指向测试 sshd 的最小样例：每种格式 / 每种密码方式各一个
        u, p, h, port = a.user, a.password, a.host, a.port
        sess = os.path.join(out, "xshell", "Sessions")
        write(os.path.join(sess, "测试组", "x50.xsh"), b"\xff\xfe" + xsh("5.0", h, port, u, xshell_enc("5.0", p)).encode("utf-16-le"))
        write(os.path.join(sess, "测试组", "x71.xsh"), b"\xff\xfe" + xsh("7.1", h, port, u, xshell_enc("7.1", p, user="tian")).encode("utf-16-le"))
        conn = os.path.join(out, "finalshell", "conn")
        write(os.path.join(conn, "g1", "folder.json"), json.dumps({"id": "g1", "name": "测试组", "parent_id": "root"}).encode())
        write(os.path.join(conn, "g1", "fs1_connect_config.json"), json.dumps(fs_host("fs1", "fs-host", h, port, u, fs_enc(p, os.urandom(8)), parent="g1")).encode())
        print("written", out)
        return

    sess = os.path.join(out, "xshell", "Sessions")
    # Xshell 7：UTF-16LE 带 BOM，密码和 Windows 账号 tian + SID 绑定
    write(os.path.join(sess, "生产", "web-01.xsh"), b"\xff\xfe" + xsh("7.1", "10.0.0.11", 22, "root", xshell_enc("7.1", "mist-test-pw", user="tian")).encode("utf-16-le"))
    # Xshell 5.0：固定密钥，哪台电脑都能解
    write(os.path.join(sess, "生产", "数据库", "db-01.xsh"), xsh("5.0", "10.0.1.21", 2222, "dba", xshell_enc("5.0", "This is a test")).encode("utf-8"))
    # Xshell 6：UTF-8 无 BOM，用户名 Administrator
    write(os.path.join(sess, "测试机.xsh"), xsh("6.0", "test.example.com", 22, "ops", xshell_enc("6.0", "This is a test", user="Administrator")).encode("utf-8"))
    # 主密码
    write(os.path.join(sess, "主密码.xsh"), b"\xff\xfe" + xsh("7.0", "10.0.0.99", 22, "root", xshell_enc("7.0", "mp-secret", master="my-master")).encode("utf-16-le"))
    # 不导入：Telnet
    write(os.path.join(sess, "telnet-box.xsh"), b"\xff\xfe" + xsh("7.1", "10.0.0.50", 23, "admin", protocol="TELNET").encode("utf-16-le"))
    # 私钥登录
    write(os.path.join(sess, "key-login.xsh"), b"\xff\xfe" + xsh("7.1", "10.0.0.60", 22, "ubuntu", user_key="id_rsa_2048", method="1").encode("utf-16-le"))

    # .xts：Xshell「文件 → 导出」，xts.zcf 里带导出时的账号
    xts = os.path.join(out, "xshell", "export.xts")
    os.makedirs(os.path.dirname(xts), exist_ok=True)
    with zipfile.ZipFile(xts, "w") as z:
        zcf = f"[SessionInfo]\r\nVersion=7.1\r\nUN={xts_field('tian')}\r\nCN={xts_field('DESKTOP-TEST')}\r\nSI={xts_field(SID)}\r\n"
        z.writestr("xts.zcf", zcf.encode("utf-16-le"))
        z.writestr("Xshell/生产/web-01.xsh", b"\xff\xfe" + xsh("7.1", "10.0.0.11", 22, "root", xshell_enc("7.1", "mist-test-pw", user="tian")).encode("utf-16-le"))
        z.writestr("Xshell/跳板机.xsh", b"\xff\xfe" + xsh("6.0", "10.0.0.1", 22, "jump", xshell_enc("6.0", "jump-pw", user="tian")).encode("utf-16-le"))

    fs = os.path.join(out, "finalshell")
    conn = os.path.join(fs, "conn")
    write(os.path.join(conn, "f1", "folder.json"), json.dumps({"id": "f1", "name": "生产", "parent_id": "root", "order": 0}).encode())
    write(os.path.join(conn, "f1", "f2", "folder.json"), json.dumps({"id": "f2", "name": "数据库", "parent_id": "f1", "order": 0}).encode())
    write(os.path.join(conn, "f1", "h1_connect_config.json"), json.dumps(fs_host("h1", "web-01", "10.0.0.11", 22, "root", fs_enc("mist-test-pw", bytes([1, 2, 3, 4, 5, 6, 7, 8])), parent="f1")).encode())
    write(os.path.join(conn, "f1", "f2", "h2_connect_config.json"), json.dumps(fs_host("h2", "db-01", "10.0.1.21", 3306 - 1084, "dba", fs_enc("数据库密码", bytes([200, 13, 99, 7, 250, 31, 0, 128])), parent="f2", proxy="p9")).encode())
    write(os.path.join(conn, "h3_connect_config.json"), json.dumps(fs_host("h3", "key-only", "10.0.0.60", 22, "ubuntu", auth=2)).encode())
    write(os.path.join(conn, "h4_connect_config.json"), json.dumps(fs_host("h4", "win-desktop", "10.0.0.70", 3389, "administrator", fs_enc("x", bytes(8)), conn_type=101)).encode())
    # 已删除 / 备份里的旧主机不能导入
    write(os.path.join(fs, "deleted", "old", "h9_connect_config.json"), json.dumps(fs_host("h9", "deleted-host", "10.9.9.9", 22, "root")).encode())
    write(os.path.join(conn, "backup", "h8_connect_config.json"), json.dumps(fs_host("h8", "backup-host", "10.8.8.8", 22, "root")).encode())
    print("written", out)


if __name__ == "__main__":
    main()
