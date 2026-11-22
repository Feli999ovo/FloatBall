use super::*;

#[test]
fn handoff_count_prefers_successful_file_writes_then_result_paragraphs() {
    assert_eq!(handoff_summary("a\n\nb\n\nc", 0), (3, "段结果".into()));
    assert_eq!(
        handoff_summary("仍然按换行保留分段", 2),
        (2, "份文件".into())
    );
}

#[test]
fn delegate_handoff_event_connects_successive_agents() {
    let e = env(
        vec![
            tool(
                "delegate",
                json!({"agent":"研究员","task":"先收集三个来源"}),
            ),
            text("来源一\n\n来源二"),
            tool(
                "delegate",
                json!({"agent":"整理员","task":"依据来源做对比"}),
            ),
            text("比较完成"),
            text("汇总"),
        ],
        |c| {
            c.upsert_agent(Agent {
                name: "研究员".into(),
                role: "找来源".into(),
                ..Agent::default()
            })
            .unwrap();
            c.upsert_agent(Agent {
                name: "整理员".into(),
                role: "整理资料".into(),
                ..Agent::default()
            })
            .unwrap();
        },
    );
    let out = rt().block_on(e.task.run(&e.task.cfg.lead().clone(), vec![], true));
    assert_eq!(out.unwrap(), "汇总");
    let events = e.events.lock().unwrap();
    let handoffs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ChatEvent::Handoff {
                from,
                to,
                count,
                unit,
                to_call,
                ..
            } => Some((
                from.as_str(),
                to.as_str(),
                *count,
                unit.as_str(),
                to_call.as_str(),
            )),
            _ => None,
        })
        .collect();
    let target = events
        .iter()
        .find_map(|event| match event {
            ChatEvent::Team {
                agent,
                call,
                status,
                ..
            } if agent == "整理员" && status == "run" => Some(call.as_str()),
            _ => None,
        })
        .unwrap();
    assert_eq!(handoffs, vec![("研究员", "整理员", 2, "段结果", target)]);
}

#[test]
fn delegate_handoff_does_not_skip_failed_calls() {
    let batch = |agents: &[&str]| {
        let calls: Vec<_> = agents.iter().enumerate().map(|(index, agent)| json!({
            "index": index, "id": format!("call{index}"), "type": "function",
            "function": {"name": "delegate", "arguments": json!({"agent":agent,"task":"test"}).to_string()}
        })).collect();
        format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"tool_calls":calls}}]})
        )
    };
    let e = env(
        vec![
            batch(&["A", "missing", "C"]),
            text("result"),
            text("result"),
            batch(&["C", "missing"]),
            text("result"),
            batch(&["A"]),
            text("result"),
            text("summary"),
        ],
        |c| {
            for name in ["A", "C"] {
                c.upsert_agent(Agent {
                    name: name.into(),
                    ..Agent::default()
                })
                .unwrap();
            }
        },
    );
    let out = rt().block_on(e.task.run(e.task.cfg.lead(), vec![], true));
    assert_eq!(out.unwrap(), "summary");
    let events = e.events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ChatEvent::Team { status, .. } if status == "done"))
            .count(),
        4
    );
    assert!(!events
        .iter()
        .any(|event| matches!(event, ChatEvent::Handoff { .. })));
}

#[test]
fn delegates_to_sub_agent() {
    let e = env(
        vec![
            tool("delegate", json!({"agent":"审查员","task":"检查错别字"})),
            text("没有错别字"),
            text("审查员说没有错别字"),
        ],
        |c| {
            c.save_skill(crate::config::Skill {
                id: String::new(),
                name: "核对出处".into(),
                when: "查资料时".into(),
                steps: "先找原文".into(),
            })
            .unwrap();
            c.upsert_agent(Agent {
                name: "审查员".into(),
                role: "检查错别字".into(),
                prompt: "你是审查员".into(),
                skills: vec![c.skills[0].id.clone()],
                ..Agent::default()
            })
            .unwrap();
        },
    );
    let out = rt().block_on(e.task.run(
        &e.task.cfg.lead().clone(),
        vec![json!({"role":"user","content":"检查一下"})],
        true,
    ));
    assert_eq!(out.unwrap(), "审查员说没有错别字");
    let got = e.got.lock().unwrap();
    assert!(got[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "delegate"));
    // 子 Agent 用自己的指令，不能再往下交
    assert_eq!(
        got[1]["messages"][0],
        json!({"role":"system","content":"你是审查员\n\n你会的技能（需要时调用 read_skill 读完整步骤）：\n- 核对出处：查资料时"})
    );
    assert!(!got[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("read_skill"));
    assert!(!got[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "read_skill"));
    assert!(!got[1]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "delegate"));
    // 交给子 Agent 不再是工具药丸，而是计划卡的一行：开始 → 完成（带这一步用掉的 token）
    assert!(pills(&e).is_empty());
    let team: Vec<(String, String, String)> = e
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|x| match x {
            ChatEvent::Team {
                agent,
                task,
                status,
                ..
            } => Some((agent.clone(), task.clone(), status.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        team,
        vec![
            ("审查员".into(), "检查错别字".into(), "wait".into()),
            ("审查员".into(), "检查错别字".into(), "run".into()),
            ("审查员".into(), "检查错别字".into(), "done".into())
        ]
    );
    // 子 Agent 的正文不流到主控的气泡里
    let deltas: String = e
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|x| match x {
            ChatEvent::Delta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, "审查员说没有错别字");
}

#[test]
fn review_uses_selected_agent_and_only_read_only_tools() {
    let mut e = env(vec![text("审查完成")], |c| {
        c.upsert_agent(Agent {
            name: "审查员".into(),
            prompt: "独立审查".into(),
            tools: vec!["写文件".into()],
            ..Agent::default()
        })
        .unwrap();
    });
    let id = e.task.cfg.agents[1].id.clone();
    let review = crate::review_agent_for(&e.task.cfg, &id).unwrap();
    Arc::get_mut(&mut e.task).unwrap().plan_only = true;
    assert!(crate::review_agent_for(&e.task.cfg, "missing").is_err());
    let out = rt().block_on(e.task.run(
        &review,
        vec![json!({"role":"user","content":"审查文件"})],
        false,
    ));
    assert_eq!(out.unwrap(), "审查完成");
    let got = e.got.lock().unwrap();
    assert!(got[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("独立审查"));
    let tools: Vec<_> = got[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert!(tools.contains(&"read_file"));
    assert!(!tools.contains(&"write_file") && !tools.contains(&"delegate"));
}
