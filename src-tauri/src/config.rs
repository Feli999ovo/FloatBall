//! 设置：普通项存在应用配置目录的 config.json；API Key 存系统钥匙串
//! （macOS 钥匙串 / Windows 凭据管理器），从不写进文件。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "com.floatball.app";
const KEYRING_USER: &str = "api_key";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// OpenAI 兼容接口地址，例如 https://api.openai.com/v1 、https://api.deepseek.com/v1 、http://localhost:11434/v1
    pub base_url: String,
    /// 模型名，例如 gpt-4o-mini、deepseek-chat、qwen2.5:7b
    pub model: String,
    /// 支持图片的模型（截屏提问时会把截图发给它）
    pub vision: bool,
    /// 系统提示词
    pub system_prompt: String,
    /// 呼出 / 收起悬浮球
    pub shortcut: String,
    /// 截屏提问
    pub capture_shortcut: String,
    /// 小球位置（逻辑像素），首次启动为空
    pub position: Option<(f64, f64)>,
    /// 没有 API Key 也允许（本地 Ollama 等）
    pub allow_no_key: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            vision: true,
            system_prompt: "你是住在用户桌面上的悬浮球助手。回答简洁、具体，默认用中文。".into(),
            shortcut: "Alt+Space".into(),
            capture_shortcut: "Alt+Shift+S".into(),
            position: None,
            allow_no_key: false,
        }
    }
}

impl Config {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("config.json")
    }

    pub fn load(dir: &Path) -> Config {
        fs::read_to_string(Self::path(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join("config.json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::rename(&tmp, Self::path(dir)).map_err(|e| e.to_string())
    }

    /// 规范化接口地址：去掉结尾的 / 和误填的 /chat/completions
    pub fn endpoint(&self) -> String {
        let mut b = self.base_url.trim().trim_end_matches('/').to_string();
        if let Some(s) = b.strip_suffix("/chat/completions") {
            b = s.to_string();
        }
        format!("{b}/chat/completions")
    }
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

pub fn get_api_key() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|s| !s.is_empty())
}

pub fn set_api_key(key: &str) -> Result<(), String> {
    let e = entry()?;
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

    #[test]
    fn roundtrip_and_defaults() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(d.path()), Config::default());
        let mut c = Config::default();
        c.model = "deepseek-chat".into();
        c.position = Some((10.0, 20.0));
        c.save(d.path()).unwrap();
        assert_eq!(Config::load(d.path()), c);
        // 旧版本少字段也能读
        std::fs::write(Config::path(d.path()), r#"{"model":"x"}"#).unwrap();
        let c2 = Config::load(d.path());
        assert_eq!(c2.model, "x");
        assert_eq!(c2.shortcut, "Alt+Space");
    }

    #[test]
    fn endpoint_normalizes() {
        let mut c = Config::default();
        c.base_url = "https://api.deepseek.com/v1/".into();
        assert_eq!(c.endpoint(), "https://api.deepseek.com/v1/chat/completions");
        c.base_url = "http://localhost:11434/v1/chat/completions".into();
        assert_eq!(c.endpoint(), "http://localhost:11434/v1/chat/completions");
    }
}
