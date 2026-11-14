//! OpenAI 兼容的 /chat/completions 流式调用（SSE）。
//! 适用于 OpenAI、DeepSeek、通义千问兼容模式、Kimi、智谱、OpenRouter、Ollama、LM Studio 等。

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEvent {
    /// 正文增量
    Delta { text: String },
    /// 推理模型的思考过程增量（DeepSeek-R1 等的 reasoning_content）
    Reasoning { text: String },
    /// 用量（部分服务在最后一个包里给）
    Usage { prompt: u64, completion: u64 },
    Done,
    Error { message: String },
    /// 模型要调用工具（流里分段到达，按 index 拼起来）。只在 Rust 里用，不发给界面
    #[serde(skip)]
    ToolCall { index: usize, id: String, name: String, args: String },
    /// Agent 用了一个工具：界面上显示成一行「工具药丸」。status：run / done / no（被拒绝）/ err
    Tool { call: String, agent: String, name: String, arg: String, status: String, result: String },
    /// 需要用户确认（发送、删除、写文件等）。界面显示确认卡片，用户的选择通过 answer_ask 传回
    Ask { ask: u64, call: String, kind: String, desc: String, scope: String, hi: bool },
}

/// 把字节流切成 SSE 事件。网络包可能在任意位置断开（包括 UTF-8 字符中间），所以按字节缓冲。
#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
    pub finished: bool,
}

impl SseParser {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<ChatEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            self.handle_line(line, &mut out);
        }
        out
    }

    /// 流结束时处理残留的最后一行（有的服务最后一行没有换行）
    pub fn flush(&mut self) -> Vec<ChatEvent> {
        let mut out = Vec::new();
        if !self.buf.is_empty() {
            let line = String::from_utf8_lossy(&std::mem::take(&mut self.buf)).to_string();
            self.handle_line(line.trim_end(), &mut out);
        }
        out
    }

    fn handle_line(&mut self, line: &str, out: &mut Vec<ChatEvent>) {
        let Some(data) = line.strip_prefix("data:") else { return };
        let data = data.trim();
        if data.is_empty() {
            return;
        }
        if data == "[DONE]" {
            self.finished = true;
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { return };
        if let Some(err) = v.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| err.to_string());
            out.push(ChatEvent::Error { message: msg });
            return;
        }
        if let Some(delta) = v.pointer("/choices/0/delta") {
            for key in ["reasoning_content", "reasoning"] {
                if let Some(t) = delta.get(key).and_then(Value::as_str) {
                    if !t.is_empty() {
                        out.push(ChatEvent::Reasoning { text: t.to_string() });
                    }
                }
            }
            if let Some(t) = delta.get("content").and_then(Value::as_str) {
                if !t.is_empty() {
                    out.push(ChatEvent::Delta { text: t.to_string() });
                }
            }
            push_tool_calls(delta, out);
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            out.push(ChatEvent::Usage {
                prompt: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
                completion: u.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
            });
        }
    }
}

/// delta.tool_calls / message.tool_calls → ToolCall 事件
fn push_tool_calls(v: &Value, out: &mut Vec<ChatEvent>) {
    for (i, c) in v.get("tool_calls").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let s = |p: &str| c.pointer(p).and_then(Value::as_str).unwrap_or("").to_string();
        out.push(ChatEvent::ToolCall {
            index: c.get("index").and_then(Value::as_u64).map_or(i, |x| x as usize),
            id: s("/id"),
            name: s("/function/name"),
            args: s("/function/arguments"),
        });
    }
}

/// 本机地址（Ollama、LM Studio）不走系统代理，否则开着代理软件时会连不上本地模型
fn is_local(endpoint: &str) -> bool {
    let rest = endpoint.split("://").nth(1).unwrap_or(endpoint);
    if rest.starts_with("[::1]") {
        return true;
    }
    let host = rest.split(['/', ':']).next().unwrap_or("");
    host == "localhost" || host.starts_with("127.")
}

pub fn client(endpoint: &str) -> reqwest::Client {
    let mut b = reqwest::Client::builder().connect_timeout(Duration::from_secs(15));
    if is_local(endpoint) {
        b = b.no_proxy();
    }
    b.build().expect("http client")
}

fn friendly_status(code: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(200).collect());
    let hint = match code {
        401 | 403 => "API Key 不对或没有权限",
        404 => "接口地址或模型名不对",
        429 => "请求太频繁或额度用完了",
        500..=599 => "模型服务那边出错了",
        _ => "请求失败",
    };
    format!("{hint}（{code}）：{detail}")
}

/// 发起流式请求，把事件逐个交给 `emit`。返回前一定会发出 Done 或 Error。
#[cfg(test)]
pub async fn stream_chat(endpoint: &str, api_key: Option<&str>, model: &str, messages: &[Value], emit: impl FnMut(ChatEvent)) {
    stream_chat_with(endpoint, api_key, model, messages, &json!({}), emit).await
}

/// 同上，extra 里的字段（tools、max_tokens 等）合并进请求
pub async fn stream_chat_with(endpoint: &str, api_key: Option<&str>, model: &str, messages: &[Value], extra: &Value, mut emit: impl FnMut(ChatEvent)) {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true}
    });
    for (k, v) in extra.as_object().into_iter().flatten() {
        body[k] = v.clone();
    }
    let mut req = client(endpoint).post(endpoint).json(&body);
    if let Some(k) = api_key {
        req = req.bearer_auth(k);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let msg = if e.is_connect() {
                format!("连不上接口地址：{e}")
            } else if e.is_timeout() {
                "连接超时".to_string()
            } else {
                e.to_string()
            };
            emit(ChatEvent::Error { message: msg });
            return;
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        // 有的服务不认 stream_options，去掉后重试一次
        if status.as_u16() == 400 && text.contains("stream_options") {
            let mut b2 = body.clone();
            b2.as_object_mut().unwrap().remove("stream_options");
            let mut r2 = client(endpoint).post(endpoint).json(&b2);
            if let Some(k) = api_key {
                r2 = r2.bearer_auth(k);
            }
            return match r2.send().await {
                Ok(r) if r.status().is_success() => pump(r, emit).await,
                Ok(r) => {
                    let c = r.status().as_u16();
                    let t = r.text().await.unwrap_or_default();
                    emit(ChatEvent::Error { message: friendly_status(c, &t) })
                }
                Err(e) => emit(ChatEvent::Error { message: e.to_string() }),
            };
        }
        emit(ChatEvent::Error { message: friendly_status(status.as_u16(), &text) });
        return;
    }
    pump(resp, emit).await
}

async fn pump(resp: reqwest::Response, mut emit: impl FnMut(ChatEvent)) {
    // 不是 SSE（服务忽略了 stream:true）就按普通 JSON 读
    let is_sse = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("event-stream"))
        .unwrap_or(true);
    if !is_sse {
        match resp.json::<Value>().await {
            Ok(v) => {
                if let Some(t) = v.pointer("/choices/0/message/content").and_then(Value::as_str) {
                    emit(ChatEvent::Delta { text: t.to_string() });
                }
                let mut calls = vec![];
                if let Some(m) = v.pointer("/choices/0/message") {
                    push_tool_calls(m, &mut calls);
                }
                calls.into_iter().for_each(&mut emit);
                if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                    emit(ChatEvent::Usage {
                        prompt: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
                        completion: u.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
                    });
                }
                emit(ChatEvent::Done);
            }
            Err(e) => emit(ChatEvent::Error { message: format!("读不懂返回内容：{e}") }),
        }
        return;
    }
    let mut parser = SseParser::default();
    let mut stream = resp.bytes_stream();
    let idle = Duration::from_secs(90);
    loop {
        match tokio::time::timeout(idle, stream.next()).await {
            Err(_) => {
                emit(ChatEvent::Error { message: "模型 90 秒没有回应，已停止".into() });
                return;
            }
            Ok(None) => break,
            Ok(Some(Err(e))) => {
                emit(ChatEvent::Error { message: format!("连接中断：{e}") });
                return;
            }
            Ok(Some(Ok(bytes))) => {
                for ev in parser.feed(&bytes) {
                    let is_err = matches!(ev, ChatEvent::Error { .. });
                    emit(ev);
                    if is_err {
                        return;
                    }
                }
                if parser.finished {
                    break;
                }
            }
        }
    }
    for ev in parser.flush() {
        emit(ev);
    }
    emit(ChatEvent::Done);
}

/// 一轮回复：正文、要调用的工具、用了多少 token
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Turn {
    pub content: String,
    pub calls: Vec<Call>,
    pub tokens: u64,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub args: String,
}

/// 请求一轮（可带工具）。正文和思考过程照常通过 emit 流给界面；工具调用拼好后放在返回值里
pub async fn turn(endpoint: &str, api_key: Option<&str>, model: &str, messages: &[Value], extra: &Value, mut emit: impl FnMut(ChatEvent)) -> Result<Turn, String> {
    let mut t = Turn::default();
    let mut err = None;
    stream_chat_with(endpoint, api_key, model, messages, extra, |ev| match ev {
        ChatEvent::Delta { ref text } => {
            t.content.push_str(text);
            emit(ev);
        }
        ChatEvent::Reasoning { .. } => emit(ev),
        ChatEvent::Usage { prompt, completion } => t.tokens = prompt + completion,
        ChatEvent::ToolCall { index, id, name, args } => {
            while t.calls.len() <= index {
                t.calls.push(Call::default());
            }
            let c = &mut t.calls[index];
            if !id.is_empty() {
                c.id = id;
            }
            c.name.push_str(&name);
            c.args.push_str(&args);
        }
        ChatEvent::Error { message } => err = Some(message),
        _ => {}
    })
    .await;
    if let Some(e) = err {
        return Err(e);
    }
    t.calls.retain(|c| !c.name.is_empty());
    for (i, c) in t.calls.iter_mut().enumerate() {
        if c.id.is_empty() {
            c.id = format!("call_{i}");
        }
    }
    Ok(t)
}

/// 设置页「获取模型列表」：GET {接口地址}/models
pub async fn list_models(base: &str, api_key: Option<&str>) -> Result<Vec<String>, String> {
    let url = format!("{base}/models");
    let mut req = client(&url).get(&url).timeout(Duration::from_secs(20));
    if let Some(k) = api_key {
        req = req.bearer_auth(k);
    }
    let r = req.send().await.map_err(|e| format!("连不上：{e}"))?;
    let code = r.status().as_u16();
    let text = r.text().await.unwrap_or_default();
    if !(200..300).contains(&code) {
        return Err(friendly_status(code, &text));
    }
    parse_model_list(&text)
}

/// OpenAI 兼容接口返回 {"data":[{"id":…}]}；个别服务用 {"models":[{"name":…}]}
fn parse_model_list(text: &str) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "读不懂接口返回的内容".to_string())?;
    let list = v.get("data").or_else(|| v.get("models")).and_then(Value::as_array).ok_or("接口返回的内容里没有模型列表")?;
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|m| m.get("id").or_else(|| m.get("name")).and_then(Value::as_str).map(str::to_string))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// 设置页「测试连接」：发一句极短的非流式请求
pub async fn test_connection(endpoint: &str, api_key: Option<&str>, model: &str) -> Result<String, String> {
    let body = json!({"model": model, "messages": [{"role":"user","content":"ping"}], "max_tokens": 5, "stream": false});
    let mut req = client(endpoint).post(endpoint).json(&body).timeout(Duration::from_secs(30));
    if let Some(k) = api_key {
        req = req.bearer_auth(k);
    }
    let t0 = std::time::Instant::now();
    let r = req.send().await.map_err(|e| format!("连不上：{e}"))?;
    let code = r.status().as_u16();
    let text = r.text().await.unwrap_or_default();
    if (200..300).contains(&code) {
        Ok(format!("连接成功，用时 {} 毫秒", t0.elapsed().as_millis()))
    } else {
        Err(friendly_status(code, &text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_lists() {
        let ids = parse_model_list(r#"{"object":"list","data":[{"id":"gpt-4o"},{"id":"deepseek-chat"},{"id":"gpt-4o"},{"object":"x"}]}"#).unwrap();
        assert_eq!(ids, vec!["deepseek-chat", "gpt-4o"]);
        assert_eq!(parse_model_list(r#"{"models":[{"name":"qwen2.5:7b"}]}"#).unwrap(), vec!["qwen2.5:7b"]);
        assert!(parse_model_list("<html>").is_err());
        assert!(parse_model_list(r#"{"error":"x"}"#).is_err());
    }

    #[test]
    fn parses_split_chunks_and_utf8() {
        let raw = "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
        let bytes = raw.as_bytes();
        // 每 3 个字节切一刀，会切到汉字中间
        let mut p = SseParser::default();
        let mut evs = vec![];
        for c in bytes.chunks(3) {
            evs.extend(p.feed(c));
        }
        assert_eq!(
            evs,
            vec![
                ChatEvent::Delta { text: "你好".into() },
                ChatEvent::Reasoning { text: "想".into() },
                ChatEvent::Usage { prompt: 3, completion: 2 },
            ]
        );
        assert!(p.finished);
    }

    #[test]
    fn error_payload_and_crlf() {
        let mut p = SseParser::default();
        let evs = p.feed(b"data: {\"error\":{\"message\":\"bad key\"}}\r\n");
        assert_eq!(evs, vec![ChatEvent::Error { message: "bad key".into() }]);
    }

    #[test]
    fn last_line_without_newline() {
        let mut p = SseParser::default();
        assert!(p.feed(b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}").is_empty());
        assert_eq!(p.flush(), vec![ChatEvent::Delta { text: "x".into() }]);
    }

    #[test]
    fn local_detection() {
        assert!(is_local("http://localhost:11434/v1/chat/completions"));
        assert!(is_local("http://127.0.0.1:1234/v1"));
        assert!(is_local("http://[::1]:8080/v1"));
        assert!(!is_local("https://api.openai.com/v1"));
        assert!(!is_local("https://localhost.evil.com/v1"));
    }

    #[test]
    fn friendly_errors() {
        let m = friendly_status(401, r#"{"error":{"message":"Incorrect API key"}}"#);
        assert!(m.contains("API Key") && m.contains("Incorrect API key"));
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// 起一个只回一次的本地 HTTP 服务，返回地址和收到的请求
    fn serve(status: &str, ctype: &str, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        let status = status.to_string();
        let ctype = ctype.to_string();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let mut req = Vec::new();
            loop {
                let n = s.read(&mut buf).unwrap();
                req.extend_from_slice(&buf[..n]);
                let txt = String::from_utf8_lossy(&req).to_string();
                if let Some(i) = txt.find("\r\n\r\n") {
                    let len = txt.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    if req.len() >= i + 4 + len { break; }
                }
                if n == 0 { break; }
            }
            // 分几段写出去，模拟网络分包
            let head = format!("HTTP/1.1 {status}\r\ncontent-type: {ctype}\r\nconnection: close\r\n\r\n");
            s.write_all(head.as_bytes()).unwrap();
            for c in body.as_bytes().chunks(7) {
                s.write_all(c).unwrap();
                s.flush().unwrap();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            String::from_utf8_lossy(&req).to_string()
        });
        (addr, h)
    }

    #[tokio::test]
    async fn streams_from_server() {
        let (addr, h) = serve(
            "200 OK",
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\n\ndata: [DONE]\n\n",
        );
        let mut evs = vec![];
        stream_chat(&format!("{addr}/v1/chat/completions"), Some("sk-x"), "m", &[json!({"role":"user","content":"hi"})], |e| evs.push(e)).await;
        let req = h.join().unwrap();
        assert!(req.contains("authorization: Bearer sk-x") || req.contains("Authorization: Bearer sk-x"));
        assert!(req.contains("\"stream\":true"));
        assert_eq!(evs, vec![ChatEvent::Delta { text: "你".into() }, ChatEvent::Delta { text: "好".into() }, ChatEvent::Done]);
    }

    #[tokio::test]
    async fn reports_http_errors() {
        let (addr, h) = serve("401 Unauthorized", "application/json", "{\"error\":{\"message\":\"Incorrect API key\"}}");
        let mut evs = vec![];
        stream_chat(&format!("{addr}/v1/chat/completions"), Some("bad"), "m", &[], |e| evs.push(e)).await;
        h.join().unwrap();
        match &evs[..] {
            [ChatEvent::Error { message }] => assert!(message.contains("API Key") && message.contains("401")),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn non_streaming_fallback() {
        let (addr, h) = serve("200 OK", "application/json", "{\"choices\":[{\"message\":{\"content\":\"整段\"}}]}");
        let mut evs = vec![];
        stream_chat(&format!("{addr}/v1/chat/completions"), None, "m", &[], |e| evs.push(e)).await;
        let req = h.join().unwrap();
        assert!(!req.to_ascii_lowercase().contains("authorization"));
        assert_eq!(evs, vec![ChatEvent::Delta { text: "整段".into() }, ChatEvent::Done]);
    }

    #[tokio::test]
    async fn connection_refused() {
        let mut evs = vec![];
        stream_chat("http://127.0.0.1:9/v1/chat/completions", None, "m", &[], |e| evs.push(e)).await;
        assert!(matches!(&evs[..], [ChatEvent::Error { message }] if message.contains("连不上")));
    }

    #[tokio::test]
    async fn test_connection_ok() {
        let (addr, h) = serve("200 OK", "application/json", "{\"choices\":[{\"message\":{\"content\":\"pong\"}}]}");
        let r = test_connection(&format!("{addr}/v1/chat/completions"), Some("k"), "m").await;
        h.join().unwrap();
        assert!(r.unwrap().contains("连接成功"));
    }
}
