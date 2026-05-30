//! 桌面端 OAuth：系统浏览器 + 本地 `127.0.0.1` 回调（见 `docs/tech/TEAM.md` §一 A.2.6）。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::client::TeamClient;
use super::models::TokenResponse;
use super::settings::team_web_oauth_desktop_callback_url;

const OAUTH_TIMEOUT_SECS: u64 = 300;

const SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>MistTerm</title></head>
<body style="font-family:system-ui;text-align:center;padding:3rem">
<h2>登录成功</h2><p>可以关闭此窗口并返回 MistTerm。</p>
</body></html>"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Google,
    Github,
}

impl OAuthProvider {
    pub fn path_segment(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Github => "github",
        }
    }
}

#[derive(Debug)]
enum OAuthCallback {
    Code(String),
    Tokens {
        access_token: String,
        refresh_token: String,
    },
    Error(String),
}

/// 在后台线程中调用：打开浏览器，等待本地回调，换取 token。
pub fn run_browser_oauth(
    api_base: &str,
    provider: OAuthProvider,
    cancel: Arc<AtomicBool>,
) -> Result<TokenResponse, String> {
    // 先绑定端口，获取实际监听端口后拼桥接 URL
    let listener = bind_oauth_listener()?;
    let local_addr = listener.local_addr().map_err(|e| e.to_string())?;
    let port = local_addr.port();
    let redirect_local = format!("http://127.0.0.1:{port}/callback");
    let redirect_bridge = format!("{}?port={port}", team_web_oauth_desktop_callback_url());
    // 优先本机回调：token 直达客户端，且规避桥接页上 ?port=…?access_token=… 的错误拼接。
    let redirect_uri = if probe_oauth_start(api_base, provider, &redirect_local).is_ok() {
        redirect_local.clone()
    } else if probe_desktop_oauth_bridge().is_ok() {
        redirect_bridge.clone()
    } else {
        redirect_local.clone()
    };

    if cancel.load(Ordering::Relaxed) {
        return Err("已取消登录".into());
    }

    let auth_url = TeamClient::oauth_authorize_url(api_base, provider, &redirect_uri);

    if !crate::platform::shell::open_url(&auth_url) {
        return Err("无法打开系统浏览器".into());
    }

    let (mut stream, _) = accept_oauth_connection(&listener, &cancel)?;

    let request = read_http_request(&mut stream).map_err(|e| e.to_string())?;
    let _ = write_html_response(&mut stream, SUCCESS_HTML);
    let callback = parse_oauth_callback(&request)?;

    let client = TeamClient::new(api_base).map_err(|e| e.to_string())?;
    match callback {
        OAuthCallback::Code(code) => client
            .oauth_exchange(provider, &code, &redirect_uri)
            .map_err(|e| e.to_string()),
        OAuthCallback::Tokens {
            access_token,
            refresh_token,
        } => Ok(TokenResponse {
            access_token,
            refresh_token,
            user: super::models::TeamUser {
                id: String::new(),
                email: String::new(),
                username: String::new(),
                display_name: String::new(),
                email_verified: false,
                created_at: None,
                updated_at: None,
            },
        }),
        OAuthCallback::Error(msg) => Err(msg),
    }
}

fn bind_oauth_listener() -> Result<TcpListener, String> {
    TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("无法启动本地回调服务: {e}"))
}

fn accept_oauth_connection(
    listener: &TcpListener,
    cancel: &AtomicBool,
) -> Result<(TcpStream, std::net::SocketAddr), String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(OAUTH_TIMEOUT_SECS);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("已取消登录".into());
        }
        if Instant::now() >= deadline {
            return Err(
                "浏览器登录超时：请从 MistTerm 点击 Google/GitHub 登录，在打开的页面完成授权；\
                 仅登录 mistlab.dev 控制台不会完成终端登录。"
                    .into(),
            );
        }
        match listener.accept() {
            Ok(conn) => return Ok(conn),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(80));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// 探测 mistlab.dev 桌面 OAuth 桥接页是否已部署。
fn probe_desktop_oauth_bridge() -> Result<(), String> {
    let url = team_web_oauth_desktop_callback_url();
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = http.get(url).send().map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        return Ok(());
    }
    Err(format!(
        "OAuth bridge page not deployed (HTTP {})",
        resp.status().as_u16()
    ))
}

/// 启动前探测 OAuth 入口是否可达（避免打开 404 页面后空等）。
fn probe_oauth_start(
    api_base: &str,
    provider: OAuthProvider,
    redirect_uri: &str,
) -> Result<(), String> {
    let url = TeamClient::oauth_authorize_url(api_base, provider, redirect_uri);
    let http = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = http.get(&url).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    if status.is_success() || status.is_redirection() {
        return Ok(());
    }
    if status.as_u16() == 404 {
        return Err(
            "团队 API 的 OAuth 接口尚未可用（/v1/oauth/google 返回 404）。\
             请先用邮箱密码登录，或请后端部署 OAuth 并配置桌面 redirect_uri。"
                .into(),
        );
    }
    Err(format!(
        "无法启动 OAuth（HTTP {}）。请稍后重试或使用密码登录。",
        status.as_u16()
    ))
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 64 * 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn write_html_response(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn parse_oauth_callback(request: &str) -> Result<OAuthCallback, String> {
    let line = request
        .lines()
        .next()
        .ok_or_else(|| "无效的 HTTP 请求".to_string())?;
    let path = line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "无效的 HTTP 请求行".to_string())?;
    let query = path.split('?').nth(1).unwrap_or("");
    let params = parse_query_string(query);

    if let Some(err) = params
        .get("error")
        .cloned()
        .or_else(|| params.get("error_description").cloned())
    {
        return Ok(OAuthCallback::Error(err));
    }

    if let Some(access) = params.get("access_token") {
        return Ok(OAuthCallback::Tokens {
            access_token: access.clone(),
            refresh_token: params
                .get("refresh_token")
                .cloned()
                .unwrap_or_default(),
        });
    }

    if let Some(code) = params.get("code") {
        return Ok(OAuthCallback::Code(code.clone()));
    }

    Err("回调中未找到 code 或 token".into())
}

fn parse_query_string(query: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = percent_decode(parts.next().unwrap_or(""));
        let value = percent_decode(parts.next().unwrap_or(""));
        map.insert(key, value);
    }
    map
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(v);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_callback_code() {
        let req = "GET /callback?code=abc123&state=x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        match parse_oauth_callback(req).unwrap() {
            OAuthCallback::Code(c) => assert_eq!(c, "abc123"),
            _ => panic!("expected code"),
        }
    }

    #[test]
    fn parse_callback_tokens() {
        let req = "GET /callback?access_token=a&refresh_token=b HTTP/1.1\r\n\r\n";
        match parse_oauth_callback(req).unwrap() {
            OAuthCallback::Tokens {
                access_token,
                refresh_token,
            } => {
                assert_eq!(access_token, "a");
                assert_eq!(refresh_token, "b");
            }
            _ => panic!("expected tokens"),
        }
    }
}
