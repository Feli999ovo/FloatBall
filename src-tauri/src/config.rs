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
}

impl Default for Model {
    fn default() -> Self {
        Self { id: 0, name: String::new(), base_url: "https://api.openai.com/v1".into(), model: String::new(), vision: true, no_key: false }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// 模型库
    pub models: Vec<Model>,
    /// 提问时用哪个模型（模型的编号）
    pub default_model: u32,
    /// 系统提示词
    pub system_prompt: String,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            models: vec![],
            default_model: 0,
            system_prompt: "你是住在用户桌面上的悬浮球助手。回答简洁、具体，默认用中文。".into(),
            shortcut: "Alt+Space".into(),
            capture_shortcut: "Alt+Shift+S".into(),
            position: None,
            orb_size: 72,
            glass: 25,
            eye: "#FFF1DC".into(),
        }
    }
}

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
                }];
                c.default_model = 1;
            }
        }
        c
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
    fn endpoint_normalizes() {
        let mut m = Model::default();
        m.base_url = "https://api.deepseek.com/v1/".into();
        assert_eq!(m.endpoint(), "https://api.deepseek.com/v1/chat/completions");
        m.base_url = "http://localhost:11434/v1/chat/completions".into();
        assert_eq!(m.endpoint(), "http://localhost:11434/v1/chat/completions");
        assert_eq!(m.base(), "http://localhost:11434/v1");
    }
}
