#!/usr/bin/python3
"""Original network probe inside Codex's production namespace. No auth/model calls."""
import json
from pathlib import Path
import socket
import sys
import urllib.error
import urllib.request

home = Path.cwd().parent

for line in sys.stdin:
    message = json.loads(line)
    result = {}
    if message.get("method") == "account/read":
        assert Path("/etc/resolv.conf").is_file(), "system resolver is missing"
        report = {"resolver_available": True}
        if (home / "online-probe").exists():
            # Public unauthenticated GET. Do not send an OAuth code, token, API
            # key, cookie, game source or model request, and do not read bodies.
            socket.getaddrinfo("auth.openai.com", 443, type=socket.SOCK_STREAM)
            report["dns"] = True
            try:
                response = urllib.request.urlopen("https://auth.openai.com/oauth/token", timeout=15)
            except urllib.error.HTTPError as response:
                report["https_status"] = response.code
                response.close()
            else:
                report["https_status"] = response.status
                response.close()
            report["tls_verified"] = True
        (home / "network-result.json").write_text(json.dumps(report))
        result = {"account": None}
    elif message.get("method") in ("thread/start", "turn/start", "account/login/start"):
        raise AssertionError("network probe must never authenticate or start inference")
    if "id" in message:
        print(json.dumps({"id": message["id"], "result": result}), flush=True)
