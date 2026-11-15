//! 模型资料库：来自 models.dev（MIT，https://models.dev）。
//! 程序里内置一份精简过的快照（data/models-dev.json），联网时每天最多更新一次，存到应用数据目录。
//! 只留设置里要用的几项：工具调用、视觉、推理、上下文长度、显示名。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const BUNDLED: &str = include_str!("../data/models-dev.json");
const API_URL: &str = "https://models.dev/api.json";
const LABS_URL: &str = "https://models.dev/models.json";
/// 多久更新一次（秒）
const REFRESH_EVERY: u64 = 24 * 3600;

/// [工具调用, 视觉, 推理, 上下文 tokens, 显示名]
pub type Row = (bool, bool, bool, u64, String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    pub n: String,
    pub h: String,
    pub m: BTreeMap<String, Row>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    pub v: u32,
    pub fetched: String,
    pub providers: BTreeMap<String, Provider>,
    pub labs: BTreeMap<String, Row>,
}

/// 查到的能力
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Caps {
    pub tools: Option<bool>,
    pub vision: Option<bool>,
    pub reasoning: Option<bool>,
    pub ctx: Option<u64>,
    pub local: bool,
    /// exact：在这家服务商里查到；lab：按模型本身查到；guess：按相近的名字推断；none：没查到
    pub src: &'static str,
    /// 资料里这个模型的名字（没查到为空）
    pub name: String,
    /// 服务商在 models.dev 的 id，用来找 logo（认不出为空）
    pub provider: String,
}

impl Catalog {
    pub fn bundled() -> Catalog {
        serde_json::from_str(BUNDLED).expect("内置模型资料损坏")
    }

    fn cache_path(dir: &Path) -> PathBuf {
        dir.join("models-dev.json")
    }

    /// 先用更新过的缓存，没有或坏了就用内置的
    pub fn load(dir: &Path) -> Catalog {
        std::fs::read_to_string(Self::cache_path(dir))
            .ok()
            .and_then(|s| serde_json::from_str::<Catalog>(&s).ok())
            .filter(|c| c.fetched.as_str() >= bundled_date())
            .unwrap_or_else(Self::bundled)
    }

    /// 缓存是否该更新了（按文件修改时间）
    pub fn stale(dir: &Path) -> bool {
        match std::fs::metadata(Self::cache_path(dir)).and_then(|m| m.modified()) {
            Ok(t) => t.elapsed().map(|e| e.as_secs() > REFRESH_EVERY).unwrap_or(true),
            Err(_) => true,
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join("models-dev.json.tmp");
        std::fs::write(&tmp, serde_json::to_string(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, Self::cache_path(dir)).map_err(|e| e.to_string())
    }

    /// 从 models.dev 的原始数据精简（与 tools/trim_models_dev.py 规则一致）
    pub fn trim(api: &Value, labs: &Value, fetched: &str) -> Catalog {
        let mut providers = BTreeMap::new();
        if let Some(obj) = api.as_object() {
            for (pid, p) in obj {
                let host = p
                    .get("api")
                    .and_then(|a| a.as_str())
                    .and_then(host_of)
                    .unwrap_or_else(|| known_host(pid).to_string());
                let mut m = BTreeMap::new();
                if let Some(ms) = p.get("models").and_then(|x| x.as_object()) {
                    for (mid, mv) in ms {
                        if let Some(r) = row(mv) {
                            m.insert(mid.clone(), r);
                        }
                    }
                }
                let n = p.get("name").and_then(|x| x.as_str()).unwrap_or(pid).to_string();
                providers.insert(pid.clone(), Provider { n, h: host, m });
            }
        }
        let mut lab = BTreeMap::new();
        if let Some(obj) = labs.as_object() {
            for (mid, mv) in obj {
                if let Some(r) = row(mv) {
                    lab.insert(mid.clone(), r);
                }
            }
        }
        Catalog { v: 1, fetched: fetched.into(), providers, labs: lab }
    }

    /// 联网下载并精简。失败就返回错误，调用方继续用旧的。
    pub async fn fetch(today: &str) -> Result<Catalog, String> {
        let c = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let get = |u: &'static str| {
            let c = c.clone();
            async move {
                let r = c.get(u).send().await.map_err(|e| e.to_string())?;
                if !r.status().is_success() {
                    return Err(format!("{u}: {}", r.status()));
                }
                r.json::<Value>().await.map_err(|e| e.to_string())
            }
        };
        let api = get(API_URL).await?;
        let labs = get(LABS_URL).await?;
        let cat = Catalog::trim(&api, &labs, today);
        if cat.providers.len() < 10 || cat.labs.len() < 50 {
            return Err("下载到的模型资料不完整".into());
        }
        Ok(cat)
    }

    /// 按服务商、接口地址、模型名查能力
    pub fn lookup(&self, provider_label: &str, base_url: &str, model: &str) -> Caps {
        let host = host_of(base_url).unwrap_or_default();
        let local = provider_label.starts_with("Ollama") || is_local_host(&host);
        let m = model.trim();
        let ml = m.to_lowercase();
        let mut caps = Caps { tools: None, vision: None, reasoning: None, ctx: None, local, src: "none", name: String::new(), provider: String::new() };

        // 服务商：选了哪家就用哪家；选「自定义」时按接口地址认
        let pid: Option<String> = label_pid(provider_label)
            .map(str::to_string)
            .or_else(|| if local { None } else { self.pid_by_host(&host, &ml) });
        if let Some(p) = &pid {
            caps.provider = p.clone();
        } else if local {
            caps.provider = "ollama-cloud".into();
        }
        if ml.is_empty() {
            return caps;
        }
        let fill = |caps: &mut Caps, r: &Row, src: &'static str| {
            caps.tools = Some(r.0);
            caps.vision = Some(r.1);
            caps.reasoning = Some(r.2);
            caps.ctx = if r.3 > 0 { Some(r.3) } else { None };
            caps.name = r.4.clone();
            caps.src = src;
        };

        // 1. 这家服务商自己的列表
        if let Some(p) = pid.as_ref().and_then(|p| self.providers.get(p)) {
            if let Some(r) = find_ci(&p.m, &ml) {
                fill(&mut caps, r, "exact");
                return caps;
            }
        }
        // 2. 模型本身（带厂商前缀，如 openai/gpt-4o；或去掉前缀按名字找）
        if let Some(r) = find_ci(&self.labs, &ml) {
            fill(&mut caps, r, "lab");
            return caps;
        }
        let tail = ml.rsplit('/').next().unwrap_or(&ml).to_string();
        let prefer = pid.as_deref().map(lab_of).unwrap_or("");
        let mut hits: Vec<(&String, &Row)> = self.labs.iter().filter(|(k, _)| k.to_lowercase().rsplit('/').next() == Some(tail.as_str())).collect();
        hits.sort_by_key(|(k, _)| (!k.starts_with(&format!("{prefer}/")), k.len()));
        if let Some((_, r)) = hits.first() {
            fill(&mut caps, r, "lab");
            return caps;
        }
        // 3. 其他服务商里同名的（中转平台常见）
        if !local {
            for p in self.providers.values() {
                if let Some(r) = find_ci(&p.m, &ml) {
                    fill(&mut caps, r, "exact");
                    return caps;
                }
            }
        }
        // 4. 按相近的名字推断（本地模型常见：llama3.1:8b → meta/llama-3.1-8b-instruct）
        let want = norm(&tail);
        if want.len() >= 4 {
            let mut best: Option<(&String, &Row)> = None;
            for (k, r) in &self.labs {
                let kt = norm(k.rsplit('/').next().unwrap_or(k));
                if kt == want || (kt.starts_with(&want) && !kt[want.len()..].starts_with(|c: char| c.is_ascii_digit())) {
                    if best.map(|(b, _)| kt.len() < norm(b.rsplit('/').next().unwrap_or(b)).len()).unwrap_or(true) {
                        best = Some((k, r));
                    }
                }
            }
            if let Some((_, r)) = best {
                fill(&mut caps, r, "guess");
            }
        }
        caps
    }

    /// 按接口地址认服务商；同一个地址有几家时，选列表里有这个模型的那家
    fn pid_by_host(&self, host: &str, ml: &str) -> Option<String> {
        if host.is_empty() {
            return None;
        }
        let same: Vec<&String> = self.providers.iter().filter(|(_, p)| p.h == host).map(|(k, _)| k).collect();
        same.iter()
            .find(|k| find_ci(&self.providers[k.as_str()].m, ml).is_some())
            .or(same.first())
            .map(|k| k.to_string())
    }
}

/// 内置快照的日期（不解析整份）
fn bundled_date() -> &'static str {
    let i = BUNDLED.find("\"fetched\":\"").map(|i| i + 11).unwrap_or(0);
    BUNDLED.get(i..i + 10).unwrap_or("")
}

/// 原型里的服务商 → models.dev 的 id
fn label_pid(label: &str) -> Option<&'static str> {
    Some(match label {
        "OpenAI" => "openai",
        "DeepSeek" => "deepseek",
        "通义千问" => "alibaba-cn",
        "Kimi" => "moonshotai-cn",
        _ => return None,
    })
}

/// 服务商 id → 模型厂商前缀（模型本身的资料用厂商前缀）
fn lab_of(pid: &str) -> &str {
    match pid {
        "alibaba-cn" | "alibaba" => "alibaba",
        "moonshotai-cn" | "moonshotai" => "moonshotai",
        p => p,
    }
}

fn known_host(pid: &str) -> &'static str {
    match pid {
        "openai" => "api.openai.com",
        "anthropic" => "api.anthropic.com",
        "google" => "generativelanguage.googleapis.com",
        "xai" => "api.x.ai",
        "mistral" => "api.mistral.ai",
        "groq" => "api.groq.com",
        _ => "",
    }
}

fn row(m: &Value) -> Option<Row> {
    let mo = m.get("modalities");
    let has = |side: &str, what: &str| {
        mo.and_then(|x| x.get(side)).and_then(|x| x.as_array()).map(|a| a.iter().any(|v| v == what))
    };
    let out_empty = mo.and_then(|x| x.get("output")).and_then(|x| x.as_array()).map(|a| a.is_empty()).unwrap_or(true);
    if !out_empty && has("output", "text") == Some(false) {
        return None;
    }
    Some((
        m.get("tool_call").and_then(|x| x.as_bool()).unwrap_or(false),
        has("input", "image").unwrap_or(false),
        m.get("reasoning").and_then(|x| x.as_bool()).unwrap_or(false),
        m.get("limit").and_then(|l| l.get("context")).and_then(|x| x.as_u64()).unwrap_or(0),
        m.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    ))
}

fn find_ci<'a>(m: &'a BTreeMap<String, Row>, ml: &str) -> Option<&'a Row> {
    m.get(ml).or_else(|| m.iter().find(|(k, _)| k.to_lowercase() == ml).map(|(_, v)| v))
}

/// 只留字母和数字，去掉 Ollama 的 :latest
fn norm(s: &str) -> String {
    let s = s.to_lowercase();
    let s = s.strip_suffix(":latest").unwrap_or(&s);
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

pub fn host_of(url: &str) -> Option<String> {
    let u = url.trim();
    // 没有协议的（如 models.dev 里 ${ENV}/v1 这种占位写法）不算地址
    let rest = u.split_once("://").map(|(_, r)| r)?;
    let hostport = rest.split(['/', '?', '#']).next()?;
    let hostport = hostport.rsplit('@').next()?;
    let host = if hostport.starts_with('[') {
        hostport.split(']').next()?.trim_start_matches('[').to_string()
    } else {
        hostport.split(':').next()?.to_string()
    };
    if host.is_empty() { None } else { Some(host.to_lowercase()) }
}

pub fn is_local_host(h: &str) -> bool {
    h == "localhost" || h == "127.0.0.1" || h == "::1" || h.ends_with(".local")
}

/// 128000 → 128K，1048576 → 1M
#[cfg_attr(not(test), allow(dead_code))]
pub fn ctx_label(n: u64) -> String {
    if n >= 1_000_000 {
        let m = n as f64 / 1_048_576.0;
        let m2 = n as f64 / 1_000_000.0;
        let v = if (m - m.round()).abs() < (m2 - m2.round()).abs() { m } else { m2 };
        format!("{}M", trim_num(v))
    } else if n >= 1000 {
        let k = n as f64 / 1024.0;
        let k2 = n as f64 / 1000.0;
        let v = if (k - k.round()).abs() < (k2 - k2.round()).abs() { k } else { k2 };
        format!("{}K", trim_num(v))
    } else {
        n.to_string()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn trim_num(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 { format!("{}", v.round() as u64) } else { format!("{:.1}", v) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        Catalog::bundled()
    }

    #[test]
    fn bundled_loads() {
        let c = cat();
        assert!(c.providers.len() > 100 && c.labs.len() > 300);
        assert!(c.providers["openai"].h == "api.openai.com");
    }

    #[test]
    fn exact_provider() {
        let c = cat().lookup("OpenAI", "https://api.openai.com/v1", "gpt-4o-mini");
        assert_eq!(c.src, "exact");
        assert_eq!((c.tools, c.vision, c.reasoning, c.ctx), (Some(true), Some(true), Some(false), Some(128000)));
        assert_eq!(c.provider, "openai");
        assert!(!c.local);
        let c = cat().lookup("通义千问", "https://dashscope.aliyuncs.com/compatible-mode/v1", "qwen-plus");
        assert_eq!((c.src, c.provider.as_str()), ("exact", "alibaba-cn"));
    }

    #[test]
    fn lab_fallback() {
        // DeepSeek 自己的列表里已经没有 deepseek-chat，按模型本身查
        let c = cat().lookup("DeepSeek", "https://api.deepseek.com/v1", "deepseek-chat");
        assert_eq!(c.src, "lab");
        assert_eq!(c.tools, Some(true));
        // 中转平台的 厂商/模型 写法
        let c = cat().lookup("自定义", "https://openrouter.ai/api/v1", "openai/gpt-4o-mini");
        assert_eq!(c.provider, "openrouter");
        assert!(c.src == "exact" || c.src == "lab");
        assert_eq!(c.vision, Some(true));
    }

    #[test]
    fn custom_by_host() {
        let c = cat().lookup("自定义", "https://api.moonshot.cn/v1", "kimi-k2.6");
        assert_eq!((c.provider.as_str(), c.src), ("moonshotai-cn", "exact"));
    }

    #[test]
    fn local_guess_and_unknown() {
        let c = cat().lookup("Ollama（本地）", "http://localhost:11434/v1", "llama3.1:8b");
        assert!(c.local);
        assert_eq!(c.src, "guess");
        assert_eq!(c.tools, Some(true));
        let c = cat().lookup("Ollama（本地）", "http://localhost:11434/v1", "qwen2.5:7b");
        assert_eq!(c.src, "guess");
        // 3.1 不能被当成 3.10
        let c = cat().lookup("自定义", "http://127.0.0.1:8000/v1", "my-finetune-xyz");
        assert!(c.local);
        assert_eq!((c.src, c.tools, c.ctx), ("none", None, None));
        let c = cat().lookup("OpenAI", "https://api.openai.com/v1", "");
        assert_eq!(c.src, "none");
    }

    #[test]
    fn hosts_and_ctx() {
        assert_eq!(host_of("https://API.deepseek.com/v1").as_deref(), Some("api.deepseek.com"));
        assert_eq!(host_of("http://localhost:11434/v1").as_deref(), Some("localhost"));
        assert_eq!(host_of("http://[::1]:8080/v1").as_deref(), Some("::1"));
        assert_eq!(ctx_label(128000), "128K");
        assert_eq!(ctx_label(131072), "128K");
        assert_eq!(ctx_label(64000), "64K");
        assert_eq!(ctx_label(1048576), "1M");
        assert_eq!(ctx_label(163840), "160K");
        assert_eq!(ctx_label(400000), "400K");
    }

    /// 程序里的精简规则与生成内置快照的脚本一致：用同一份原始数据精简，结果应与内置快照相同
    #[test]
    fn trim_matches_bundled() {
        let raw = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/models-dev-raw");
        let (Ok(a), Ok(l)) = (std::fs::read_to_string(raw.join("api.json")), std::fs::read_to_string(raw.join("models.json"))) else {
            eprintln!("跳过：没有原始数据");
            return;
        };
        let t = Catalog::trim(&serde_json::from_str(&a).unwrap(), &serde_json::from_str(&l).unwrap(), &cat().fetched);
        let b = cat();
        for (k, p) in &b.providers {
            assert_eq!(Some(p), t.providers.get(k), "服务商 {k}");
        }
        for (k, r) in &b.labs {
            assert_eq!(Some(r), t.labs.get(k), "模型 {k}");
        }
        assert_eq!(t, b);
    }

    #[test]
    fn cache_roundtrip_and_stale() {
        let d = tempfile::tempdir().unwrap();
        assert!(Catalog::stale(d.path()));
        assert_eq!(Catalog::load(d.path()), cat());
        let mut c = cat();
        c.fetched = "2099-01-01".into();
        c.save(d.path()).unwrap();
        assert!(!Catalog::stale(d.path()));
        assert_eq!(Catalog::load(d.path()).fetched, "2099-01-01");
        // 旧于内置快照的缓存不用
        c.fetched = "2000-01-01".into();
        c.save(d.path()).unwrap();
        assert_eq!(Catalog::load(d.path()).fetched, cat().fetched);
    }
}
