use super::*;

#[test]
fn reads_file_then_answers() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("报告.txt");
    std::fs::write(&f, "营收 +18%").unwrap();
    let e = env(
        vec![
            tool("read_file", json!({"path": f.to_string_lossy()})),
            text("营收涨了 18%"),
        ],
        |_| {},
    );
    let lead = e.task.cfg.lead().clone();
    let out = rt()
        .block_on(e.task.run(
            &lead,
            vec![json!({"role":"user","content":"总结报告"})],
            true,
        ))
        .unwrap();
    assert_eq!(out, "营收涨了 18%");
    assert_eq!(
        pills(&e),
        vec![
            ("读取文件".into(), "报告.txt".into(), "run".into()),
            ("读取文件".into(), "报告.txt".into(), "done".into())
        ]
    );
    let regular_tool = serde_json::to_value(
        e.events
            .lock()
            .unwrap()
            .iter()
            .find(|event| matches!(event, ChatEvent::Tool { .. }))
            .unwrap(),
    )
    .unwrap();
    assert!(regular_tool.get("memory_id").is_none() && regular_tool.get("agent_id").is_none());
    let got = e.got.lock().unwrap();
    // 第一次带上主控默认能用的工具（读文件、写文件）；搜索网页要用户自己勾，删除 / 发邮件 / 浏览器也不带
    let names: Vec<&str> = got[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["read_file", "list_dir", "write_file"]);
    assert_eq!(got[0]["messages"][0]["role"], "system");
    // 第二次把工具结果带回去
    let m = got[1]["messages"].as_array().unwrap();
    assert_eq!(
        m[m.len() - 1],
        json!({"role":"tool","tool_call_id":"c1","content":"营收 +18%"})
    );
    assert_eq!(
        m[m.len() - 2]["tool_calls"][0]["function"]["name"],
        "read_file"
    );
    assert_eq!(e.task.tokens(), 25 + 15);
}

#[test]
fn file_tool_events_carry_full_paths_only_for_file_tools() {
    let e = env(vec![], |c| c.agents[0].tools.push("搜索网页".into()));
    let folder = e
        .dir
        .path()
        .join("两个目录下可以有相同的名字，这里只测试完整路径");
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("周报.txt");
    std::fs::write(&path, "内容").unwrap();
    let file = llm::Call {
        name: "read_file".into(),
        args: json!({"path":path}).to_string(),
        ..Default::default()
    };
    let search = llm::Call {
        name: "web_search".into(),
        args: json!({"query":"周报"}).to_string(),
        ..Default::default()
    };
    rt().block_on(async {
        let lead = e.task.cfg.lead();
        e.task.call(lead, &file, None).await;
        e.task.call(lead, &search, None).await;
    });
    let events = e.events.lock().unwrap();
    let tools = events
        .iter()
        .filter(|ev| matches!(ev, ChatEvent::Tool { status, .. } if status == "done"));
    let serialized: Vec<Value> = tools.map(|ev| serde_json::to_value(ev).unwrap()).collect();
    assert_eq!(serialized.len(), 2);
    assert_eq!(serialized[0]["arg"], "周报.txt");
    assert_eq!(serialized[0]["path"], json!(path));
    assert!(serialized[1].get("path").is_none());
}

#[test]
fn read_skill_returns_steps_only_for_assigned_skills() {
    let e = env(
        vec![
            tool("read_skill", json!({"name":"核对出处"})),
            tool("read_skill", json!({"name":"  rEvIeW  "})),
            tool("read_skill", json!({"name":"审稿"})),
            text("完成"),
        ],
        |c| {
            for (name, when, steps) in [
                ("核对出处", "查资料时", "先找原文"),
                ("Review", "", "逐项复核"),
                ("审稿", "发布前", "看审稿标准"),
            ] {
                c.save_skill(crate::config::Skill {
                    id: String::new(),
                    name: name.into(),
                    when: when.into(),
                    steps: steps.into(),
                })
                .unwrap();
            }
            c.agents[0].skills = c.skills[..2].iter().map(|s| s.id.clone()).collect();
            c.upsert_agent(Agent {
                name: "审查员".into(),
                skills: vec![c.skills[2].id.clone()],
                ..Agent::default()
            })
            .unwrap();
        },
    );
    let lead = e.task.cfg.lead();
    assert_eq!(
        rt().block_on(e.task.run(lead, vec![], true)).unwrap(),
        "完成"
    );
    let got = e.got.lock().unwrap();
    let prompt = got[0]["messages"][0]["content"].as_str().unwrap();
    assert_eq!(prompt, format!("{}\n\n你会的技能（需要时调用 read_skill 读完整步骤）：\n- 核对出处：查资料时\n- Review：（没写什么时候用）", lead.prompt));
    assert!(got[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "read_skill"));
    let results: Vec<&str> = got
        .iter()
        .skip(1)
        .map(|request| {
            request["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(
        results,
        [
            "技能：核对出处\n步骤：\n先找原文",
            "技能：Review\n步骤：\n逐项复核",
            "没有这个技能。你会的技能：核对出处、Review"
        ]
    );
    assert_eq!(
        pills(&e),
        vec![
            ("读技能".into(), "核对出处".into(), "done".into()),
            ("读技能".into(), "Review".into(), "done".into())
        ]
    );
}
