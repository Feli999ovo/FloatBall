use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// 本地假模型：按顺序回 SSE，记下每次收到的请求体
fn server(replies: Vec<String>) -> (String, Arc<Mutex<Vec<Value>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}/v1", l.local_addr().unwrap());
    let got = Arc::new(Mutex::new(vec![]));
    let g = got.clone();
    std::thread::spawn(move || {
        for body in replies {
            let Ok((mut s, _)) = l.accept() else { return };
            let mut req = vec![];
            let mut buf = vec![0u8; 65536];
            loop {
                let n = s.read(&mut buf).unwrap();
                req.extend_from_slice(&buf[..n]);
                let t = String::from_utf8_lossy(&req).to_string();
                if let Some(i) = t.find("\r\n\r\n") {
                    let len = t
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if req.len() >= i + 4 + len {
                        g.lock()
                            .unwrap()
                            .push(serde_json::from_slice(&req[i + 4..]).unwrap_or(Value::Null));
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let head =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(body.as_bytes()).unwrap();
        }
    });
    (addr, got)
}

fn text(t: &str) -> String {
    format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"content":t}}]}),
        json!({"usage":{"prompt_tokens":10,"completion_tokens":5}})
    )
}

/// 工具调用分两段到达（名字和参数拆开），模拟真实的流
fn tool(name: &str, args: Value) -> String {
    let a = args.to_string();
    let (a1, a2) = a.split_at(a.floor_char_boundary(a.len() / 2));
    format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"content":"","tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":name,"arguments":a1}}]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":a2}}]}}]}),
        json!({"usage":{"prompt_tokens":20,"completion_tokens":5}})
    )
}

struct Fake {
    opened: Mutex<Vec<String>>,
    remembered: Mutex<Vec<Allow>>,
    memory: Option<Mutex<crate::store::Store>>,
}
impl MemoryStore for Mutex<crate::store::Store> {
    fn recent(&self, agent: &str) -> Vec<String> {
        self.lock()
            .unwrap()
            .memory(agent, 20)
            .unwrap()
            .into_iter()
            .map(|m| m.text)
            .collect()
    }
    fn add(&self, agent: &str, text: &str) -> Result<i64, String> {
        self.lock()
            .unwrap()
            .remember(agent, text)
            .map_err(|e| e.to_string())
    }
}
impl World for Fake {
    fn open(&self, url: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(url.into());
        Ok(())
    }
    fn search<'a>(
        &'a self,
        q: &'a str,
    ) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("1. 关于{q}的结果\nhttps://example.com")) })
    }
    fn fetch<'a>(
        &'a self,
        _: &'a str,
    ) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("网页正文".into()) })
    }
    fn remember(&self, a: Allow) {
        self.remembered.lock().unwrap().push(a)
    }
    fn memory_store(&self) -> Option<&dyn MemoryStore> {
        self.memory.as_ref().map(|m| m as &dyn MemoryStore)
    }
}

struct Env {
    task: Arc<Task>,
    fake: Arc<Fake>,
    events: Arc<Mutex<Vec<ChatEvent>>>,
    got: Arc<Mutex<Vec<Value>>>,
    dir: tempfile::TempDir,
}

fn env(replies: Vec<String>, f: impl FnOnce(&mut Config)) -> Env {
    env_with_memory(replies, f, None)
}

fn env_with_memory(
    replies: Vec<String>,
    f: impl FnOnce(&mut Config),
    memory: Option<crate::store::Store>,
) -> Env {
    let (base, got) = server(replies);
    let mut cfg = Config::default();
    // 测试从只有主控开始；要子 Agent 的测试自己加
    cfg.agents.truncate(1);
    cfg.upsert(Model {
        model: "m".into(),
        base_url: base,
        no_key: true,
        tools: Some(true),
        ..Model::default()
    })
    .unwrap();
    f(&mut cfg);
    let fake = Arc::new(Fake {
        opened: Mutex::new(vec![]),
        remembered: Mutex::new(vec![]),
        memory: memory.map(Mutex::new),
    });
    let events = Arc::new(Mutex::new(vec![]));
    let ev = events.clone();
    let dir = tempfile::tempdir().unwrap();
    let task = Task::new(
        cfg,
        fake.clone(),
        Arc::new(Asks::default()),
        dir.path().join("回收站"),
        Arc::new(move |e| ev.lock().unwrap().push(e)),
        false,
        vec![],
        Arc::new(crate::changes::Changes::default()),
        dir.path().join("改动备份"),
    );
    Env {
        task,
        fake,
        events,
        got,
        dir,
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn pills(e: &Env) -> Vec<(String, String, String)> {
    e.events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|x| match x {
            ChatEvent::Tool {
                name, arg, status, ..
            } => Some((name.clone(), arg.clone(), status.clone())),
            _ => None,
        })
        .collect()
}

/// 等确认卡片出现，然后回答
async fn answer(e: &Env, a: Answer) -> (String, String, bool) {
    for _ in 0..200 {
        let ask = e.events.lock().unwrap().iter().find_map(|x| match x {
            ChatEvent::Ask {
                ask,
                desc,
                scope,
                hi,
                ..
            } => Some((*ask, desc.clone(), scope.clone(), *hi)),
            _ => None,
        });
        if let Some((id, d, s, hi)) = ask {
            assert!(e.task.asks.answer(id, a));
            return (d, s, hi);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("没有出现确认卡片")
}

mod ask;
mod cost;
mod delegate;
mod exec;
mod memory;
mod tools;
mod world;
