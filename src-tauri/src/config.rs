//! 设置：普通项存在应用配置目录的 config.json；API Key 存系统钥匙串
//! （macOS 钥匙串 / Windows 凭据管理器），从不写进文件。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tauri_plugin_global_shortcut::Shortcut;

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
const MAX_SKILLS: usize = 50;
pub const DEFAULT_USD_CNY: f64 = 7.2;
fn default_usd_cny() -> f64 { DEFAULT_USD_CNY }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Skill {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub when: String,
    pub steps: String,
}
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
    pub skills: Vec<String>,
    /// 用哪个模型（模型编号），0 = 跟随默认
    pub model: u32,
    /// 备用模型：主模型出错时换它，0 = 无
    pub fallback: u32,
    /// 自动省钱：简单步骤换便宜 / 本地模型
    pub auto: bool,
}

impl Default for Agent {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), role: String::new(), prompt: String::new(), tint: "#E6D49A".into(), emoji: String::new(), tools: vec!["读文件".into()], skills: vec![], model: 0, fallback: 0, auto: false }
    }
}

impl Agent {
    pub fn lead(prompt: &str) -> Self {
        Self { id: LEAD.into(), name: "主控".into(), role: "拆解与分派".into(), prompt: prompt.into(), tint: "#C9B8F2".into(), tools: vec!["读文件".into(), "写文件".into()], ..Self::default() }
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
/// 交给终端：上次用的工具、终端、项目文件夹（最近 5 个）、要不要存进项目、自定义命令
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Handoff {
    pub tool: String,
    pub term: String,
    pub dirs: Vec<String>,
    /// 同时存到 项目/.floatball/handoff.md（旁边的 .gitignore 让 Git 看不到它）
    pub save: bool,
    /// 自定义命令，{file} = 交接说明文件
    pub custom: String,
    /// 对话像在写代码时，提示交给终端
    pub hint: bool,
}

impl Default for Handoff {
    fn default() -> Self {
        Self { tool: String::new(), term: String::new(), dirs: vec![], save: true, custom: String::new(), hint: true }
    }
}

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
    pub skills: Vec<Skill>,
    #[serde(default = "default_usd_cny")]
    pub usd_cny: f64,
    /// 预算
    pub limits: Limits,
    /// 定时任务（B5）
    pub routines: Vec<crate::routine::Routine>,
    /// 交给终端（B4）：上次的选择
    pub handoff: Handoff,
    /// 权限：总是允许的操作
    pub always_allow: Vec<Allow>,
    /// 离开电脑时暂停敏感操作：确认卡片一直等；关掉则 5 分钟没确认就跳过
    pub pause_away: bool,
    /// 呼出 / 收起悬浮球
    pub shortcut: String,
    /// 截屏提问
    pub capture_shortcut: String,
    pub win_shortcut: String,
    pub share_shortcut: String,
    pub feed_shortcut: String,
    /// 新对话
    pub new_shortcut: String,
    /// 打开任务
    pub tasks_shortcut: String,
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
    /// 悬停快捷入口：指针停在球上时旁边弹出的小泡
    pub hover: Hover,
    /// 划词工具栏
    pub sel: Sel,
    /// 语音
    pub voice: Voice,
    /// 按住说话：RightAlt（默认）或组合键，空 = 关闭
    pub talk_shortcut: String,
    /// 多显示器：小球出现在哪块屏幕。mouse 鼠标所在 / main 固定主屏 / last 上次所在
    pub screen: String,
    /// 跟着鼠标换屏时滑过去而不是闪现
    pub screen_slide: bool,
    /// 每天联网从 models.dev 更新模型资料。默认关：只用安装包自带的那份，不连别的服务器
    pub catalog_auto: bool,
}

/// 悬停快捷入口（原型的 hoverEntries）。order 里 show 的前 4 个按顺序排开
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Hover {
    pub on: bool,
    /// 停多久后弹出（毫秒）：快 200、中 350、慢 600
    pub delay: u32,
    pub order: Vec<HoverItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HoverItem {
    pub id: String,
    pub show: bool,
}

/// 程序里有的入口：截屏提问、新对话、历史记录、任务列表
pub const HOVER_IDS: [&str; 4] = ["shot", "new", "hist", "tasks"];

impl Default for Hover {
    fn default() -> Self {
        Self { on: true, delay: 350, order: HOVER_IDS.iter().map(|id| HoverItem { id: id.to_string(), show: true }).collect() }
    }
}

impl Hover {
    /// 去掉不认识的、重复的，补上缺的；最多显示 4 个
    pub fn normalized(mut self) -> Self {
        let mut seen = vec![];
        self.order.retain(|o| HOVER_IDS.contains(&o.id.as_str()) && !seen.contains(&o.id) && { seen.push(o.id.clone()); true });
        for id in HOVER_IDS {
            if !self.order.iter().any(|o| o.id == id) {
                self.order.push(HoverItem { id: id.into(), show: false });
            }
        }
        let mut n = 0;
        for o in &mut self.order {
            if o.show { n += 1; if n > 4 { o.show = false; } }
        }
        self.delay = self.delay.clamp(100, 1500);
        self
    }
}

/// 语音（原型的 vcCfg）。识别用模型库里某个模型的接口地址和 Key，调它的 /audio/transcriptions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Voice {
    /// 识别语言：auto、zh（普通话）、en、yue（粤语）
    pub lang: String,
    /// 松开后：send 直接发送 / box 先放进输入框
    pub after: String,
    /// 朗读回复
    pub tts: bool,
    /// 用模型库里哪个模型的接口和 Key（编号，0 = 默认模型）
    pub model: u32,
    /// 识别模型名，例如 whisper-1、gpt-4o-mini-transcribe、whisper-large-v3、FunAudioLLM/SenseVoiceSmall
    pub stt: String,
    /// 识别引擎：system 系统自带（默认，免费）/ provider 用上面选的模型服务商
    pub engine: String,
}

impl Default for Voice {
    fn default() -> Self {
        Self { lang: "auto".into(), after: "send".into(), tts: false, model: 0, stt: "whisper-1".into(), engine: "system".into() }
    }
}

impl Voice {
    pub fn normalized(mut self) -> Self {
        if !["auto", "zh", "en", "yue"].contains(&self.lang.as_str()) { self.lang = "auto".into(); }
        if self.after != "box" { self.after = "send".into(); }
        self.stt = self.stt.trim().to_string();
        if self.stt.is_empty() { self.stt = "whisper-1".into(); }
        if self.engine != "provider" { self.engine = "system".into(); }
        self
    }
}

/// 「按住说话」的默认键：单独按住右 Alt（右 ⌥）。不是组合键，全局快捷键注册不了，由划词的后台线程读按键状态
pub const TALK_RALT: &str = "RightAlt";

/// 划词工具栏（原型的 selCfg）。order 里勾了「显示」的前 6 个放在工具栏上，其余放进「⋯」
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Sel {
    pub on: bool,
    /// instant：选中后立即出现；alt：按住 Alt（⌥）再选中
    pub mode: String,
    pub order: Vec<HoverItem>,
    /// 翻译成：auto（中↔英）、en、zh、ja
    pub tr: String,
    /// 不在这些应用里出现（按应用名或程序文件名匹配，不分大小写）
    pub apps: Vec<String>,
}

pub const SEL_IDS: [&str; 10] = ["ask", "explain", "tr", "sum", "rew", "copy", "read", "search", "expand", "shrink"];
pub const SEL_MAX: usize = 6;

impl Default for Sel {
    fn default() -> Self {
        let apps: &[&str] = if cfg!(target_os = "macos") { &["1Password", "钥匙串访问", "终端"] } else { &["1Password", "KeePass", "Bitwarden"] };
        Self {
            // 默认关：Windows 上读选中文字要模拟 Ctrl+C，会动剪贴板，由用户自己在设置里打开
            on: false,
            mode: "instant".into(),
            order: SEL_IDS.iter().enumerate().map(|(i, id)| HoverItem { id: id.to_string(), show: i < SEL_MAX }).collect(),
            tr: "auto".into(),
            apps: apps.iter().map(|a| a.to_string()).collect(),
        }
    }
}

impl Sel {
    pub fn normalized(mut self) -> Self {
        let mut seen = vec![];
        self.order.retain(|o| SEL_IDS.contains(&o.id.as_str()) && !seen.contains(&o.id) && { seen.push(o.id.clone()); true });
        for id in SEL_IDS {
            if !self.order.iter().any(|o| o.id == id) {
                self.order.push(HoverItem { id: id.into(), show: false });
            }
        }
        if self.mode != "alt" { self.mode = "instant".into(); }
        if !["auto", "en", "zh", "ja"].contains(&self.tr.as_str()) { self.tr = "auto".into(); }
        let mut seen: Vec<String> = vec![];
        self.apps = self.apps.into_iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty() && !seen.contains(&a.to_lowercase()) && { seen.push(a.to_lowercase()); true }).collect();
        self
    }

    /// 前台应用是否在排除列表里：应用名或程序文件名（不带 .exe）包含列表里的某一项
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    pub fn excluded(&self, app_name: &str, exe: &str) -> bool {
        let (n, e) = (app_name.to_lowercase(), exe.to_lowercase());
        let e = e.trim_end_matches(".exe");
        self.apps.iter().any(|a| { let a = a.to_lowercase(); n.contains(&a) || (!e.is_empty() && e.contains(&a.replace(' ', ""))) })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            models: vec![],
            default_model: 0,
            system_prompt: String::new(),
            agents: default_agents(),
            skills: vec![],
            usd_cny: DEFAULT_USD_CNY,
            limits: Limits::default(),
            routines: vec![],
            handoff: Handoff::default(),
            always_allow: vec![],
            pause_away: true,
            shortcut: "Alt+Space".into(),
            capture_shortcut: "Alt+S".into(),
            win_shortcut: "Alt+W".into(),
            share_shortcut: "Alt+Shift+S".into(),
            feed_shortcut: "Alt+C".into(),
            new_shortcut: "Alt+N".into(),
            tasks_shortcut: "Alt+T".into(),
            position: None,
            orb_size: 72,
            glass: 0,
            eye: "#FFF1DC".into(),
            idle_ms: 10 * 60 * 1000,
            follow_system: false,
            auto_dock: true,
            calm: false,
            hover: Hover::default(),
            sel: Sel::default(),
            voice: Voice::default(),
            talk_shortcut: TALK_RALT.into(),
            screen: "mouse".into(),
            screen_slide: true,
            catalog_auto: false,
        }
    }
}

/// 第一次用时的 4 个 Agent（照原型）：主控 + 研究员 / 整理员 / 写手。
/// 研究员默认只带「读文件」：搜索网页默认不带（J 定的），要联网时在设置里给它勾上。
pub fn default_agents() -> Vec<Agent> {
    let ag = |id: &str, name: &str, role: &str, tint: &str, tools: &[&str], prompt: &str| Agent {
        id: id.into(), name: name.into(), role: role.into(), tint: tint.into(), prompt: prompt.into(),
        tools: tools.iter().map(|t| t.to_string()).collect(), ..Agent::default()
    };
    vec![
        Agent::lead(DEFAULT_PROMPT),
        ag("research", "研究员", "搜索与阅读", "#A9BEDB", &["读文件"], "你是研究员。按要求查找、阅读资料，给出要点和出处；不确定的标明不确定。"),
        ag("organize", "整理员", "文件与表格", "#AECDB6", &["读文件", "写文件"], "你是整理员。把资料整理成清楚的表格、清单或文件；保持数据原样，不编造。"),
        ag("writer", "写手", "起草与润色", "#DCC0A8", &["读文件", "写文件"], "你是写手。根据给你的材料起草或润色文字，结构清楚、语气自然，默认用中文。"),
    ]
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
        c.clear_conflicting_shortcut_defaults(&v);
        c.fix_agents(if v.get("agents").is_none() { v.get("system_prompt").and_then(Value::as_str) } else { None });
        c
    }

    pub fn shortcut_bindings(&self) -> [(&str, &'static str); 8] {
        [(&self.shortcut, "toggle"), (&self.new_shortcut, "new_chat"), (&self.capture_shortcut, "capture"),
         (&self.tasks_shortcut, "tasks"), (&self.talk_shortcut, "talk"), (&self.win_shortcut, "capture_window"),
         (&self.share_shortcut, "share"), (&self.feed_shortcut, "feed")]
    }

    fn clear_conflicting_shortcut_defaults(&mut self, raw: &Value) {
        let conflicts = |action, key: &str| {
            let Ok(key) = key.parse::<Shortcut>() else { return false };
            self.shortcut_bindings().iter().any(|(other, name)| *name != action && other.trim().parse::<Shortcut>().ok() == Some(key))
        };
        let window = raw.get("win_shortcut").is_none() && conflicts("capture_window", &self.win_shortcut);
        let share = raw.get("share_shortcut").is_none() && conflicts("share", &self.share_shortcut);
        let feed = raw.get("feed_shortcut").is_none() && conflicts("feed", &self.feed_shortcut);
        if window { self.win_shortcut.clear(); }
        if share { self.share_shortcut.clear(); }
        if feed { self.feed_shortcut.clear(); }
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

    pub fn save_skill(&mut self, mut skill: Skill) -> Result<(), String> {
        skill.name = skill.name.trim().to_string();
        skill.when = skill.when.trim().to_string();
        skill.steps = skill.steps.trim().to_string();
        if skill.name.is_empty() || skill.steps.is_empty() { return Err("请填写技能名字和步骤".into()); }
        if skill.name.chars().count() > 60 || skill.when.chars().count() > 500 || skill.steps.chars().count() > 6000 {
            return Err("技能内容超过长度限制".into());
        }
        if skill.id.is_empty() {
            if self.skills.len() >= MAX_SKILLS { return Err("技能库最多保存 50 个技能".into()); }
            skill.id = (1..).map(|n| format!("sk{n}")).find(|id| !self.skills.iter().any(|s| &s.id == id)).unwrap();
            self.skills.push(skill);
        } else {
            let saved = self.skills.iter_mut().find(|s| s.id == skill.id).ok_or("这个技能已被删除")?;
            *saved = skill;
        }
        Ok(())
    }

    pub fn remove_skill(&mut self, id: &str) {
        self.skills.retain(|s| s.id != id);
        for agent in &mut self.agents { agent.skills.retain(|s| s != id); }
    }

    pub fn agent_prompt(&self, agent: &Agent) -> String {
        let mut prompt = agent.prompt.clone();
        for skill in self.skills.iter().filter(|s| agent.skills.contains(&s.id)) {
            prompt.push_str(&format!("\n\n技能：{}\n什么时候用：{}\n步骤：\n{}", skill.name, skill.when, skill.steps));
        }
        prompt
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
        let mut assigned = std::collections::HashSet::new();
        a.skills.retain(|id| self.skills.iter().any(|s| &s.id == id) && assigned.insert(id.clone()));
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
        a.id = format!("ag-{}", uuid::Uuid::new_v4());
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

    /// 自动省钱时优先使用可以调用工具的免 Key 模型。
    pub fn effective_model_for(&self, agent: &Agent) -> Option<&Model> {
        if agent.auto {
            if let Some(model) = self.models.iter().find(|m| m.no_key && m.tools != Some(false)) {
                return Some(model);
            }
        }
        self.model_for(agent)
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
    fn hover_normalizes() {
        let h = Hover { on: true, delay: 9, order: vec![HoverItem { id: "hist".into(), show: true }, HoverItem { id: "zzz".into(), show: true }, HoverItem { id: "hist".into(), show: false }] }.normalized();
        assert_eq!(h.order.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["hist", "shot", "new", "tasks"]);
        assert_eq!(h.order.iter().filter(|o| o.show).count(), 1);
        assert_eq!(h.delay, 100);
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
        c.hover.order.swap(0, 2);
        c.hover.order[1].show = false;
        c.save(d.path()).unwrap();
        assert_eq!(Config::load(d.path()), c);
    }

    #[test]
    fn voice_normalizes() {
        let v = Voice { lang: "fr".into(), after: "x".into(), tts: true, model: 3, stt: "  ".into(), engine: "x".into() }.normalized();
        assert_eq!(v, Voice { lang: "auto".into(), after: "send".into(), tts: true, model: 3, stt: "whisper-1".into(), engine: "system".into() });
        assert_eq!(Voice { engine: "provider".into(), ..Voice::default() }.normalized().engine, "provider");
        // 默认用系统自带识别
        assert_eq!(Voice::default().engine, "system");
        let v = Voice { lang: "yue".into(), after: "box".into(), stt: " whisper-large-v3 ".into(), ..Voice::default() }.normalized();
        assert_eq!((v.lang.as_str(), v.after.as_str(), v.stt.as_str()), ("yue", "box", "whisper-large-v3"));
        // 旧配置里没有 voice、talk_shortcut：用默认值
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.voice, Voice::default());
        assert_eq!(c.talk_shortcut, TALK_RALT);
    }

    #[test]
    fn sel_normalizes_and_matches_apps() {
        let s = Sel { on: true, mode: "x".into(), tr: "fr".into(), apps: vec![" 1Password ".into(), "1password".into(), "".into(), "Windows Terminal".into()],
            order: vec![HoverItem { id: "copy".into(), show: true }, HoverItem { id: "bad".into(), show: true }, HoverItem { id: "copy".into(), show: false }] }.normalized();
        assert_eq!((s.mode.as_str(), s.tr.as_str()), ("instant", "auto"));
        assert_eq!(s.apps, vec!["1Password", "Windows Terminal"]);
        assert_eq!(s.order.len(), 10);
        assert_eq!((s.order[0].id.as_str(), s.order[0].show, s.order[1].show), ("copy", true, false));
        assert!(s.excluded("1Password 8", "1Password.exe"));
        assert!(s.excluded("Terminal", "WindowsTerminal.exe"));
        assert!(!s.excluded("Microsoft Edge", "msedge.exe"));
        let d = Sel::default();
        assert_eq!(d.order.iter().filter(|o| o.show).count(), SEL_MAX);
        // 划词默认关（会动剪贴板，由用户自己打开）；新配置也一样
        assert!(!d.on);
        assert!(!Config::default().sel.on);
    }

    #[test]
    fn legacy_handoff_without_hint_defaults_on() {
        let old = r#"{"handoff":{"tool":"codex","term":"powershell","dirs":["C:/work"],"save":false,"custom":"my-agent {file}"}}"#;
        let c: Config = serde_json::from_str(old).unwrap();
        assert!(c.handoff.hint);
        assert_eq!((c.handoff.tool.as_str(), c.handoff.term.as_str(), c.handoff.custom.as_str()), ("codex", "powershell", "my-agent {file}"));
        assert_eq!(c.handoff.dirs, vec!["C:/work"]);
        assert!(!c.handoff.save);
        let mut h = c.handoff;
        h.hint = false;
        let roundtrip: Handoff = serde_json::from_str(&serde_json::to_string(&h).unwrap()).unwrap();
        assert_eq!(roundtrip, h);
        assert!(!roundtrip.hint);
    }

    #[test]
    fn old_config_uses_default_exchange_rate() {
        let c: Config = serde_json::from_str(r#"{"capture_shortcut":"Alt+S"}"#).unwrap();
        assert_eq!(c.usd_cny, 7.2);
        let c: Config = serde_json::from_str(r#"{"usd_cny":8.4}"#).unwrap();
        assert_eq!(c.usd_cny, 8.4);
    }

    #[test]
    fn old_config_without_skills_loads_empty_lists() {
        let c: Config = serde_json::from_str(r#"{"agents":[{"id":"lead","name":"主控"},{"id":"research","name":"研究员"}]}"#).unwrap();
        assert!(c.skills.is_empty());
        assert!(c.agents.iter().all(|a| a.skills.is_empty()));
    }

    #[test]
    fn skill_limit_assignment_and_deletion() {
        let mut c = Config::default();
        let skill = Skill { id: String::new(), name: "核对出处".into(), when: "查资料时".into(), steps: "先找原文".into() };
        for _ in 0..MAX_SKILLS { c.save_skill(skill.clone()).unwrap(); }
        assert!(c.save_skill(skill).is_err());
        let id = c.skills[0].id.clone();
        let mut researcher = c.agents[1].clone();
        researcher.skills = vec![id.clone(), id.clone(), "missing".into()];
        c.upsert_agent(researcher).unwrap();
        assert_eq!(c.agents[1].skills, [id.clone()]);
        assert!(c.agent_prompt(&c.agents[1]).ends_with("技能：核对出处\n什么时候用：查资料时\n步骤：\n先找原文"));
        assert_eq!(c.agent_prompt(c.lead()), c.lead().prompt);
        c.remove_skill(&id);
        assert!(c.agents[1].skills.is_empty());
        assert_eq!(c.agent_prompt(&c.agents[1]), c.agents[1].prompt);
    }

    #[test]
    fn new_capture_shortcuts_have_defaults() {
        let d = tempfile::tempdir().unwrap();
        let c = Config::load(d.path());
        assert_eq!((c.win_shortcut.as_str(), c.share_shortcut.as_str(), c.feed_shortcut.as_str()), ("Alt+W", "Alt+Shift+S", "Alt+C"));
        fs::write(Config::path(d.path()), r#"{"capture_shortcut":"Alt+S"}"#).unwrap();
        let c = Config::load(d.path());
        assert_eq!((c.win_shortcut.as_str(), c.share_shortcut.as_str(), c.feed_shortcut.as_str()), ("Alt+W", "Alt+Shift+S", "Alt+C"));
    }

    #[test]
    fn feed_shortcut_only_clears_missing_default_on_conflict() {
        let d = tempfile::tempdir().unwrap();
        let path = Config::path(d.path());
        fs::write(&path, r#"{"win_shortcut":"Alt+C"}"#).unwrap();
        let c = Config::load(d.path());
        assert_eq!(c.win_shortcut, "Alt+C");
        assert!(c.feed_shortcut.is_empty());
        fs::write(&path, r#"{"win_shortcut":"Alt+C","feed_shortcut":"Alt+C"}"#).unwrap();
        assert_eq!(Config::load(d.path()).feed_shortcut, "Alt+C");
    }

    #[test]
    fn capture_shortcut_upgrade_preserves_existing_bindings() {
        let d = tempfile::tempdir().unwrap();
        let path = Config::path(d.path());
        fs::write(&path, r#"{"capture_shortcut":"Alt+Shift+S"}"#).unwrap();
        let c = Config::load(d.path());
        assert_eq!((c.capture_shortcut.as_str(), c.win_shortcut.as_str(), c.share_shortcut.as_str()), ("Alt+Shift+S", "Alt+W", ""));
        let mut parsed = Vec::new();
        for (key, _) in c.shortcut_bindings() {
            if let Ok(key) = key.parse::<Shortcut>() {
                assert!(!parsed.contains(&key));
                parsed.push(key);
            }
        }
        fs::write(&path, r#"{"new_shortcut":"Alt+W","capture_shortcut":"Alt+S","share_shortcut":"Alt+S"}"#).unwrap();
        let c = Config::load(d.path());
        assert_eq!((c.new_shortcut.as_str(), c.win_shortcut.as_str(), c.share_shortcut.as_str()), ("Alt+W", "", "Alt+S"));
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
        assert_eq!((c.capture_shortcut.as_str(), c.new_shortcut.as_str(), c.tasks_shortcut.as_str()), ("Alt+S", "Alt+N", "Alt+T"));
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
        assert_eq!((c.agents.len(), c.lead().id.as_str(), c.lead().prompt.as_str()), (4, LEAD, "只用中文"));
        // 第一次用：照原型给 4 个 Agent，研究员默认不带搜索网页
        let names: Vec<&str> = c.agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["主控", "研究员", "整理员", "写手"]);
        assert_eq!(c.agents[1].tools, vec!["读文件".to_string()]);
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
        assert_eq!(c.agents.len(), 4);
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
