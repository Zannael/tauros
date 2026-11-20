#!/usr/bin/python3
"""Original deterministic ACP peer standing in for `agy acp`; never an inference
or auth substitute. It launches the stdio MCP servers Tauros attaches, so tool
calls reach the real in-process Tauros server and project native tools. Every
tool call first asks `session/request_permission`, as Antigravity does, and only
runs when Tauros allows it.
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
cancelled = threading.Event()
sessions = {}
turns = 0
next_id = 1000
authenticated = False
# Login tests put the mode next to the dedicated workspace; absent means signed in.
mode_file = os.path.join(os.path.dirname(os.getcwd()), "login-mode")
login_mode = open(mode_file).read().strip() if os.path.exists(mode_file) else "already"


def finish_login(ident):
    global authenticated
    threading.Event().wait(0.3)
    authenticated = True
    emit(dict(id=ident, result=dict(_meta=dict(antigravity=dict(tier=dict(name="PRIVATE_TIER"))))))


def emit(value):
    with lock:
        print(json.dumps(dict(jsonrpc="2.0", **value)), flush=True)


def update(session, **update):
    emit(dict(method="session/update", params=dict(sessionId=session, update=update)))


class Mcp:
    def __init__(self, cfg):
        env = dict(os.environ, **{e["name"]: e["value"] for e in cfg["env"]})
        self.child = subprocess.Popen([cfg["command"], *cfg["args"]], stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, text=True, env=env)
        self.ident = 0
        self.rpc("initialize", dict(protocolVersion="2025-06-18", capabilities={},
                                    clientInfo=dict(name="acp-fixture", version="1")))
        self.child.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
        self.child.stdin.flush()
        self.tools = [t["name"] for t in self.rpc("tools/list", {})["tools"]]

    def rpc(self, method, params):
        self.ident += 1
        self.child.stdin.write(json.dumps(dict(jsonrpc="2.0", id=self.ident, method=method, params=params)) + "\n")
        self.child.stdin.flush()
        while True:
            assert select.select([self.child.stdout], [], [], 30)[0], "MCP response timed out"
            reply = json.loads(self.child.stdout.readline())
            if reply.get("id") == self.ident:
                assert "error" not in reply, reply
                return reply["result"]

    def close(self):
        self.child.terminate()
        self.child.wait(timeout=5)


def tool(session, name, args, kind="other", run=None):
    """Announce a call, ask permission, run it only when allowed."""
    global next_id
    call = f"call-{next_id}"
    next_id += 1
    info = dict(toolCallId=call, name=name, kind=kind, title=name, rawInput=args)
    update(session, sessionUpdate="tool_call", status="pending", **info)
    emit(dict(id=next_id, method="session/request_permission", params=dict(sessionId=session, toolCall=info, options=[
        dict(optionId="allow", name="Allow", kind="allow_once"),
        dict(optionId="always", name="Always", kind="allow_always"),
        dict(optionId="deny", name="Deny", kind="reject_once")])))
    reply = replies.get(timeout=15)
    assert reply["id"] == next_id, reply
    outcome = reply["result"]["outcome"]
    allowed = outcome == dict(outcome="selected", optionId="allow")
    assert allowed or outcome in (dict(outcome="selected", optionId="deny"), dict(outcome="cancelled")), outcome
    result = None
    if allowed and run:
        result = run()
    failed = not allowed or result is None or result.get("isError")
    update(session, sessionUpdate="tool_call_update", status="failed" if failed else "completed",
           content=[dict(type="content", content=dict(type="text", text=json.dumps(result)))], **info)
    return allowed, result


def mcp_call(session, server, name, args):
    mcp = sessions[session]["mcp"][server]
    return tool(session, f"mcp_{server}_{name}", args,
                run=lambda: mcp.rpc("tools/call", dict(name=name, arguments=args)))


def run_turn(ident, session, text):
    global turns
    turns += 1
    number = turns
    try:
        state = sessions[session]
        if number == 1:
            assert "<tauros-instructions>" in text, "first turn lacks the Tauros instructions"
        else:
            assert "<tauros-instructions>" not in text, "instructions repeated"
        text = text.rsplit("\n", 1)[-1]
        if text.startswith("edit:") or text.startswith("reject:"):
            root = text.split(":", 1)[1]
            assert os.getcwd() != root and not os.path.exists("scene.c"), "game tree used as agent workspace"
            # Built-in tools are refused even for paths inside the game project.
            for name, kind in [("write_to_file", "edit"), ("run_command", "execute"), ("read_file", "read"),
                               ("mcp_other_tool", "other")]:
                allowed, _ = tool(session, name, dict(path=root + "/scene.c"), kind)
                assert not allowed, f"{name} was allowed"
            allowed, read = mcp_call(session, "project", "tauros_read_file", dict(path="scene.c"))
            assert allowed and not read.get("isError"), read
            allowed, write = mcp_call(session, "project", "tauros_write_file",
                                      dict(path="scene.c", content="edited by Antigravity fixture\n"))
            assert allowed and bool(write.get("isError")) == text.startswith("reject:"), write
            if text.startswith("edit:"):
                allowed, escape = mcp_call(session, "project", "tauros_write_file", dict(path="../escape.c", content="bad"))
                assert allowed and escape.get("isError"), escape
                allowed, trainer = mcp_call(session, "tauros", "create_trainer", dict(
                    id="TRAINER_AGY_FIXTURE", name="Antigravity", **{"class": "Youngster"},
                    party=[dict(species="SPECIES_TEST", level=7)]))
                assert allowed and not trainer.get("isError"), trainer
        elif text == "wait":
            cancelled.wait(15)
            # Late prompts after cancellation must be answered `cancelled`.
            allowed, _ = mcp_call(session, "tauros", "tauros_status", {})
            if allowed:
                os._exit(3)  # A cancelled turn ends without an error report.
        elif text == "continue":
            assert number == 2, "session continuity lost"
        elif text == "crash":
            os._exit(1)
        update(session, sessionUpdate="agent_thought_chunk", content=dict(type="text", text="checked"))
        update(session, sessionUpdate="agent_message_chunk", content=dict(type="text", text=f"turn {number}"))
        reason = "cancelled" if cancelled.is_set() else "end_turn"
        emit(dict(id=ident, result=dict(stopReason=reason, usage=dict(inputTokens=3, outputTokens=2))))
    except Exception as error:
        emit(dict(id=ident, error=dict(code=-32603, message=f"fixture: {error}")))


assert sys.argv[1] == "acp" and "--gemini-dir" in sys.argv and "--disable-telemetry" in sys.argv
for name in ("GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_APPLICATION_CREDENTIALS", "AGY_ADC_AUTH"):
    assert name not in os.environ, f"{name} leaked to the runtime"
for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method is None:
        replies.put(message)
        continue
    ident = message.get("id")
    params = message.get("params", {})
    result = {}
    if method == "initialize":
        assert params["protocolVersion"] == 1
        assert params["clientCapabilities"] == dict(fs=dict(readTextFile=False, writeTextFile=False), terminal=False)
        result = dict(protocolVersion=1, agentCapabilities=dict(loadSession=True), authMethods=[])
    elif method == "session/new":
        if "--fixture-auth-required" in sys.argv or (login_mode != "already" and not authenticated):
            emit(dict(id=ident, error=dict(code=-32000, message="Authentication required")))
            continue
        assert params["cwd"] == os.getcwd()
        session = f"session-{len(sessions) + 1}"
        sessions[session] = dict(mcp={s["name"]: Mcp(s) for s in params["mcpServers"]}, mode=None)
        result = dict(sessionId=session)
    elif method == "authenticate":
        # The real runtime opens the browser itself; no URL is ever sent to the client.
        assert params == dict(methodId="oauth-personal"), params
        with open(os.path.join(os.path.dirname(os.getcwd()), "login-started"), "w") as marker:
            marker.write(str(os.getpid()))
        if login_mode == "success":
            threading.Thread(target=finish_login, args=(ident,), daemon=True).start()
        elif login_mode == "failure":
            emit(dict(id=ident, error=dict(code=-32603, message="PRIVATE_failure detail")))
        continue  # "pending": never answers until terminated
    elif method == "session/set_mode":
        assert params["modeId"] == "default"
        sessions[params["sessionId"]]["mode"] = params["modeId"]
    elif method == "session/prompt":
        assert sessions[params["sessionId"]]["mode"] == "default"
        cancelled.clear()
        threading.Thread(target=run_turn, args=(ident, params["sessionId"], params["prompt"][0]["text"]),
                         daemon=True).start()
        continue
    elif method == "session/cancel":
        cancelled.set()
        continue
    elif method == "session/close":
        for mcp in sessions.pop(params["sessionId"])["mcp"].values():
            mcp.close()
    if ident is not None:
        emit(dict(id=ident, result=result))
