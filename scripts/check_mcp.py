#!/usr/bin/env python3
"""Exercise a real Tauros stdio process with a stdlib-only MCP client."""
import argparse
import json
from pathlib import Path
import selectors
import subprocess


def check(command: list[str]) -> None:
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, text=True, bufsize=1) as child:
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ)

        def send(body):
            child.stdin.write(json.dumps({"jsonrpc": "2.0", **body}) + "\n")
            child.stdin.flush()

        def request(number, method, params=None):
            send({"id": number, "method": method, **({"params": params} if params is not None else {})})
            for _ in range(20):
                if not selector.select(timeout=10):
                    raise RuntimeError(f"timeout waiting for {method}")
                line = child.stdout.readline()
                if not line:
                    raise RuntimeError(f"server exited during {method}")
                message = json.loads(line)
                if message.get("id") == number:
                    return message
            raise RuntimeError("too many unexpected notifications")

        try:
            init = request(1, "initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                           "clientInfo": {"name": "tauros-stdlib-check", "version": "0.1.0"}})
            assert init["result"]["serverInfo"]["name"] == "tauros", init
            assert "tools" in init["result"]["capabilities"], init
            send({"method": "notifications/initialized"})
            tools = request(2, "tools/list")["result"]["tools"]
            tool = next(t for t in tools if t["name"] == "tauros_status")
            assert tool["inputSchema"]["type"] == "object", tool
            assert tool["annotations"]["readOnlyHint"], tool
            result = request(3, "tools/call", {"name": "tauros_status", "arguments": {}})
            assert not result["result"].get("isError"), result
            text = result["result"]["content"][0]["text"]
            assert text == "Tauros 0.1.0 ready", result
            unknown = request(4, "tools/call", {"name": "not_a_tauros_tool", "arguments": {}})
            assert "error" in unknown or unknown.get("result", {}).get("isError"), unknown
            assert "result" in request(5, "ping")
            print(f"MCP stdio: initialized, listed tauros_status, called -> {text}; unknown tool rejected; ping passed")
            child.stdin.close()
            assert child.wait(timeout=10) == 0, child.stderr.read()
        finally:
            selector.close()
            if child.poll() is None:
                child.kill()
                child.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", nargs="*", default=[str(Path(__file__).resolve().parents[1] / "target/debug/tauros-mcp")])
    args = parser.parse_args()
    check(args.command)


if __name__ == "__main__":
    main()
