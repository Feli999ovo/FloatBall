//! 设置：普通项存在应用配置目录的 config.json；API Key 存系统钥匙串
//! （macOS 钥匙串 / Windows 凭据管理器），从不写进文件。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "com.floatball.desktop";
/// 模型库最多放这么多个。Windows 卸载程序按编号逐个清 Key（src-tauri/windows/hooks.nsh），改这里要一起改。
pub const MAX_MODELS: u32 = 32;

/// 模型库里的一个模型（OpenAI 兼容接口）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Model {
    /// 编号 1…MAX_MODELS，删掉后可以再用；0 表示还没保存
    pub id: u32,
    /// 显示的名字，空着就显示模型名
    pub name: String,
    /// 接口地址，例如 https://api.openai.com/v1 、https://api.deepseek.com/v1 、http://localhost:11434/v1
    pub base_url: String,
    /// 模型名，例如 gpt-4o-mini、deepseek-chat、qwen2.5:7b
    pub model: String,
    /// 支持图片的模型（截屏提问时会把截图发给它）
    pub vision: bool,
    /// 不需要 API Key（本地 Ollama 等）
    pub no_key: bool,
    /// 设置窗口里选的服务商（OpenAI / DeepSeek / 通义千问 / Kimi / OpenRouter / Ollama（本地）/ 自定义），只用来显示和查能力
    pub provider: String,
    /// 能力：支持工具调用、推理（思考）、上下文长度（token）。None 表示不知道
    pub tools: Option<bool>,
    /// false：不知道支不支持图片（vision 这时按 true 处理，截图照发）
    pub vision_known: bool,
    pub reasoning: Option<bool>,
    pub ctx: Option<u64>,
    /// 能力从哪来：exact（models.dev 里这家服务商的这个模型）、lab（同名模型）、guess（相近的名字推断）、manual（用户点过）、none
    pub caps_src: String,
    /// models.dev 的服务商编号，界面按它找 logo
    pub logo: String,
}

impl Default for Model {
    fn default() -> Self {
        Self { id: 0, name: String::new(), base_url: "https://api.openai.com/v1".into(), model: String::new(), vision: true, no_key: false, provider: String::new(), tools: None, vision_known: true, reasoning: None, ctx: None, caps_src: String::new(), logo: String::new() }
    }
}

impl Model {
    /// 规范化接口地址：去掉结尾的 / 和误填的 /chat/completions
    pub fn base(&self) -> String {
        let b = self.base_url.trim().trim_end_matches('/');
        b.strip_suffix("/chat/completions").unwrap_or(b).to_string()
    }

    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base())
    }
}

/// 工具（Agent 能用哪些）。名字就是界面上显示的字。
pub const TOOLS: [&str; 6] = ["搜索网页", "读文件", "写文件", "删除", "发邮件", "浏览器"];
pub const MAX_AGENTS: usize = 16;
pub const LEAD: &str = "lead";

/// 一个 Agent：主控（id = lead，不能删）或用户建的小伙伴
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// 一句话职责，主控按它决定交给谁
    pub role: String,
    /// 指令（它的系统提示词）
    pub prompt: String,
    /// 头像颜色（#RRGGBB）和头像上的一个字或 emoji
    pub tint: String,
    pub emoji: String,
    /// 能用的工具，取自 TOOLS
    pub tools: Vec<String>,
    /// 用哪个模型（模型编号），0 = 跟随默认
    pub model: u32,
    /// 备用模型：主模型出错时换它，0 = 无
    pub fallback: u32,
    /// 自动省钱：简单步骤换便宜 / 本地模型
    pub auto: bool,
}

impl Default for Agent {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), role: String::new(), prompt: String::new(), tint: "#E6D49A".into(), emoji: String::new(), tools: vec!["读文件".into()], model: 0, fallback: 0, auto: false }
    }
}

impl Agent {
    pub fn lead(prompt: &str) -> Self {
        Self { id: LEAD.into(), name: "主控".into(), role: "拆解与分派".into(), prompt: prompt.into(), tint: "#C9B8F2".into(), tools: vec!["搜索网页".into(), "读文件".into(), "写文件".into()], ..Self::default() }
    }
}

/// 总是允许的一类操作：种类（写入 / 打开网页 / 删除 / 发送）+ 范围（文件夹名、网站、收件人）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Allow {
    pub kind: String,
    pub scope: String,
}

/// 预算：单个任务最多用多少 token（0 不限）、最多多少步、最多同时几个子 Agent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Limits {
    pub tokens: u64,
    pub steps: u32,
    pub parallel: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self { tokens: 50_000, steps: 50, parallel: 3 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// 模型库
    pub models: Vec<Model>,
    /// 提问时用哪个模型（模型的编号）
    pub default_model: u32,
    /// 早期版本的系统提示词。现在是主控的指令（agents[0].prompt），这里只用来迁移
    #[serde(skip_serializing)]
    pub system_prompt: String,
    /// Agents，第一个永远是主控
    pub agents: Vec<Agent>,
    /// 预算
    pub limits: Limits,
    /// 权限：总是允许的操作
    pub always_allow: Vec<Allow>,
    /// 离开电脑时暂停敏感操作：确认卡片一直等；关掉则 5 分钟没确认就跳过
    pub pause_away: bool,
    /// 呼出 / 收起悬浮球
    pub shortcut: String,
    /// 截屏提问
    pub capture_shortcut: String,
    /// 小球位置（逻辑像素），首次启动为空
    pub position: Option<(f64, f64)>,
    /// 小球直径（逻辑像素）
    pub orb_size: u32,
    /// 卡片的透明度，0 不透，最大 60
    pub glass: u8,
    /// 眼睛颜色（#RRGGBB）
    pub eye: String,
    /// 空闲多久后睡觉（毫秒），0 = 从不
    pub idle_ms: u64,
    /// 跟随系统深浅色：系统为深色时卡片玻璃更深
    pub follow_system: bool,
    /// 拖到屏幕边缘时贴住并压平
    pub auto_dock: bool,
    /// 减少动态效果
    pub calm: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            models: vec![],
            default_model: 0,
            system_prompt: String::new(),
            agents: vec![Agent::lead(DEFAULT_PROMPT)],
            limits: Limits::default(),
            always_allow: vec![],
            pause_away: true,
            shortcut: "Alt+Space".into(),
            capture_shortcut: "Alt+Shift+S".into(),
            position: None,
            orb_size: 72,
            glass: 25,
            eye: "#FFF1DC".into(),
            idle_ms: 10 * 60 * 1000,
            follow_system: false,
            auto_dock: true,
            calm: false,
        }
    }
}

pub const DEFAULT_PROMPT: &str = "你是住在用户桌面上的悬浮球助手。回答简洁、具体，默认用中文。";

impl Config {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("config.json")
    }

    pub fn load(dir: &Path) -> Config {
        let Some(v) = fs::read_to_string(Self::path(dir)).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
            return Config::default();
        };
        let mut c: Config = serde_json::from_value(v.clone()).unwrap_or_default();
        // 早期版本只有一个模型，字段直接放在最外层
        if v.get("models").is_none() {
            if let Some(model) = v.get("model").and_then(Value::as_str) {
                let d = Model::default();
                c.models = vec![Model {
                    id: 1,
                    name: String::new(),
                    base_url: v.get("base_url").and_then(Value::as_str).unwrap_or(&d.base_url).to_string(),
                    model: model.to_string(),
                    vision: v.get("vision").and_then(Value::as_bool).unwrap_or(true),
                    no_key: v.get("allow_no_key").and_then(Value::as_bool).unwrap_or(false),
                    ..Model::default()
                }];
                c.default_model = 1;
            }
        }
        c.fix_agents(if v.get("agents").is_none() { v.get("system_prompt").and_then(Value::as_str) } else { None });
        c
    }

    /// 主控永远在第一个；早期版本的系统提示词搬进主控的指令；指向已删模型的 Agent 改回跟随默认
    fn fix_agents(&mut self, old_prompt: Option<&str>) {
        if !self.agents.iter().any(|a| a.id == LEAD) {
            self.agents.insert(0, Agent::lead(old_prompt.unwrap_or(DEFAULT_PROMPT)));
        }
        if let Some(i) = self.agents.iter().position(|a| a.id == LEAD) {
            let lead = self.agents.remove(i);
            self.agents.insert(0, lead);
        }
        if let Some(p) = old_prompt {
            self.agents[0].prompt = p.to_string();
        }
        self.system_prompt.clear();
        let ids: Vec<u32> = self.models.iter().map(|m| m.id).collect();
        for a in &mut self.agents {
            if !ids.contains(&a.model) { a.model = 0; }
            if !ids.contains(&a.fallback) { a.fallback = 0; }
            a.tools.retain(|t| TOOLS.contains(&t.as_str()));
        }
        if !self.agents[0].tools.iter().any(|t| t == "读文件") {
            self.agents[0].tools.push("读文件".into());
        }
    }

    pub fn lead(&self) -> &Agent {
        &self.agents[0]
    }

    /// 新增（id 为空或不存在）或修改一个 Agent，返回它的 id
    pub fn upsert_agent(&mut self, mut a: Agent) -> Result<String, String> {
        a.name = a.name.trim().chars().take(12).collect();
        if a.name.is_empty() { a.name = "未命名".into(); }
        a.role = a.role.trim().chars().take(40).collect();
        a.prompt = a.prompt.trim().to_string();
        a.emoji = a.emoji.trim().chars().last().map(String::from).unwrap_or_default();
        let mut seen = vec![];
        a.tools.retain(|t| TOOLS.contains(&t.as_str()) && !seen.contains(t) && { seen.push(t.clone()); true });
        if !self.models.iter().any(|m| m.id == a.model) { a.model = 0; }
        if !self.models.iter().any(|m| m.id == a.fallback) || a.fallback == a.model && a.model != 0 { a.fallback = 0; }
        if let Some(old) = self.agents.iter_mut().find(|x| !a.id.is_empty() && x.id == a.id) {
            if a.id == LEAD && !a.tools.iter().any(|t| t == "读文件") { a.tools.push("读文件".into()); }
            *old = a.clone();
            return Ok(a.id);
        }
        if self.agents.len() >= MAX_AGENTS {
            return Err(format!("最多 {MAX_AGENTS} 个 Agent"));
        }
        a.id = (1..).map(|i| format!("ag{i}")).find(|id| !self.agents.iter().any(|x| &x.id == id)).unwrap();
        self.agents.push(a.clone());
        Ok(a.id)
    }

    pub fn remove_agent(&mut self, id: &str) -> Result<(), String> {
        if id == LEAD { return Err("主控不能删".into()); }
        self.agents.retain(|a| a.id != id);
        Ok(())
    }

    /// 这个 Agent 用的模型：它自己选的，没选就用默认
    pub fn model_for(&self, a: &Agent) -> Option<&Model> {
        self.models.iter().find(|m| m.id == a.model).or_else(|| self.current())
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join("config.json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::rename(&tmp, Self::path(dir)).map_err(|e| e.to_string())
    }

    /// 提问时用的模型
    pub fn current(&self) -> Option<&Model> {
        self.models.iter().find(|m| m.id == self.default_model)
    }

    /// 新增（编号为 0 或不存在）或修改一个模型。返回它的编号，以及是不是新增的。
    /// 库里还没有默认模型时，新增的这个就是默认。
    pub fn upsert(&mut self, mut m: Model) -> Result<(u32, bool), String> {
        m.name = m.name.trim().to_string();
        m.base_url = m.base_url.trim().to_string();
        m.model = m.model.trim().to_string();
        if m.base_url.is_empty() || m.model.is_empty() {
            return Err("请填写接口地址和模型名".into());
        }
        let id = m.id;
        if let Some(old) = self.models.iter_mut().find(|x| id != 0 && x.id == id) {
            *old = m;
            return Ok((id, false));
        }
        let id = (1..=MAX_MODELS)
            .find(|i| !self.models.iter().any(|x| x.id == *i))
            .ok_or(format!("最多保存 {MAX_MODELS} 个模型"))?;
        m.id = id;
        self.models.push(m);
        if self.current().is_none() {
            self.default_model = id;
        }
        Ok((id, true))
    }

    /// 删掉一个模型；删的是默认模型时，默认换成剩下的第一个
    pub fn remove(&mut self, id: u32) {
        self.models.retain(|m| m.id != id);
        for a in &mut self.agents {
            if a.model == id { a.model = 0; }
            if a.fallback == id { a.fallback = 0; }
        }
        if self.default_model == id {
            self.default_model = self.models.first().map_or(0, |m| m.id);
        }
    }
}

/// 每个模型的 Key 在钥匙串里的名字。1 号沿用最早的名字，这样早期版本存的 Key 不用搬。
fn key_user(id: u32) -> String {
    if id == 1 { "api_key".into() } else { format!("api_key_{id}") }
}

fn entry(id: u32) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, &key_user(id)).map_err(|e| e.to_string())
}

pub fn get_api_key(id: u32) -> Option<String> {
    entry(id).ok()?.get_password().ok().filter(|s| !s.is_empty())
}

/// key 为空表示删除
pub fn set_api_key(id: u32, key: &str) -> Result<(), String> {
    let e = entry(id)?;
    if key.is_empty() {
        match e.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    } else {
        e.set_password(key).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str) -> Model {
        Model { model: name.into(), ..Model::default() }
    }

    #[test]
    fn roundtrip_and_defaults() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(d.path()), Config::default());
        let mut c = Config::default();
        c.upsert(model("deepseek-chat")).unwrap();
        c.position = Some((10.0, 20.0));
        c.calm = true;
        c.idle_ms = 0;
        c.save(d.path()).unwrap();
        assert_eq!(Config::load(d.path()), c);
    }

    #[test]
    fn reads_single_model_config_from_early_versions() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            Config::path(d.path()),
            r#"{"model":"x","base_url":"http://localhost:11434/v1","allow_no_key":true,"shortcut":"Alt+K","position":[3.0,4.0]}"#,
        )
        .unwrap();
        let c = Config::load(d.path());
        assert_eq!(c.shortcut, "Alt+K");
        assert_eq!(c.capture_shortcut, "Alt+Shift+S");
        assert_eq!(c.position, Some((3.0, 4.0)));
        let m = c.current().unwrap();
        assert_eq!((m.id, m.model.as_str(), m.base_url.as_str(), m.no_key, m.vision), (1, "x", "http://localhost:11434/v1", true, true));
    }

    #[test]
    fn model_library_add_edit_remove() {
        let mut c = Config::default();
        assert!(c.current().is_none());
        assert!(c.upsert(model("  ")).is_err());
        assert_eq!(c.upsert(model("a")).unwrap(), (1, true));
        assert_eq!(c.upsert(model("b")).unwrap(), (2, true));
        assert_eq!(c.current().unwrap().model, "a"); // 第一个自动成为默认
        // 修改不换编号
        assert_eq!(c.upsert(Model { id: 2, ..model(" b2 ") }).unwrap(), (2, false));
        assert_eq!(c.models[1].model, "b2");
        // 删掉默认的，默认换成剩下的；空出来的编号下次再用
        c.remove(1);
        assert_eq!(c.current().unwrap().id, 2);
        assert_eq!(c.upsert(model("c")).unwrap(), (1, true));
        c.remove(1);
        c.remove(2);
        assert!(c.current().is_none());
        // 放满以后不能再加
        for i in 0..MAX_MODELS {
            c.upsert(model(&format!("m{i}"))).unwrap();
        }
        assert!(c.upsert(model("more")).is_err());
        assert_eq!(key_user(1), "api_key");
        assert_eq!(key_user(MAX_MODELS), "api_key_32");
    }

    #[test]
    fn agents_lead_migration_and_crud() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(Config::path(d.path()), r#"{"system_prompt":"只用中文","models":[]}"#).unwrap();
        let mut c = Config::load(d.path());
        assert_eq!((c.agents.len(), c.lead().id.as_str(), c.lead().prompt.as_str()), (1, LEAD, "只用中文"));
        c.save(d.path()).unwrap();
        let raw = std::fs::read_to_string(Config::path(d.path())).unwrap();
        assert!(!raw.contains("system_prompt"));
        assert_eq!(Config::load(d.path()), c);
        // 新建：名字截短、工具去重去掉不认识的、emoji 只留一个字
        let id = c.upsert_agent(Agent { name: " 审查员审查员审查员审查员审查员 ".into(), emoji: "ab".into(), tools: vec!["读文件".into(), "读文件".into(), "飞".into()], model: 9, ..Agent::default() }).unwrap();
        let a = c.agents.iter().find(|a| a.id == id).unwrap();
        assert_eq!((a.name.chars().count(), a.emoji.as_str(), a.tools.len(), a.model), (12, "b", 1, 0));
        // 主控不能删，也不能去掉读文件
        assert!(c.remove_agent(LEAD).is_err());
        c.upsert_agent(Agent { tools: vec![], ..c.lead().clone() }).unwrap();
        assert_eq!(c.lead().tools, vec!["读文件".to_string()]);
        // 模型删了，用它的 Agent 改回跟随默认
        let m = c.upsert(model("x")).unwrap().0;
        let mut a = c.agents[1].clone();
        a.model = m;
        c.upsert_agent(a).unwrap();
        assert_eq!(c.model_for(&c.agents[1]).unwrap().id, m);
        c.remove(m);
        assert_eq!(c.agents[1].model, 0);
        c.remove_agent(&id).unwrap();
        assert_eq!(c.agents.len(), 1);
    }

    #[test]
    fn endpoint_normalizes() {
        let mut m = Model::default();
        m.base_url = "https://api.deepseek.com/v1/".into();
        assert_eq!(m.endpoint(), "https://api.deepseek.com/v1/chat/completions");
        m.base_url = "http://localhost:11434/v1/chat/completions".into();
        assert_eq!(m.endpoint(), "http://localhost:11434/v1/chat/completions");
        assert_eq!(m.base(), "http://localhost:11434/v1");
    }
}
