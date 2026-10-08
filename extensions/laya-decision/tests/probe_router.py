#!/usr/bin/env python3
"""LAYA post-enablement probe suite — canonical router questions vs sidecar.

Questions are byte-identical to crates/neomind-api/src/handlers/decisions.rs
(TOOL_INSTRUCTIONS/TOOL_CRITERIA, SIMPLE_INSTRUCTIONS noul, DOMAIN_*).
Constants mirror crates/neomind-api/src/handlers/decisions.rs — if the
platform questions change, update them here (drift = invalid eval).
Scoring gates mirror the production GatePolicy: domain 0.8, tool/simple 0.7.
"""
import json, time, urllib.request

SIDECAR = "http://127.0.0.1:8123/v1/systemone"

TOOL_INSTRUCTIONS = "Which tool should serve this user request?"
TOOL_CRITERIA = {
    "shell": "ALL neomind CLI platform operations, INCLUDING device control commands (reboot a device, send downlink), rules, agents config, dashboards, transforms, messages, extensions, purge/export/import",
    "query_conclusion": "read an AI agent's most recent conclusion — instant and free ('is X ok', 'what did the inspection say')",
    "run_now": "trigger a FRESH run of an AI AGENT right now — only for agents; device commands (e.g. reboot a sensor) are shell, not this",
    "skill": "load a step-by-step workflow guide for a complex or unfamiliar domain (onboarding, development guides)",
    "memory": "persist or recall cross-conversation facts and preferences ('remember…', 'forget…', 'what do you know about me')",
    "vision": "understand, read or describe the CONTENT of an image — what is in it, gauge readings, text, anomalies; no pixel changes",
    "image_edit": "modify image pixels: crop, draw, blur, annotate, highlight — changes the picture, does not interpret it",
    "web_fetch": "fetch the content of an external URL",
    "file_write": "create or overwrite a WHOLE file with new content",
    "file_edit": "surgically replace an exact existing string inside an existing file, keeping the rest untouched",
    "none": "pure conversation — greeting, identity, thanks, chat with no platform entity",
}
SIMPLE_INSTRUCTIONS = "Can this request be completed with a single direct command or lookup, without multi-step reasoning, correlation or drafting?"
DOMAIN_INSTRUCTIONS = "Which NeoMind CLI domain (or special path) should handle the user request?"
DOMAIN_CRITERIA = {
    "device": "inspect, onboard, control or configure devices and read their metrics",
    "rule": "create, inspect or manage automation rules (threshold alerts, event triggers)",
    "message": "send notifications or manage notification channels and alert history",
    "extension": "install, configure or invoke extension capabilities (vision AI, bridges)",
    "agent": "create, run or manage scheduled AI agents",
    "dashboard": "create or edit dashboards and bind data sources to cards/widgets",
    "transform": "create or test data transforms (computed/virtual metrics)",
    "widget": "develop or manage community dashboard widgets",
    "connector": "manage protocol connectors (Modbus, OPC-UA, BACnet, LoRaWAN)",
    "push": "configure forwarding telemetry to external systems (data push)",
    "llm": "configure or switch LLM backends",
    "settings": "platform settings (retention, language, system configuration)",
    "system": "system diagnostics, health, logs, version info",
    "image_edit": "edit or annotate an image (crop, draw, blur)",
    "converse": "plain conversation, greeting, opinion or summary needing no platform tool",
}
QUESTIONS = {
    "tool": {"type": "choice", "instructions": TOOL_INSTRUCTIONS, "criteria": TOOL_CRITERIA},
    "simple": {"type": "noul", "instructions": SIMPLE_INSTRUCTIONS},
    "domain": {"type": "choice", "instructions": DOMAIN_INSTRUCTIONS, "criteria": DOMAIN_CRITERIA},
}
GATES = {"domain": 0.8, "tool": 0.7, "simple": 0.7}

# (utterance, expected domain, expected tool, expected simple)
PROBES = [
    ("查看所有在线设备的状态",            "device",   "shell",  True),
    ("reboot sensor-01 please",          "device",   "shell",  True),
    ("创建一个温度超过30度就告警的规则",   "rule",     "shell",  False),
    ("帮我新建一个温湿度仪表盘",          "dashboard","shell",  False),
    ("发一条告警通知到钉钉",              "message",  "shell",  False),
    ("系统健康状态怎么样",                "system",   "shell",  True),
    ("How many devices are online now?", "device",   "shell",  True),
    ("你觉得边缘计算的未来怎么样",         "converse", "none",   True),
    ("你好呀",                           "converse", "none",   True),
    ("这张图片里有什么异常吗",             "extension","vision", False),
    ("记住我的名字是小明",                "settings", "memory", True),
    ("抓取这个网页的内容 https://example.com/doc", "system", "web_fetch", True),
    ("巡检代理上次跑的结论是什么",         "agent",    "query_conclusion", True),
    ("马上再跑一次巡检代理",              "agent",    "run_now", True),
    ("帮我裁剪一下这张图并加上马赛克",      "image_edit","image_edit", False),
    ("综合分析最近一周的温度数据写一份异常报告", "transform", "shell", False),
]

def ask(message):
    body = json.dumps({"state": f"User request: {message}", "questions": QUESTIONS}).encode()
    req = urllib.request.Request(SIDECAR, data=body, headers={"Content-Type": "application/json"})
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=10) as r:
        out = json.load(r)
    return out, (time.time() - t0) * 1000

ok = miss = lowgate = 0
rows = []
for msg, ed, et, es in PROBES:
    try:
        out, ms = ask(msg)
    except Exception as e:
        rows.append((msg, "ERROR", str(e)[:40], "", "", 0)); miss += 1; continue
    a = out.get("answers", {})
    dom = a.get("domain", {}); tool = a.get("tool", {}); sim = a.get("simple", {})
    dc, tc = dom.get("choice"), tool.get("choice")
    dconf, tconf = dom.get("answer_confidence") or 0, tool.get("answer_confidence") or 0
    sconf = sim.get("answer_confidence") or 0
    d_ok = dc == ed
    t_ok = tc == et
    s_val = (sim.get("noul") or 0) >= 0.5
    s_ok = s_val == es
    row_ok = d_ok and t_ok and s_ok
    gated = dconf >= GATES["domain"]
    rows.append((msg, f"{dc}({dconf:.2f}){'✓' if d_ok else '✗ exp:' + ed}",
                 f"{tc}({tconf:.2f}){'✓' if t_ok else '✗ exp:' + et}",
                 f"{'Y' if s_val else 'N'}({sconf:.2f}){'✓' if s_ok else '✗ exp:' + ('Y' if es else 'N')}",
                 "GATE" if gated else "pass", round(ms)))
    ok += row_ok; miss += (not row_ok)

print(f"{'utterance':38s} {'domain':22s} {'tool':26s} {'simple':14s} {'dom-gate':8s} ms")
for r in rows:
    print(f"{r[0]:38s} {str(r[1]):22s} {str(r[2]):26s} {str(r[3]):14s} {r[4] if len(r)>4 else '':8s} {r[5] if len(r)>5 else ''}")
print(f"\nfull-match: {ok}/{len(PROBES)}  miss: {miss}")
