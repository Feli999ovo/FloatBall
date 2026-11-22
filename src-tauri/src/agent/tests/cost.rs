use super::*;

#[test]
fn delegate_handoff_cost_uses_the_selected_provider_rate() {
    let e = env(vec![], |c| {
        c.usd_cny = 8.0;
        c.models[0].base_url = "https://api.openai.com/v1".into();
        c.models[0].provider = "OpenAI".into();
        c.agents[0].model = c.models[0].id;
    });
    e.task.catalog.lock().unwrap().replace(crate::catalog::Catalog::trim(
        &json!({"openai":{"name":"OpenAI","api":"https://api.openai.com/v1","models":{"m":{"modalities":{"input":["text"],"output":["text"]},"tool_call":true,"cost":{"input":0.15,"output":0.6}}}}}),
        &Value::Null, "2026-11-21"));
    assert!(
        (e.task
            .price_for(&e.task.cfg.models[0].base_url, "m", "OpenAI", 1_000, 2_000)
            .unwrap()
            - 0.0108)
            .abs()
            < 1e-12
    );
    assert_eq!(
        e.task
            .price_for("https://unknown.example/v1", "m", "", 1_000, 2_000),
        None
    );
}
