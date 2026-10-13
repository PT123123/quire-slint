// MCP (Model Context Protocol) 接入：让外部 AI Agent 读写这份笔记库。
//
// 三部分，一个文件：
//   1. 协议核心 —— 纯函数，JSON-RPC 2.0 上的 initialize / tools/list /
//      tools/call / ping。`handle_message` 不碰任何 IO，测试直接喂 JSON。
//   2. HTTP 传输 —— `McpServer`，绑 127.0.0.1，Bearer token 鉴权，复用
//      `services::lan_server` 的无依赖手法（std::net，一线程一连接）。
//      tools/call 经由调用方注入的回调出去：应用层把它翻译成一个
//      channel 任务，丢给 UI 线程的泵执行（AppState 是 Rc，出不了线程），
//      再等回复。HTTP 线程自己不认识 AppState。
//   3. stdio 桥 —— `quire.exe mcp` 子命令。MCP 客户端最普遍的接入方式是
//      spawn 一个子进程走 stdin/stdout，而唯一写者是运行中的应用，所以
//      桥只做转发：一行 JSON 进来 → POST 到应用内嵌的 HTTP 端点 → 响应
//      写回 stdout。端口和 token 从库目录旁的 `mcp.json` 读（应用启动
//      MCP 服务时写入），`--port` 可覆盖前者。
//
// 为什么不用 stdio 直接开数据库：id 的 watermark 在应用的内存里
//（core ADR-0067 的caller-allocates 规则），第二个写者会撞号；而且防抖
// 持久化管线意味着 UI 不会看到外部写进来的行。所以应用是唯一写者，
// Agent 的每个调用都走和键盘输入完全相同的路径。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

/// 5878 is `services::sync::SYNC_PORT` — every Quire on the LAN listens there,
/// by design and by documentation (ADR-0128 counts a peer by whether 5878
/// answers). Double-booking it worked only because `controller::wire` binds the
/// wildcard before this module asks for the loopback address, and a later swap
/// of that order would fail the *sync* bind while the MCP page still looked
/// healthy. 5880 is next free: 5877 LAN share, 5878 sync, 5879 discovery.
pub const DEFAULT_PORT: u16 = 5880;

/// 我们能原样回给客户端的协议版本。协商规则：请求带来的版本在这张表里
/// 就回它，否则回最新的——MCP 规范要求服务端这样降级。
const SUPPORTED_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];
const LATEST_VERSION: &str = "2025-06-18";

/// 一个工具的定义，原样进 `tools/list` 的响应。
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema（`{"type":"object","properties":{…}}`）。
    pub input_schema: Value,
}

impl ToolDef {
    fn object(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
        })
    }
}

/// 应用层交给 `tools/call` 的执行者：`(工具名, 参数) -> 结果或错误`。
/// 应用侧的实现会跨线程到 UI 再回来，所以这里只要求 Send + Sync。
pub type CallFn =
    Arc<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync>;

/// 协议核心对一条消息的回答。`None` 是通知（不该有响应）。
pub enum Reply {
    /// JSON-RPC result。
    Result(Value),
    /// JSON-RPC error（code, message）。
    Error(i64, String),
    /// 通知或无需回答。
    Silent,
}

/// 处理一条已解析的 JSON-RPC 消息。`id` 是请求方的 id（通知为 None）。
pub fn handle_message(
    tools: &[ToolDef],
    method: &str,
    params: &Value,
    id: Option<&Value>,
    call: &CallFn,
) -> Reply {
    match method {
        "initialize" => {
            let asked = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(LATEST_VERSION);
            let version = if SUPPORTED_VERSIONS.contains(&asked) {
                asked
            } else {
                LATEST_VERSION
            };
            Reply::Result(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "quire",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }))
        }
        "ping" => Reply::Result(json!({})),
        "tools/list" => Reply::Result(json!({
            "tools": tools.iter().map(ToolDef::object).collect::<Vec<_>>(),
        })),
        "tools/call" => {
            if id.is_none() {
                return Reply::Silent; // 通知形式的 call 不回答
            }
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(name, args) {
                Ok(v) => Reply::Result(json!({
                    "content": [{ "type": "text", "text": v.to_string() }],
                    "isError": false,
                })),
                Err(e) => Reply::Result(json!({
                    "content": [{ "type": "text", "text": e }],
                    "isError": true,
                })),
            }
        }
        // 客户端的 initialized / cancelled 等通知一律静默（含未知通知）；
        // 未知方法若带 id 则落到下面报 -32601。
        _m if id.is_none() => Reply::Silent,
        _ => Reply::Error(-32601, format!("method not found: {method}")),
    }
}

/// 处理一个 HTTP body 里的 JSON-RPC 请求（MCP 允许单条；批量已从
/// 2025-06-18 起移除，见到数组按 -32600 拒绝）。返回要回给客户端的
/// JSON 文本；`None` 表示 202（通知，空响应体）。
pub fn handle_request(tools: &[ToolDef], body: &str, call: &CallFn) -> Option<String> {
    let msg: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            return Some(err_text(None, -32700, "parse error"));
        }
    };
    if msg.is_array() {
        return Some(err_text(None, -32600, "batch requests are not supported"));
    }
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let id = msg.get("id").filter(|v| !v.is_null());
    match handle_message(tools, method, msg.get("params").unwrap_or(&Value::Null), id, call) {
        Reply::Result(v) => Some(json!({ "jsonrpc": "2.0", "id": id.cloned().unwrap_or(Value::Null), "result": v }).to_string()),
        Reply::Error(code, message) => {
            Some(err_text(id.cloned(), code, &message))
        }
        Reply::Silent => None,
    }
}

fn err_text(id: Option<Value>, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": { "code": code, "message": message },
    })
    .to_string()
}

/// 内嵌在运行中的应用里的 HTTP 服务。默认关闭；绑定 127.0.0.1，
/// 每个请求都要带对 token（`Authorization: Bearer …` 或 `X-Quire-Token`）。
pub struct McpServer {
    port: u16,
    token: String,
    tools: Vec<ToolDef>,
    call: CallFn,
}

impl McpServer {
    pub fn new(port: u16, token: String, tools: Vec<ToolDef>, call: CallFn) -> Self {
        McpServer { port, token, tools, call }
    }

    pub fn bind(&self) -> Result<TcpListener, String> {
        TcpListener::bind(("127.0.0.1", self.port)).map_err(|e| e.to_string())
    }

    /// 在已绑定的 listener 上服务到永远（调用方开线程）。
    pub fn serve(&self, listener: TcpListener) {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let this = McpServer {
                port: self.port,
                token: self.token.clone(),
                tools: self.tools.clone(),
                call: self.call.clone(),
            };
            std::thread::spawn(move || {
                let _ = this.handle_connection(stream);
            });
        }
    }

    fn handle_connection(&self, stream: TcpStream) -> std::io::Result<()> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");
        let mut headers: HashMap<String, String> = HashMap::new();
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some((name, value)) = trimmed.split_once(':') {
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().to_string();
                if name == "content-length" {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.insert(name, value);
            }
        }
        let authorized = headers
            .get("authorization")
            .map(|v| v.trim_start_matches("Bearer ").trim() == self.token)
            .unwrap_or(false)
            || headers.get("x-quire-token").map(|v| v.as_str() == self.token).unwrap_or(false);

        let path = path.split('?').next().unwrap_or(path);
        if path != "/mcp" {
            return respond(&stream, 404, "application/json", "{\"error\":\"not found\"}");
        }
        if !authorized {
            return respond(&stream, 401, "application/json", "{\"error\":\"unauthorized\"}");
        }
        if method != "POST" {
            // SSE 长连接（GET）这个实现不支持：客户端按规范的回退
            // （每请求一条 POST）照常工作。
            return respond(&stream, 405, "application/json", "{\"error\":\"POST only\"}");
        }
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body)?;
        let body = String::from_utf8_lossy(&body).to_string();
        match handle_request(&self.tools, &body, &self.call) {
            Some(text) => respond(&stream, 200, "application/json", &text),
            None => respond(&stream, 202, "text/plain", ""),
        }
    }
}

fn respond(mut stream: &TcpStream, code: u16, ctype: &str, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        202 => "Accepted",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

// ─── mcp.json 指针文件 ───────────────────────────────────────────────────────
//
// 桥进程要知道两件事：端口和 token。都写在库目录旁的 `mcp.json`（应用开
// 服务时写），桥用同一套 data_location 规则解析目录，不碰 SQLite。

/// 指针文件：`{"port": 5880, "token": "…"}`。
pub fn pointer_path(dir: &Path) -> PathBuf {
    dir.join("mcp.json")
}

pub fn write_pointer(dir: &Path, port: u16, token: &str) -> std::io::Result<()> {
    std::fs::write(
        pointer_path(dir),
        json!({ "port": port, "token": token }).to_string(),
    )
}

pub fn read_pointer(dir: &Path) -> Option<(u16, String)> {
    let text = std::fs::read_to_string(pointer_path(dir)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let port = v.get("port")?.as_u64()? as u16;
    let token = v.get("token")?.as_str()?.to_string();
    Some((port, token))
}

/// 没有依赖可用的随机源：拿 `RandomState` 的种子哈希两次进程状态。
/// 127.0.0.1 上的本地 token，够用。
pub fn mint_token() -> String {
    use std::hash::{BuildHasher, Hash, Hasher};
    let mut a = std::collections::hash_map::RandomState::new().build_hasher();
    std::time::SystemTime::now().hash(&mut a);
    std::process::id().hash(&mut a);
    let hi = a.finish();
    let mut b = std::collections::hash_map::RandomState::new().build_hasher();
    hi.hash(&mut b);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        .hash(&mut b);
    let lo = b.finish();
    format!("{hi:016x}{lo:016x}")
}

// ─── stdio 桥（`quire.exe mcp` 子命令）──────────────────────────────────────

/// 桥的启动参数：`quire.exe mcp [--port N] [--db <path>] [--token <t>]`。
/// 端口缺省从指针文件读；token 只信指针文件或显式参数（不给的话，谁拿
/// 到本机端口谁就能写库，所以缺了就拒绝启动）。
pub fn run_stdio_bridge(argv: &[String]) -> i32 {
    let mut port: Option<u16> = None;
    let mut token: Option<String> = None;
    let mut db: Option<PathBuf> = None;
    let mut i = 1; // argv[0] 之后第一个词是 "mcp"
    while i < argv.len() {
        let arg = argv[i].as_str();
        match arg {
            "--port" => {
                port = argv.get(i + 1).and_then(|v| v.parse().ok());
                i += 1;
            }
            "--token" => {
                token = argv.get(i + 1).cloned();
                i += 1;
            }
            "--db" => {
                db = argv.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let dir = crate::storage::data_location::data_dir(&crate::storage::data_location::LaunchOptions {
        db_override: db,
        portable: false,
    });
    let (p, t) = match (port, read_pointer(&dir), token) {
        (Some(p), _, Some(t)) => (p, t),
        (Some(p), Some((_, t)), None) => (p, t),
        (None, Some((p, t)), _) => (p, t),
        // 只有端口没有 token 是拒绝启动而不是静默降级：这个服务的
        // token 就是唯一的门，缺了它谁拿到端口谁就能写库。
        _ => {
            eprintln!(
                "quire mcp: 找不到 {}/mcp.json — Quire 正在运行且已在设置里开启 MCP 吗？",
                dir.display()
            );
            eprintln!("quire mcp: 也可以 --port N --token T 手工指定（token 要和应用里的一致）。");
            return 2;
        }
    };
    let base = format!("http://127.0.0.1:{p}/mcp");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        // 请求先在本地解析出 id：这样“应用不在”这类桥自身的错误也能
        // 按协议回一条 error，客户端不至于把 stdio 断掉。
        let id = serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|v| v.get("id").filter(|i| !i.is_null()).cloned());
        match post_json(&base, &token_of(&t), &line) {
            Ok(Some(resp)) => {
                if writeln!(out, "{resp}").and_then(|_| out.flush()).is_err() {
                    break;
                }
            }
            Ok(None) => {} // 通知：无响应
            Err(e) => {
                let text = err_text(id, -32000, &format!("bridge: {e}"));
                let _ = writeln!(out, "{text}").and_then(|_| out.flush());
            }
        }
    }
    0
}

fn token_of(t: &str) -> String {
    format!("Bearer {t}")
}

/// POST 一条消息；`Ok(None)` = 202（通知）。有 body 就原样转发。
fn post_json(url: &str, auth: &str, body: &str) -> Result<Option<String>, String> {
    let (host, port, path) = parse_url(url)?;
    let mut stream = TcpStream::connect((host.as_str(), port))
        .map_err(|e| format!("连不上 Quire（{e}）— 应用还开着吗？"))?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {auth}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).map_err(|e| e.to_string())?;
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(v) = trimmed
            .split_once(':')
            .filter(|(n, _)| n.eq_ignore_ascii_case("content-length"))
            .map(|(_, v)| v.trim().to_string())
        {
            length = v.parse().unwrap_or(0);
        }
    }
    if status_line.contains(" 202") {
        return Ok(None);
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Some(String::from_utf8_lossy(&body).to_string()))
}

fn parse_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("只支持 http:// — {url}"))?;
    let (hostport, path) = rest.split_once('/').map_or(
        (rest.to_string(), "/".to_string()),
        |(h, p)| (h.to_string(), format!("/{p}")),
    );
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().map_err(|_| "端口不是数字")?),
        None => (hostport.to_string(), 80),
    };
    Ok((host, port, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn noop(_: &str, _: Value) -> Result<Value, String> {
        Ok(json!({}))
    }

    fn call_recording(names: Arc<Mutex<Vec<String>>>) -> CallFn {
        Arc::new(move |name: &str, _: Value| {
            names.lock().unwrap().push(name.to_string());
            Ok(json!({"ok": true}))
        })
    }

    fn req(method: &str, id: Value, params: Value) -> String {
        json!({"jsonrpc":"2.0","method":method,"id":id,"params":params}).to_string()
    }

    #[test]
    fn initialize_negotiates_a_known_version_and_falls_back() {
        let tools: Vec<ToolDef> = Vec::new();
        let call: CallFn = Arc::new(noop);
        let out = handle_request(&tools, &req("initialize", json!(1), json!({"protocolVersion":"2025-03-26"})), &call).unwrap();
        assert!(out.contains("2025-03-26"), "{out}");
        let out = handle_request(&tools, &req("initialize", json!(1), json!({"protocolVersion":"1999-01-01"})), &call).unwrap();
        assert!(out.contains(LATEST_VERSION), "{out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["result"]["serverInfo"]["name"], "quire");
    }

    #[test]
    fn tools_list_and_call_round_trip() {
        let tools = vec![ToolDef {
            name: "list_pages",
            description: "page tree",
            input_schema: json!({"type":"object","properties":{}}),
        }];
        let seen = Arc::new(Mutex::new(Vec::new()));
        let call = call_recording(seen.clone());
        let out = handle_request(&tools, &req("tools/list", json!(2), json!({})), &call).unwrap();
        assert!(out.contains("list_pages"), "{out}");
        let out = handle_request(
            &tools,
            &req("tools/call", json!(3), json!({"name":"list_pages","arguments":{}})),
            &call,
        )
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["result"]["content"][0]["type"], "text");
        assert_eq!(v["result"]["isError"], false);
        assert_eq!(*seen.lock().unwrap(), vec!["list_pages".to_string()]);
    }

    #[test]
    fn tool_errors_are_is_error_results_not_rpc_errors() {
        let tools = vec![ToolDef {
            name: "boom",
            description: "",
            input_schema: json!({"type":"object"}),
        }];
        let call: CallFn = Arc::new(|_: &str, _: Value| Err("页面不存在".into()));
        let out = handle_request(&tools, &req("tools/call", json!(4), json!({"name":"boom"})), &call).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["result"]["isError"].as_bool().unwrap());
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("页面不存在"));
        assert!(v.get("error").is_none());
    }

    #[test]
    fn notifications_silence_and_unknown_methods_report() {
        let tools: Vec<ToolDef> = Vec::new();
        let call: CallFn = Arc::new(noop);
        // 通知（无 id）：不回答
        assert!(handle_request(&tools, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &call).is_none());
        // 未知方法带 id：-32601
        let out = handle_request(&tools, &req("resources/list", json!(5), json!({})), &call).unwrap();
        assert!(out.contains("-32601"), "{out}");
    }

    #[test]
    fn parse_and_batch_errors() {
        let tools: Vec<ToolDef> = Vec::new();
        let call: CallFn = Arc::new(noop);
        assert!(handle_request(&tools, "not json", &call).unwrap().contains("-32700"));
        assert!(handle_request(&tools, "[]", &call).unwrap().contains("-32600"));
    }

    #[test]
    fn pointer_file_round_trips() {
        let dir = std::env::temp_dir().join(format!("quire-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_pointer(&dir, DEFAULT_PORT, "tok").unwrap();
        assert_eq!(
            read_pointer(&dir),
            Some((DEFAULT_PORT, "tok".to_string()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn server_refuses_wrong_token_and_wrong_path() {
        let this = McpServer::new(0, "secret".into(), Vec::new(), Arc::new(noop));
        let listener = this.bind().unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || this.serve(listener));
        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = "POST /mcp HTTP/1.1\r\nAuthorization: Bearer wrong\r\nContent-Length: 2\r\n\r\n{}";
        s.write_all(req.as_bytes()).unwrap();
        let mut buf = String::new();
        BufReader::new(s).read_to_string(&mut buf).unwrap();
        assert!(buf.contains("401"), "{buf}");

        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = "GET /other HTTP/1.1\r\n\r\n";
        s.write_all(req.as_bytes()).unwrap();
        let mut buf = String::new();
        BufReader::new(s).read_to_string(&mut buf).unwrap();
        assert!(buf.contains("404"), "{buf}");
        drop(t);
    }
}
