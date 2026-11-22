use super::*;

#[test]
fn tool_not_granted_is_refused() {
    let e = env(
        vec![
            tool(
                "send_email",
                json!({"to":"a@b.c","subject":"hi","body":"x"}),
            ),
            text("发不了"),
        ],
        |_| {},
    );
    let out = rt().block_on(e.task.run(
        &e.task.cfg.lead().clone(),
        vec![json!({"role":"user","content":"发邮件"})],
        true,
    ));
    assert_eq!(out.unwrap(), "发不了");
    assert!(e.fake.opened.lock().unwrap().is_empty());
    let m = e.got.lock().unwrap()[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(m["content"], "主控 没有「发邮件」这个工具的权限");
}

#[test]
fn planning_limits_main_and_delegated_tools() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("should-not-exist.txt");
    let write = tool("write_file", json!({"path":path,"content":"不得写入"}));
    let all_tools = || DEFS.iter().map(|d| d.group.to_string()).collect();
    let mut e = env_with_memory(
        vec![
            write.clone(),
            tool("delegate", json!({"agent":"审查员","task":"只读审查"})),
            write,
            text("审查完成"),
            text("规划完成"),
        ],
        |c| {
            c.agents[0].tools = all_tools();
            c.upsert_agent(Agent {
                name: "审查员".into(),
                tools: all_tools(),
                ..Agent::default()
            })
            .unwrap();
        },
        Some(crate::store::Store::open(&dir.path().join("memory.db")).unwrap()),
    );
    Arc::get_mut(&mut e.task).unwrap().plan_only = true;
    let result = rt().block_on(e.task.run(
        e.task.cfg.lead(),
        vec![json!({"role":"user","content":"规划"})],
        true,
    ));
    assert_eq!(result.unwrap(), "规划完成");
    assert!(!path.exists());
    let got = e.got.lock().unwrap();
    for request in got.iter() {
        assert!(request["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("现在是只规划模式"));
        assert!(request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| plan_tool(t["function"]["name"].as_str().unwrap())));
    }
    assert_eq!(
        got[1]["messages"].as_array().unwrap().last().unwrap()["content"],
        "只规划模式下不能做这一步"
    );
    assert_eq!(
        got[3]["messages"].as_array().unwrap().last().unwrap()["content"],
        "只规划模式下不能做这一步"
    );
    assert!(got[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "delegate"));
    assert!(got[2]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "read_file"));
}

#[test]
fn model_without_tools_gets_plain_chat() {
    let e = env(vec![text("你好")], |c| c.models[0].tools = Some(false));
    let out = rt().block_on(e.task.run(
        &e.task.cfg.lead().clone(),
        vec![json!({"role":"user","content":"hi"})],
        true,
    ));
    assert_eq!(out.unwrap(), "你好");
    assert!(e.got.lock().unwrap()[0].get("tools").is_none());
}
