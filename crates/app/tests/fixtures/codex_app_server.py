#!/usr/bin/python3
"""Original deterministic app-server peer, never an inference or auth substitute.
Runs inside the production namespace and exercises the real host tool/MCP paths.
"""
import json
import os
import queue
import select
import subprocess
import sys
import threading

lock = threading.Lock()
replies = queue.Queue()
interrupted = threading.Event()
turn_count = 0
servers = {}
api_mode = False
turn_model = None


def emit(value):
    with lock:
        print(json.dumps(value), flush=True)


def note(method, **params):
    emit(dict(method=method, params=dict(threadId="fixture-thread", **params)))


def item(tool, arguments, call_id, kind="dynamicToolCall", **extra):
    return dict(id=call_id, type=kind, tool=tool, arguments=arguments, **extra)


def dynamic(tool, arguments, call_id):
    value = item(tool, arguments, call_id, status="inProgress")
    note("item/started", item=value)
    emit(dict(id=call_id, method="item/tool/call", params=dict(
        threadId="fixture-thread", turnId=f"remote-{turn_count}", callId=call_id,
        tool=tool, arguments=arguments)))
    reply = replies.get(timeout=15)
    assert reply["id"] == call_id
    value.update(status="completed" if reply["result"]["success"] else "failed", **reply["result"])
    note("item/completed", item=value)
    return reply["result"]


def run_turn(text, number):
    try:
        # Neither project sources nor the real host home are exposed to Codex.
        if text.startswith("edit:") or text.startswith("reject:"):
            root = text.split(":", 1)[1]
            assert not os.path.exists(root + "/scene.c"), "game tree leaked into Codex namespace"
            read = dynamic("tauros_read_file", dict(path="scene.c"), "read")
            assert read["success"]
            write = dynamic("tauros_write_file", dict(path="scene.c", content="edited by Codex fixture\n"), "write")
            assert write["success"] == text.startswith("edit:")
            escape = dynamic("tauros_write_file", dict(path="../escape.c", content="bad"), "escape")
            assert not escape["success"]
            # Built-in file edits, exec escalation and permissions must be refused.
            for method, expected in [("item/fileChange/requestApproval", {"decision":"decline"}),
                                     ("item/commandExecution/requestApproval", {"decision":"decline"}),
                                     ("item/permissions/requestApproval", {"permissions":{}})]:
                emit(dict(id="approval", method=method, params=dict(threadId="fixture-thread", turnId=f"remote-{number}")))
                assert replies.get(timeout=10)["result"] == expected
            cfg = servers["tauros"]
            child = subprocess.Popen([cfg["command"], *cfg["args"]], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, text=True)
            try:
                def rpc(method, params, ident):
                    child.stdin.write(json.dumps(dict(jsonrpc="2.0", id=ident, method=method, params=params)) + "\n")
                    child.stdin.flush()
                    while True:
                        assert select.select([child.stdout], [], [], 10)[0], "MCP response timed out"
                        reply = json.loads(child.stdout.readline())
                        if reply.get("id") == ident:
                            assert "error" not in reply, reply
                            return reply["result"]
                rpc("initialize", dict(protocolVersion="2024-11-05", capabilities={}, clientInfo=dict(name="codex-fixture", version="1")), 1)
                child.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
                child.stdin.flush()
                result = rpc("tools/call", dict(name="create_trainer", arguments=dict(
                    id="TRAINER_CODEX_FIXTURE", name="Codex", **{"class":"Youngster"},
                    party=[dict(species="SPECIES_TEST", level=7)])), 2)
                assert not result.get("isError"), result
                value = item("create_trainer", {}, "mcp", "mcpToolCall", server="tauros", status="inProgress")
                note("item/started", item=value)
                value.update(status="completed", result=result)
                note("item/completed", item=value)
            finally:
                child.terminate()
                child.wait(timeout=5)
        elif text == "wait":
            interrupted.wait(15)
        elif text == "model?":
            note("item/agentMessage/delta", delta=f"model {turn_model} ")
        elif text == "continue":
            assert number == 2, "session continuity lost"
        elif text == "crash":
            os._exit(1)
        note("item/reasoning/summaryTextDelta", delta="checked")
        note("item/agentMessage/delta", delta=f"turn {number}")
        note("turn/completed", turn=dict(id=f"remote-{number}", status="interrupted" if interrupted.is_set() else "completed"))
    except Exception as error:
        note("error", error=dict(message=str(error)))
        note("turn/completed", turn=dict(id=f"remote-{number}", status="failed"))


for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method is None:
        replies.put(message)
        continue
    ident = message.get("id")
    params = message.get("params", {})
    result = {}
    if method == "account/login/start":
        assert params["type"] == "apiKey" and params["apiKey"] == "tauros-local-protocol-fixture-key"
        api_mode = True
        result = dict(type="apiKey")
    elif method == "account/read":
        result = dict(account=dict(type="apiKey" if api_mode else "chatgpt"))
    elif method == "thread/start":
        assert "features.code_mode_host=true" in sys.argv
        assert "features.shell_tool=false" in sys.argv
        assert "features.unified_exec=false" in sys.argv
        assert params["sandbox"] == "read-only"
        assert params["dynamicTools"]
        assert params["cwd"] == os.getcwd()
        servers = params["config"]["mcp_servers"]
        result = dict(thread=dict(id="fixture-thread"), model="synthetic-code-mode-model")
    elif method == "mcpServerStatus/list":
        assert params["threadId"] == "fixture-thread"
        assert params["serverName"] == "tauros"
        result = dict(data=[dict(name="tauros", runtimeStatus="connected",
                                tools={"tauros_status": dict(name="tauros_status")}, toolsError=None)])
    elif method == "model/list":
        # Two pages; hidden models must not be offered.
        if params.get("cursor") is None:
            result = dict(data=[dict(id="p1", model="synthetic-code-mode-model", displayName="Synthetic", description="fixture default",
                                     hidden=False, isDefault=True),
                                dict(id="p2", model="hidden-model", displayName="Hidden", description="", hidden=True, isDefault=False)],
                          nextCursor="page-2")
        else:
            assert params["cursor"] == "page-2"
            result = dict(data=[dict(id="p3", model="synthetic-large", displayName="Synthetic Large", description="fixture alternative",
                                     hidden=False, isDefault=False)], nextCursor=None)
    elif method == "turn/start":
        assert params["threadId"] == "fixture-thread"
        turn_model = params.get("model") or turn_model
        assert params["sandboxPolicy"] == dict(type="readOnly", networkAccess=False)
        turn_count += 1
        interrupted.clear()
        result = dict(turn=dict(id=f"remote-{turn_count}"))
        emit(dict(id=ident, result=result))
        note("turn/started", turn=result["turn"])
        threading.Thread(target=run_turn, args=(params["input"][0]["text"], turn_count), daemon=True).start()
        continue
    elif method == "turn/interrupt":
        interrupted.set()
    if ident is not None:
        emit(dict(id=ident, result=result))
