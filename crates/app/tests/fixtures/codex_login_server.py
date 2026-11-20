#!/usr/bin/python3
"""Original synthetic login protocol peer; never contacts an auth provider."""
import json
from pathlib import Path
import sys

home = Path.cwd().parent
mode = (home / "login-mode").read_text().strip()
signed_in = mode == "already"


def emit(value):
    print(json.dumps(value), flush=True)


for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    params = msg.get("params", {})
    result = {}
    if method == "account/read":
        assert params == {"refreshToken": False}
        result = {"account": {"type": "chatgpt", "email": "PRIVATE_ACCOUNT_SENTINEL"} if signed_in else None}
    elif method == "account/login/start":
        assert params == {"type": "chatgpt"}, "API login must never be requested"
        result = {"type": "chatgpt", "loginId": "attempt", "authUrl": "https://auth.openai.com/oauth/authorize?state=PRIVATE_URL_SENTINEL"}
        if mode == "unsafe":
            result["authUrl"] = "https://evil.test/authorize"
        if mode in ("success", "failure", "stale"):
            signed_in = mode == "success"
            # Notification deliberately precedes the RPC response.
            emit({"method": "account/login/completed", "params": {
                "loginId": "other-attempt" if mode == "stale" else "attempt",
                "success": mode != "failure", "error": "PRIVATE_ERROR_SENTINEL"}})
    elif method == "account/login/cancel":
        assert params == {"loginId": "attempt"}
        (home / "cancelled").write_text("yes")
        emit({"method": "account/login/completed", "params": {"loginId": "attempt", "success": False}})
    elif method in ("thread/start", "turn/start"):
        raise AssertionError("login must not start model turns")
    if "id" in msg:
        emit({"id": msg["id"], "result": result})
        if method == "account/login/start" and mode == "eof":
            sys.exit(0)
