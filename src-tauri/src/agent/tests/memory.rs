use super::*;

#[test]
fn ordinary_agents_can_remember_and_only_receive_the_latest_twenty() {
    let store =
        crate::store::Store::from_conn(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    for n in 0..21 {
        store.remember("lead", &format!("fact-{n:02}")).unwrap();
    }
    store.remember("other", "other-agent").unwrap();
    let e = env_with_memory(
        vec![
            tool("remember", json!({"text":"new preference"})),
            text("ok"),
        ],
        |_| {},
        Some(store),
    );
    let result = rt().block_on(e.task.run(e.task.cfg.lead(), vec![], true));
    assert_eq!(result.unwrap(), "ok");
    let got = e.got.lock().unwrap();
    assert!(got[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "remember"));
    let prompt = got[0]["messages"][0]["content"].as_str().unwrap();
    assert!(!prompt.contains("fact-00") && !prompt.contains("other-agent"));
    for n in 1..21 {
        assert!(prompt.contains(&format!("fact-{n:02}")));
    }
    assert_eq!(
        e.fake
            .memory
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .memory("lead", 1)
            .unwrap()[0]
            .text,
        "new preference"
    );
}

#[test]
fn automatic_memory_event_carries_the_inserted_scoped_row_id() {
    let store =
        crate::store::Store::from_conn(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let e = env_with_memory(
        vec![
            tool("remember", json!({"text":"用户喜欢短答案"})),
            text("已记下"),
        ],
        |_| {},
        Some(store),
    );
    let out = rt().block_on(e.task.run(e.task.cfg.lead(), vec![], true));
    assert_eq!(out.unwrap(), "已记下");
    let rows = e
        .fake
        .memory
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .memory("lead", 10)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "用户喜欢短答案");
    let event = e
        .events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            ChatEvent::Tool {
                memory_id,
                agent_id,
                name,
                arg,
                status,
                ..
            } => Some((
                *memory_id,
                agent_id.clone(),
                name.clone(),
                arg.clone(),
                status.clone(),
            )),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (
            event.0,
            event.1.as_deref(),
            event.2.as_str(),
            event.3.as_str(),
            event.4.as_str()
        ),
        (
            Some(rows[0].id),
            Some("lead"),
            "已记住：用户喜欢短答案",
            "",
            "done"
        )
    );
    let serialized = serde_json::to_value(
        e.events
            .lock()
            .unwrap()
            .iter()
            .find(|ev| matches!(ev, ChatEvent::Tool { .. }))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(serialized["memory_id"], rows[0].id);
    assert_eq!(serialized["agent_id"], "lead");
    assert!(serialized.get("memoryId").is_none());
}

#[test]
fn memory_ask_refusal_does_not_write_and_disallows_always() {
    let store =
        crate::store::Store::from_conn(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let e = env_with_memory(
        vec![
            tool("remember", json!({"text":"用户喜欢短答案"})),
            text("明白"),
        ],
        |c| c.memory_ask = true,
        Some(store),
    );
    let r = rt();
    let (out, result) = r.block_on(async {
        let t = e.task.clone();
        let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![], true).await });
        let (desc, _, _) = answer(
            &e,
            Answer {
                reason: "这条不准确".into(),
                ..Answer::default()
            },
        )
        .await;
        (h.await.unwrap(), desc)
    });
    assert_eq!(out.unwrap(), "明白");
    assert_eq!(result, "记住：用户喜欢短答案");
    assert!(e
        .fake
        .memory
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .memory("lead", 10)
        .unwrap()
        .is_empty());
    let events = e.events.lock().unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ChatEvent::Ask {
            no_always: true,
            ..
        }
    )));
    let ask = events
        .iter()
        .find(|event| matches!(event, ChatEvent::Ask { .. }))
        .unwrap();
    assert_eq!(serde_json::to_value(ask).unwrap()["no_always"], true);
    assert_eq!(
        e.got.lock().unwrap()[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["content"],
        "用户拒绝记住这条，原因：这条不准确"
    );
}

#[test]
fn memory_ask_writes_the_edited_text_without_undo_ids() {
    let store =
        crate::store::Store::from_conn(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let e = env_with_memory(
        vec![tool("remember", json!({"text":"原始偏好"})), text("好的")],
        |c| c.memory_ask = true,
        Some(store),
    );
    let r = rt();
    let out = r.block_on(async {
        let t = e.task.clone();
        let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![], true).await });
        let (desc, _, _) = answer(
            &e,
            Answer {
                allow: true,
                edit: "修改后的偏好".into(),
                ..Answer::default()
            },
        )
        .await;
        assert_eq!(desc, "记住：原始偏好");
        h.await.unwrap()
    });
    assert_eq!(out.unwrap(), "好的");
    let rows = e
        .fake
        .memory
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .memory("lead", 10)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "修改后的偏好");
    assert!(e.events.lock().unwrap().iter().any(|event| matches!(event, ChatEvent::Tool { name, memory_id: None, agent_id: None, .. } if name == "已记住：修改后的偏好")));
}

#[test]
fn task_without_memory_rejects_unadvertised_remember() {
    let e = env(
        vec![tool("remember", json!({"text":"secret draft"})), text("ok")],
        |_| {},
    );
    assert!(rt()
        .block_on(e.task.run(e.task.cfg.lead(), vec![], true))
        .is_ok());
    let got = e.got.lock().unwrap();
    assert!(!got[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "remember"));
    assert_eq!(
        got[1]["messages"].as_array().unwrap().last().unwrap()["content"],
        "当前对话不能写入记忆"
    );
}
