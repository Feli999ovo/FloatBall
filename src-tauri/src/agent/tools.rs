use super::*;

/// 工具定义（OpenAI function calling）。group 是设置里「能用的工具」那一项
pub(super) struct ToolDef {
    pub(super) name: &'static str,
    pub(super) group: &'static str,
    pub(super) desc: &'static str,
    pub(super) params: fn() -> Value,
}

pub(super) fn obj(props: &[(&str, &str)]) -> Value {
    let mut p = serde_json::Map::new();
    for (k, d) in props {
        p.insert(k.to_string(), json!({"type": "string", "description": d}));
    }
    json!({"type": "object", "properties": p, "required": props.iter().map(|x| x.0).collect::<Vec<_>>()})
}

pub(super) const DEFS: &[ToolDef] = &[
    ToolDef {
        name: "web_search",
        group: "搜索网页",
        desc: "联网搜索，返回前几条结果的标题、链接和摘要",
        params: || obj(&[("query", "搜索词")]),
    },
    ToolDef {
        name: "fetch_page",
        group: "搜索网页",
        desc: "读取一个网页的正文文字",
        params: || obj(&[("url", "网址，http 或 https 开头")]),
    },
    ToolDef {
        name: "read_file",
        group: "读文件",
        desc: "读取本机一个文本文件（最多 200 KB）",
        params: || obj(&[("path", "文件的完整路径")]),
    },
    ToolDef {
        name: "list_dir",
        group: "读文件",
        desc: "列出本机一个文件夹里的文件和子文件夹",
        params: || obj(&[("path", "文件夹的完整路径")]),
    },
    ToolDef {
        name: "write_file",
        group: "写文件",
        desc: "把文字写进本机一个文件（已存在就覆盖）",
        params: || obj(&[("path", "文件的完整路径"), ("content", "要写入的全部内容")]),
    },
    ToolDef {
        name: "delete_file",
        group: "删除",
        desc: "删除本机一个文件（移到悬浮球回收站，可以找回）",
        params: || obj(&[("path", "文件的完整路径")]),
    },
    ToolDef {
        name: "send_email",
        group: "发邮件",
        desc: "在用户的邮件程序里打开一封写好的邮件，用户确认后由用户点发送",
        params: || obj(&[("to", "收件人邮箱"), ("subject", "主题"), ("body", "正文")]),
    },
    ToolDef {
        name: "open_url",
        group: "浏览器",
        desc: "在用户的浏览器里打开一个网页",
        params: || obj(&[("url", "网址，http 或 https 开头")]),
    },
];

/// 风险：0 只读，1 写入/联网（每个范围问一次），2 发送/删除（每次问）
pub(super) fn risk(name: &str) -> u8 {
    match name {
        "write_file" | "open_url" => 1,
        "delete_file" | "send_email" => 2,
        _ => 0,
    }
}

pub(super) fn plan_tool(name: &str) -> bool {
    matches!(
        name,
        "web_search" | "fetch_page" | "read_file" | "list_dir" | "read_skill" | "delegate"
    )
}

pub(super) fn arg(a: &Value, k: &str) -> String {
    a.get(k)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

pub(super) fn short(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() > n {
        c[..n].iter().collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

/// 文件所在文件夹的名字（「总是允许」按它记范围）
pub(super) fn folder(p: &str) -> String {
    Path::new(p)
        .parent()
        .and_then(|d| d.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "根目录".into())
}

pub(super) fn host(u: &str) -> String {
    u.split("://")
        .nth(1)
        .unwrap_or(u)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_string()
}

pub(super) fn http_url(u: &str) -> Result<(), String> {
    if u.starts_with("http://") || u.starts_with("https://") {
        Ok(())
    } else {
        Err("网址要以 http:// 或 https:// 开头".into())
    }
}

/// 确认卡片上的说明：做什么、范围、种类
pub(super) fn describe(name: &str, a: &Value) -> (String, String, String) {
    match name {
        "write_file" => (
            format!("写入文件 {}", arg(a, "path")),
            folder(&arg(a, "path")),
            "写入".into(),
        ),
        "open_url" => (
            format!("在浏览器打开 {}", arg(a, "url")),
            host(&arg(a, "url")),
            "打开网页".into(),
        ),
        "delete_file" => (
            format!("删除 {}", arg(a, "path")),
            folder(&arg(a, "path")),
            "删除".into(),
        ),
        "send_email" => (
            format!("发送邮件给 {}：{}", arg(a, "to"), arg(a, "subject")),
            arg(a, "to"),
            "发送".into(),
        ),
        _ => (name.into(), String::new(), name.into()),
    }
}

/// 药丸上显示的工具名和参数
pub(super) fn label(name: &str, a: &Value) -> (String, String) {
    let (n, v) = match name {
        "web_search" => ("搜索网页", arg(a, "query")),
        "fetch_page" => ("读取网页", host(&arg(a, "url"))),
        "read_file" => ("读取文件", file_name(&arg(a, "path"))),
        "list_dir" => ("查看文件夹", arg(a, "path")),
        "write_file" => ("写入文件", file_name(&arg(a, "path"))),
        "delete_file" => ("删除文件", file_name(&arg(a, "path"))),
        "send_email" => ("发送邮件", arg(a, "to")),
        "open_url" => ("打开网页", host(&arg(a, "url"))),
        _ => (name, String::new()),
    };
    (n.to_string(), short(&v, 40))
}

pub(super) fn file_name(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

impl Task {
    pub(super) fn tools_for(&self, a: &Agent, m: &Model, lead: bool) -> Vec<Value> {
        if m.tools == Some(false) {
            return vec![];
        }
        let mut v: Vec<Value> = DEFS
            .iter()
            .filter(|d| (!self.plan_only || plan_tool(d.name)) && a.tools.iter().any(|t| t == d.group))
            .map(|d| json!({"type": "function", "function": {"name": d.name, "description": d.desc, "parameters": (d.params)()}}))
            .collect();
        if !self.plan_only && self.world.memory_store().is_some() {
            v.push(json!({"type":"function","function":{
                "name":"remember",
                "description":"记住以后对用户有用的事实或偏好。只存一句话，别存密码、密钥或敏感信息。",
                "parameters":obj(&[("text", "简短的事实或偏好")])
            }}));
        }
        if self.cfg.assigned_skills(a).next().is_some() {
            v.push(json!({"type":"function","function":{
                "name":"read_skill",
                "description":"读取一个你会的技能的完整步骤",
                "parameters":obj(&[("name", "技能名字")])
            }}));
        }
        let others: Vec<&Agent> = self.cfg.agents.iter().skip(1).collect();
        if lead && !others.is_empty() {
            let list = others
                .iter()
                .map(|o| {
                    format!(
                        "{}（{}）",
                        o.name,
                        if o.role.is_empty() {
                            "没写职责"
                        } else {
                            &o.role
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("；");
            v.push(json!({"type": "function", "function": {
                "name": "delegate",
                "description": format!("把一个步骤交给别的 Agent 去做，返回它的结果。可以在一轮里同时交给几个。可选：{list}"),
                "parameters": obj(&[("agent", "Agent 的名字"), ("task", "交给它的具体任务，写清楚要什么结果")])
            }}));
        }
        v
    }
}
