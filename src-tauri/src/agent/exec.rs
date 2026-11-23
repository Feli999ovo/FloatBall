use super::*;

pub(super) fn list_dir(p: &str) -> Result<String, String> {
    let mut v: Vec<String> = std::fs::read_dir(p)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| {
            let dir = e.file_type().is_ok_and(|t| t.is_dir());
            let n = e.file_name().to_string_lossy().to_string();
            if dir {
                n + "/"
            } else {
                format!("{n}（{} 字节）", e.metadata().map_or(0, |m| m.len()))
            }
        })
        .collect();
    v.sort();
    let n = v.len();
    v.truncate(200);
    Ok(if n == 0 {
        "空文件夹".into()
    } else if n > 200 {
        v.join("\n") + &format!("\n…共 {n} 项，只列了前 200 项")
    } else {
        v.join("\n")
    })
}

impl Task {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pill(
        &self,
        call: &str,
        agent: &Agent,
        name: &str,
        arg: &str,
        status: &str,
        result: &str,
        path: Option<String>,
    ) {
        (self.emit)(ChatEvent::Tool {
            call: call.into(),
            agent: agent.name.clone(),
            name: name.into(),
            arg: arg.into(),
            status: status.into(),
            result: short(result, 600),
            path,
            memory_id: None,
            agent_id: None,
        });
    }

    pub(super) fn memory_pill(
        &self,
        call: &str,
        agent: &Agent,
        text: &str,
        status: &str,
        result: &str,
        undo_id: Option<i64>,
    ) {
        let name = format!("已记住：{}", text.chars().take(30).collect::<String>());
        (self.emit)(ChatEvent::Tool {
            call: call.into(),
            agent: agent.name.clone(),
            name,
            arg: String::new(),
            status: status.into(),
            result: short(result, 600),
            path: None,
            memory_id: undo_id,
            agent_id: undo_id.map(|_| agent.id.clone()),
        });
    }

    /// 执行一个工具调用，返回给模型的结果文字
    pub(super) async fn call(
        self: &Arc<Self>,
        agent: &Agent,
        c: &llm::Call,
        stats: Option<Arc<DelegateStats>>,
    ) -> String {
        let call = format!("t{}", self.seq.fetch_add(1, Ordering::Relaxed));
        if self.plan_only && !plan_tool(&c.name) {
            return "只规划模式下不能做这一步".into();
        }
        let mut a: Value = match serde_json::from_str(if c.args.trim().is_empty() {
            "{}"
        } else {
            &c.args
        }) {
            Ok(v) => v,
            Err(_) => return "参数不是合法的 JSON".into(),
        };
        if c.name == "remember" {
            let Some(store) = self.world.memory_store() else {
                return "当前对话不能写入记忆".into();
            };
            if self.model(agent).is_some_and(|m| m.tools == Some(false)) {
                return "当前模型不能使用工具".into();
            }
            let text = arg(&a, "text")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.is_empty() || text.chars().count() > 500 {
                return "请提供 1 至 500 字的简短事实或偏好".into();
            }
            let text = if self.cfg.memory_ask {
                let (ans, _) = self
                    .ask(
                        &call,
                        "记住",
                        &format!("记住：{text}"),
                        &agent.name,
                        false,
                        true,
                    )
                    .await;
                if !ans.allow {
                    let why = if ans.reason.is_empty() {
                        "用户拒绝记住这条".to_string()
                    } else {
                        format!("用户拒绝记住这条，原因：{}", ans.reason)
                    };
                    self.pill(&call, agent, "记住偏好", "", "no", &why, None);
                    return why;
                }
                if !ans.edit.trim().is_empty() {
                    ans.edit.split_whitespace().collect::<Vec<_>>().join(" ")
                } else {
                    text
                }
            } else {
                text
            };
            if text.is_empty() || text.chars().count() > 500 {
                return "请提供 1 至 500 字的简短事实或偏好".into();
            }
            return match store.add(&agent.id, &text) {
                Ok(id) => {
                    let undo_id = (!self.cfg.memory_ask).then_some(id);
                    self.memory_pill(&call, agent, &text, "done", "已保存到独立记忆", undo_id);
                    "已记住".into()
                }
                Err(e) => format!("未能保存记忆：{e}"),
            };
        }
        if c.name == "read_skill" {
            if self.model(agent).is_some_and(|m| m.tools == Some(false)) {
                return "当前模型不能使用工具".into();
            }
            let name = a.get("name").and_then(Value::as_str).unwrap_or_default();
            let skills = self.cfg.assigned_skills(agent).collect::<Vec<_>>();
            let skill = skills.iter().find(|skill| skill.name == name).or_else(|| {
                let name = name.trim().to_lowercase();
                skills
                    .iter()
                    .find(|skill| skill.name.trim().to_lowercase() == name)
            });
            if let Some(skill) = skill {
                self.pill(
                    &call,
                    agent,
                    "读技能",
                    &skill.name,
                    "done",
                    "已读取完整步骤",
                    None,
                );
                return format!("技能：{}\n步骤：\n{}", skill.name, skill.steps);
            }
            let names = skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>()
                .join("、");
            let names = if names.is_empty() {
                "（无）"
            } else {
                &names
            };
            return format!("没有这个技能。你会的技能：{names}");
        }
        let Some(def) = DEFS.iter().find(|d| d.name == c.name) else {
            return format!("没有这个工具：{}", c.name);
        };
        let (name, shown) = label(&c.name, &a);
        let file_tool = matches!(
            c.name.as_str(),
            "read_file" | "list_dir" | "write_file" | "delete_file"
        );
        if !agent.tools.iter().any(|t| t == def.group) {
            return format!("{} 没有「{}」这个工具的权限", agent.name, def.group);
        }
        // 需要确认的先问
        let rk = risk(&c.name);
        let mut pre_authorized = false;
        if rk > 0 {
            let (desc, scope, kind) = describe(&c.name, &a);
            let key = format!("{kind} · {scope}");
            let always = self
                .cfg
                .always_allow
                .iter()
                .any(|x| x.kind == kind && x.scope == scope);
            let once = rk == 1 && self.allowed.lock().unwrap().contains(&key);
            if !always && !once {
                if !self.pre_authorized(&kind) {
                    self.pill(
                        &call,
                        agent,
                        &name,
                        &shown,
                        "wait",
                        "",
                        file_tool.then(|| arg(&a, "path")),
                    );
                }
                let (ans, pre) = self.ask(&call, &kind, &desc, &scope, rk == 2, false).await;
                pre_authorized = pre;
                if !ans.allow {
                    let why = if ans.reason.is_empty() {
                        "用户拒绝了这个操作".to_string()
                    } else {
                        format!("用户拒绝了这个操作，原因：{}", ans.reason)
                    };
                    self.pill(
                        &call,
                        agent,
                        &name,
                        &shown,
                        "no",
                        &why,
                        file_tool.then(|| arg(&a, "path")),
                    );
                    return why;
                }
                if !ans.edit.trim().is_empty() {
                    let k = match c.name.as_str() {
                        "write_file" => "content",
                        "send_email" => "body",
                        "open_url" => "url",
                        _ => "path",
                    };
                    a[k] = json!(ans.edit.trim());
                }
                if ans.always {
                    self.world.remember(Allow {
                        kind: kind.clone(),
                        scope: scope.clone(),
                    });
                }
                if !pre_authorized {
                    self.allowed.lock().unwrap().insert(key);
                }
            }
        }
        let path = file_tool.then(|| arg(&a, "path"));
        self.pill(
            &call,
            agent,
            &name,
            &shown,
            "run",
            if pre_authorized { "已预授权" } else { "" },
            path.clone(),
        );
        let r = self.exec(&c.name, &a, &call, &agent.id).await;
        if c.name == "write_file" && r.is_ok() {
            if let Some(stats) = stats {
                stats.writes.fetch_add(1, Ordering::Relaxed);
            }
        }
        let (status, result) = match &r {
            Ok(out) => ("done", out),
            Err(e) => ("err", e),
        };
        let shown_result = if pre_authorized {
            format!("已预授权 · {result}")
        } else {
            result.clone()
        };
        self.pill(&call, agent, &name, &shown, status, &shown_result, path);
        r.unwrap_or_else(|e| format!("出错了：{e}"))
    }

    pub(super) async fn exec(
        &self,
        name: &str,
        a: &Value,
        call: &str,
        agent_id: &str,
    ) -> Result<String, String> {
        match name {
            "web_search" => self.world.search(&arg(a, "query")).await,
            "fetch_page" => {
                let u = arg(a, "url");
                http_url(&u)?;
                self.world.fetch(&u).await
            }
            "read_file" => crate::capture::read_text_file(&arg(a, "path")).map(|(_, t)| t).map_err(|e| e.message),
            "list_dir" => list_dir(&arg(a, "path")),
            "write_file" => {
                let p = arg(a, "path");
                if p.is_empty() {
                    return Err("没有给文件路径".into());
                }
                let content = a.get("content").and_then(Value::as_str).unwrap_or("");
                let change = self
                    .changes
                    .write(Path::new(&p), content, &self.backup, call, agent_id)
                    .map_err(|e| e.to_string())?;
                (self.emit)(ChatEvent::Change { change });
                Ok(format!("已写入 {p}（{} 字）", content.chars().count()))
            }
            "delete_file" => {
                let p = PathBuf::from(arg(a, "path"));
                let change = self
                    .changes
                    .delete(&p, &self.trash, agent_id)
                    .map_err(|e| e.to_string())?;
                let result = format!(
                    "已删除 {}，移到了悬浮球回收站：{}",
                    p.display(),
                    change.backup.as_deref().unwrap().display()
                );
                (self.emit)(ChatEvent::Change { change });
                Ok(result)
            }
            "send_email" => {
                let enc = |s: &str| {
                    s.bytes()
                        .map(|b| {
                            if b.is_ascii_alphanumeric() || b"-._~@".contains(&b) {
                                (b as char).to_string()
                            } else {
                                format!("%{b:02X}")
                            }
                        })
                        .collect::<String>()
                };
                let url = format!(
                    "mailto:{}?subject={}&body={}",
                    enc(&arg(a, "to")),
                    enc(&arg(a, "subject")),
                    enc(a.get("body").and_then(Value::as_str).unwrap_or(""))
                );
                self.world.open(&url)?;
                Ok("已在邮件程序里打开这封邮件，等用户点发送".into())
            }
            "open_url" => {
                let u = arg(a, "url");
                http_url(&u)?;
                self.world.open(&u)?;
                Ok(format!("已在浏览器打开 {u}"))
            }
            _ => Err("没有这个工具".into()),
        }
    }
}
