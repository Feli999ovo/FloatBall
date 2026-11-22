use super::*;
use crate::config::Limits;

#[test]
fn write_asks_once_and_always_remembers() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("out").join("a.md");
    let e = env(
        vec![
            tool(
                "write_file",
                json!({"path": f.to_string_lossy(), "content": "# 周报"}),
            ),
            text("写好了"),
        ],
        |_| {},
    );
    let lead = e.task.cfg.lead().clone();
    let r = rt();
    let (out, ans) = r.block_on(async {
        let t = e.task.clone();
        let h = tokio::spawn(async move {
            t.run(
                &t.cfg.lead().clone(),
                vec![json!({"role":"user","content":"写周报"})],
                true,
            )
            .await
        });
        let a = answer(
            &e,
            Answer {
                allow: true,
                always: true,
                ..Answer::default()
            },
        )
        .await;
        (h.await.unwrap(), a)
    });
    let _ = lead;
    assert_eq!(out.unwrap(), "写好了");
    assert_eq!(
        ans,
        (
            format!("写入文件 {}", f.to_string_lossy()),
            "out".into(),
            false
        )
    );
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "# 周报");
    assert_eq!(
        *e.fake.remembered.lock().unwrap(),
        vec![Allow {
            kind: "写入".into(),
            scope: "out".into()
        }]
    );
    let ask = serde_json::to_value(
        e.events
            .lock()
            .unwrap()
            .iter()
            .find(|event| matches!(event, ChatEvent::Ask { .. }))
            .unwrap(),
    )
    .unwrap();
    assert!(ask.get("no_always").is_none());
    let st: Vec<String> = pills(&e).into_iter().map(|p| p.2).collect();
    assert_eq!(st, vec!["wait", "run", "done"]);
}

#[test]
fn delete_denied_keeps_file_and_tells_why() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("旧.txt");
    std::fs::write(&f, "x").unwrap();
    let e = env(
        vec![
            tool("delete_file", json!({"path": f.to_string_lossy()})),
            text("好的，不删了"),
        ],
        |c| c.agents[0].tools.push("删除".into()),
    );
    let out = rt().block_on(async {
        let t = e.task.clone();
        let h = tokio::spawn(async move {
            t.run(
                &t.cfg.lead().clone(),
                vec![json!({"role":"user","content":"删掉旧文件"})],
                true,
            )
            .await
        });
        let a = answer(
            &e,
            Answer {
                reason: "还要用".into(),
                ..Answer::default()
            },
        )
        .await;
        assert!(a.2, "删除是高风险");
        h.await.unwrap()
    });
    assert_eq!(out.unwrap(), "好的，不删了");
    assert!(f.exists());
    let m = e.got.lock().unwrap()[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(m["content"], "用户拒绝了这个操作，原因：还要用");
    assert_eq!(pills(&e).last().unwrap().2, "no");
}

#[test]
fn delete_allowed_moves_to_trash_and_always_allow_skips_ask() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("dl").join("旧.txt");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "x").unwrap();
    let e = env(
        vec![
            tool("delete_file", json!({"path": f.to_string_lossy()})),
            text("删好了"),
        ],
        |c| {
            c.agents[0].tools.push("删除".into());
            c.always_allow.push(Allow {
                kind: "删除".into(),
                scope: "dl".into(),
            });
        },
    );
    let out = rt().block_on(e.task.run(
        &e.task.cfg.lead().clone(),
        vec![json!({"role":"user","content":"删"})],
        true,
    ));
    assert_eq!(out.unwrap(), "删好了");
    assert!(!f.exists());
    let trashed: Vec<_> = std::fs::read_dir(e.dir.path().join("回收站"))
        .unwrap()
        .collect();
    assert_eq!(trashed.len(), 1);
    assert!(!e
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|x| matches!(x, ChatEvent::Ask { .. })));
}

#[test]
fn email_opens_draft_after_edit() {
    let e = env(
        vec![
            tool(
                "send_email",
                json!({"to":"wang@x.com","subject":"周报","body":"见附件"}),
            ),
            text("已打开"),
        ],
        |c| c.agents[0].tools.push("发邮件".into()),
    );
    let out = rt().block_on(async {
        let t = e.task.clone();
        let h = tokio::spawn(async move {
            t.run(
                &t.cfg.lead().clone(),
                vec![json!({"role":"user","content":"发周报"})],
                true,
            )
            .await
        });
        let a = answer(
            &e,
            Answer {
                allow: true,
                edit: "改过的正文".into(),
                ..Answer::default()
            },
        )
        .await;
        assert_eq!(
            a,
            (
                "发送邮件给 wang@x.com：周报".into(),
                "wang@x.com".into(),
                true
            )
        );
        h.await.unwrap()
    });
    assert_eq!(out.unwrap(), "已打开");
    let o = e.fake.opened.lock().unwrap();
    assert_eq!(o[0], "mailto:wang@x.com?subject=%E5%91%A8%E6%8A%A5&body=%E6%94%B9%E8%BF%87%E7%9A%84%E6%AD%A3%E6%96%87");
}

#[test]
fn routine_permission_only_applies_to_selected_kind_in_its_task() {
    let mut e = env(vec![], |c| c.agents[0].tools.push("删除".into()));
    let path = e.dir.path().join("report.txt");
    let write = llm::Call {
        id: "w".into(),
        name: "write_file".into(),
        args: json!({"path":path,"content":"周报"}).to_string(),
    };
    let delete = llm::Call {
        id: "d".into(),
        name: "delete_file".into(),
        args: json!({"path":path}).to_string(),
    };
    Arc::get_mut(&mut e.task).unwrap().pre_allow = vec!["写入".into()];
    rt().block_on(async {
        let lead = e.task.cfg.lead();
        assert!(e.task.call(lead, &write, None).await.starts_with("已写入"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "周报");
        assert!(!e.events.lock().unwrap().iter().any(|x| matches!(x, ChatEvent::Ask { .. })));
        assert!(e.events.lock().unwrap().iter().any(|x| matches!(x, ChatEvent::Tool { result, status, .. } if status == "done" && result.starts_with("已预授权"))));
        let (_, (desc, _, _)) = tokio::join!(e.task.call(lead, &delete, None), answer(&e, Answer::default()));
        assert!(desc.starts_with("删除"));
        assert!(path.exists());
    });
    let ordinary = env(vec![], |_| {});
    rt().block_on(async {
        let (_, (desc, _, _)) = tokio::join!(
            ordinary.task.call(ordinary.task.cfg.lead(), &write, None),
            answer(&ordinary, Answer::default())
        );
        assert!(desc.starts_with("写入文件"));
    });
}

async fn answer_budget(e: &Env, allow: bool) -> (String, u64, u64, u64) {
    for _ in 0..200 {
        let b = e.events.lock().unwrap().iter().find_map(|x| match x {
            ChatEvent::Budget {
                ask,
                kind,
                used,
                limit,
                more,
            } => Some((*ask, kind.clone(), *used, *limit, *more)),
            _ => None,
        });
        if let Some((id, k, u, l, m)) = b {
            assert!(e.task.asks.answer(
                id,
                Answer {
                    allow,
                    ..Answer::default()
                }
            ));
            return (k, u, l, m);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("没有出现预算卡")
}

#[test]
fn budget_pauses_and_asks() {
    // 步数用满：暂停问用户，选「停止」就结束
    let e = env(
        (0..5)
            .map(|_| tool("web_search", json!({"query":"x"})))
            .collect(),
        |c| {
            c.limits = Limits {
                tokens: 0,
                steps: 3,
                parallel: 1,
            }
        },
    );
    let (out, b) = rt().block_on(async {
        let t = e.task.clone();
        let h = tokio::spawn(async move {
            t.run(
                &t.cfg.lead().clone(),
                vec![json!({"role":"user","content":"搜"})],
                true,
            )
            .await
        });
        let b = answer_budget(&e, false).await;
        (h.await.unwrap(), b)
    });
    assert!(out.unwrap_err().contains("最多步数"));
    assert_eq!(b, ("steps".into(), 3, 3, MORE_STEPS));
    // token 用满：选「再给 2万」就接着做完
    let e = env(
        vec![
            tool("web_search", json!({"query":"x"})),
            tool("web_search", json!({"query":"y"})),
            text("好了"),
        ],
        |c| {
            c.limits = Limits {
                tokens: 50,
                steps: 50,
                parallel: 1,
            }
        },
    );
    let (out, b) = rt().block_on(async {
        let t = e.task.clone();
        let h = tokio::spawn(async move {
            t.run(
                &t.cfg.lead().clone(),
                vec![json!({"role":"user","content":"搜"})],
                true,
            )
            .await
        });
        let b = answer_budget(&e, true).await;
        (h.await.unwrap(), b)
    });
    assert_eq!(out.unwrap(), "好了");
    assert_eq!((b.0.as_str(), b.2, b.3), ("tokens", 50, MORE_TOKENS));
    assert!(b.1 >= 50);
    assert_eq!(e.got.lock().unwrap().len(), 3);
    // 只问了一次
    assert_eq!(
        e.events
            .lock()
            .unwrap()
            .iter()
            .filter(|x| matches!(x, ChatEvent::Budget { .. }))
            .count(),
        1
    );
}
