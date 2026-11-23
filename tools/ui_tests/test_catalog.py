from trim_models_dev import trim
from ui_support import check


def test_catalog():
    api = {"alpha": {"name": "Alpha", "api": "https://same.example/v1", "models": {
        "priced": {"tool_call": True, "modalities": {"input": ["text"], "output": ["text"]}, "cost": {"input": 1.25, "output": 3.5}},
        "zero": {"tool_call": True, "modalities": {"input": ["text"], "output": ["text"]}, "cost": {"input": 0, "output": 0}},
        "missing": {"tool_call": True, "modalities": {"input": ["text"], "output": ["text"]}},
        "invalid": {"tool_call": True, "modalities": {"input": ["text"], "output": ["text"]}, "cost": {"input": True, "output": 1}},
    }}}
    trimmed = trim(api, {}, "2026-11-21")
    check(trimmed["providers"]["alpha"]["c"] == {"priced": [1.25, 3.5], "zero": [0, 0]}, "模型资料按服务商保存准确单价，保留零价并跳过缺失或无效价格")
