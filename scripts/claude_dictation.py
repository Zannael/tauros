#!/usr/bin/env python3
"""Experimental draft-only bridge to the pinned Claude interactive CLI.

The official process handles sign-in and speech. Only controlled keystrokes reach
its PTY; no Enter, prompt, user command or transcript is submitted to the agent.
JSON lines on stdin: finish/cancel. JSON lines on stdout: recording/text/error.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import shutil
import signal
import stat
import struct
import subprocess
import sys
import tempfile
import termios
import time
import fcntl
from datetime import datetime, timezone

CLI_SHA256 = "ea0f135784631619a7a409d07322ecec964f9cc4fd377508ed1ba3f141f27912"
MARKER = "Tauros unsent draft handshake 7f93"
MAX_TEXT = 65536


class Diagnostics:
    """Retain only fixed event names and numeric counters, never provider text."""
    def __init__(self, directory=None, operation="dictation"):
        data = Path(os.environ.get("TAUROS_DATA", str(Path.home() / ".local/share/tauros")))
        names = {"dictation": "last-claude-dictation.json", "draft_probe": "last-claude-draft-probe.json",
                 "availability_probe": "last-claude-availability-probe.json"}
        self.operation = operation
        self.path = Path(directory or data / "logs/voice") / names[operation]
        self.phase = "preflight"
        self.counts = {}
        self.events = []
        self.pending = b""

    def event(self, code, amount=1):
        self.counts[code] = min(1_000_000_000, self.counts.get(code, 0) + amount)
        self.events = (self.events + [code])[-24:]

    def feed(self, data):
        self.pending += data
        lines = self.pending.split(b"\n")
        self.pending = lines.pop()[-4096:]
        for raw in lines:
            line = raw.decode("utf-8", "replace")
            for fragment, code in [
                ("[voice] Starting recording session", "capture_start"),
                ("[voice] Recording stopped", "capture_stop"),
                ("[voice] Recording failed", "capture_error"),
                ("[voice] Recording not available", "capture_unavailable"),
                ("[voice] circuit breaker", "capture_breaker"),
                ("[voice_stream] WebSocket connected", "stream_open"),
                ("[voice_stream] WebSocket closed", "stream_closed"),
                ("[voice_stream] Server error", "stream_error"),
                ("[voice_stream] TranscriptError", "stream_error"),
                ("[voice] voice_stream error", "stream_error"),
                ("[voice] Failed to connect to voice_stream", "stream_auth_error"),
                ("[voice] onTranscript:", "transcript_event"),
            ]:
                if fragment in line:
                    self.event(code)
            match = re.search(r"\[voice_stream\] Sending audio chunk: (\d+) bytes", line)
            if match:
                self.event("audio_chunks")
                self.event("audio_bytes", min(int(match[1]), 1_000_000))
            match = re.search(r"\[voice\] Final transcript assembled \((\d+) chars\)", line)
            if match:
                self.event("final_transcripts")
                self.event("final_chars", min(int(match[1]), MAX_TEXT))

    def save(self, result):
        value = {"schema": 1, "operation": self.operation, "utc": datetime.now(timezone.utc).isoformat(),
                 "phase": self.phase, "result": result,
                 "counters": self.counts, "events": self.events}
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            with tempfile.NamedTemporaryFile(mode="w", dir=self.path.parent, delete=False) as stream:
                pending = Path(stream.name)
                json.dump(value, stream)
                stream.write("\n")
            pending.replace(self.path)
        except OSError:
            pass  # A logging failure must not replace the actual voice failure.


def emit(**value):
    print(json.dumps(value), flush=True)


def editor(source, output):
    path = Path(source)
    resolved = path.resolve()
    # The vendor's external-editor file is private temporary input, never auth.
    if path.is_symlink() or not resolved.is_relative_to(Path("/tmp")):
        raise ValueError("Unexpected external-editor input")
    with resolved.open("rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise ValueError("External-editor input is not a regular file")
        data = stream.read(MAX_TEXT + 1)
    if len(data) > MAX_TEXT:
        raise ValueError("Draft is too large")
    data.decode("utf-8")
    target = Path(output)
    temporary = target.with_suffix(".pending")
    temporary.write_bytes(data)
    temporary.replace(target)


def binary_path():
    candidate = os.environ.get("TAUROS_CLAUDE_VOICE_BIN") or shutil.which("claude")
    if not candidate:
        raise ValueError("Claude Code is not installed.")
    binary = Path(candidate).resolve()
    digest = hashlib.sha256()
    with binary.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    if digest.hexdigest() != CLI_SHA256:
        raise ValueError("This experiment requires the verified Claude Code 2.1.304 binary.")
    return binary


def sandbox(root, binary, offline, audio=True, auth_status=False):
    command = ["/usr/bin/bwrap", "--unshare-all", "--die-with-parent"]
    if not offline and not auth_status:
        command += ["--share-net"]
    for location in ["/usr", "/lib", "/lib64", "/etc/ssl", "/etc/resolv.conf", "/etc/hosts"]:
        if Path(location).exists():
            command += ["--ro-bind", location, location]
    command += ["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
                "--bind", str(root), "/work", "--dir", "/runtime",
                "--ro-bind", str(binary), "/runtime/claude",
                "--ro-bind", str(Path(__file__).resolve()), "/runtime/bridge.py",
                "--chdir", "/work", "--clearenv"]
    environment = {"HOME": "/work", "PATH": "/usr/bin:/bin", "TERM": "xterm-256color",
                   "VISUAL": "/usr/bin/python3 /runtime/bridge.py --editor /work/export.txt",
                   "EDITOR": "/usr/bin/python3 /runtime/bridge.py --editor /work/export.txt",
                   "DISABLE_AUTOUPDATER": "1", "DISABLE_TELEMETRY": "1"}
    if offline:
        # No credentials, real audio devices or network in the reproducible probe.
        environment["ANTHROPIC_API_KEY"] = "tauros-offline-probe"
    else:
        auth_dir = Path(os.environ.get("CLAUDE_CONFIG_DIR", str(Path.home() / ".claude"))).resolve()
        credential_file = auth_dir / ".credentials.json"
        if not credential_file.is_file():
            raise ValueError("Claude Code is not signed in. Run: claude auth login --claudeai, then retry Record.")
        # Runtime reads its own credential file; Tauros never reads or copies it.
        command += ["--ro-bind", str(credential_file), "/work/.claude/.credentials.json"]
        for location in (["/dev/snd", "/etc/asound.conf", "/etc/alsa", "/etc/pipewire", "/etc/machine-id"] if audio else []):
            if Path(location).exists():
                command += ["--ro-bind", location, location]
        runtime = Path(os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}"))
        pipewire = runtime / "pipewire-0"
        if audio and pipewire.exists():
            command += ["--ro-bind", str(pipewire), "/audio/pipewire-0"]
            environment["PIPEWIRE_RUNTIME_DIR"] = "/audio"
        pulse = runtime / "pulse/native"
        if audio and pulse.exists():
            command += ["--ro-bind", str(pulse), "/audio/pulse-native"]
            environment["PULSE_SERVER"] = "unix:/audio/pulse-native"
        cookie = Path.home() / ".config/pulse/cookie"
        if audio and cookie.is_file():
            command += ["--ro-bind", str(cookie), "/audio/pulse-cookie"]
            environment["PULSE_COOKIE"] = "/audio/pulse-cookie"
    for name, value in environment.items():
        command += ["--setenv", name, value]
    if auth_status:
        return command + ["/runtime/claude", "auth", "status"]
    settings = {"voice": {"enabled": not offline, "mode": "hold", "autoSubmit": False},
                "language": "italian", "promptSuggestionEnabled": False}
    return command + ["/runtime/claude", "--setting-sources", "", "--tools", "",
                      "--permission-mode", "plan", "--strict-mcp-config",
                      "--mcp-config", '{"mcpServers":{}}', "--settings", json.dumps(settings),
                      "--debug-file", "/dev/stderr"]


def require_subscription_auth(status):
    """Validate only the official runtime's public status, never credential data."""
    if not isinstance(status, dict) or status.get("loggedIn") is not True:
        raise ValueError("Claude Code is not signed in. Run: claude auth login --claudeai, then retry Record.")
    if status.get("authMethod") != "claude.ai":
        raise ValueError("Claude dictation needs Claude subscription sign-in. Run: claude auth login --claudeai, then retry Record.")


def check_sign_in(root, binary):
    try:
        result = subprocess.run(sandbox(root, binary, False, audio=False, auth_status=True),
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=10)
    except subprocess.TimeoutExpired:
        raise ValueError("Claude sign-in check timed out. Run: claude auth status --text.") from None
    try:
        status = json.loads(result.stdout)
    except (ValueError, UnicodeError):
        raise ValueError("Cannot check Claude sign-in. Run: claude auth status --text.") from None
    require_subscription_auth(status)
    if result.returncode != 0:
        raise ValueError("Claude sign-in check failed. Run: claude auth status --text.")


class Session:
    def __init__(self, command, diagnostics=None):
        self.diagnostics = diagnostics or Diagnostics()
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))

        def attach():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        try:
            self.process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=subprocess.PIPE,
                                            preexec_fn=attach)
        finally:
            os.close(slave)
        self.screen = ""

    def write(self, value):
        os.write(self.master, value)

    def pump(self, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            ready = select.select([self.master, self.process.stderr], [], [], min(.05, max(0, deadline-time.monotonic())))[0]
            if self.process.stderr in ready:
                data = os.read(self.process.stderr.fileno(), 65536)
                if data:
                    self.diagnostics.feed(data)
            if self.master not in ready:
                continue
            try:
                data = os.read(self.master, 65536)
            except OSError:
                raise ValueError("Claude CLI closed before returning a draft.") from None
            if not data:
                raise ValueError("Claude CLI closed before returning a draft.")
            if b"\x1b[6n" in data:
                self.write(b"\x1b[1;1R")
            if b"\x1b[c" in data:
                self.write(b"\x1b[?1;2c")
            text = data.decode("utf-8", "replace")
            text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)
            self.screen = (self.screen + text)[-65536:]

    def close(self):
        self.process.terminate()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        os.close(self.master)
        self.process.stderr.close()


def export(session, root, expected=None):
    result = root / "export.txt"
    result.unlink(missing_ok=True)
    session.write(b"\x07")  # Ctrl+G only: never Enter to submit a prompt.
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        session.pump(.1)
        if result.exists():
            text = result.read_text().strip()
            if expected is not None and text != expected:
                raise ValueError("Claude draft handshake did not match; recording was not started.")
            return text
    raise ValueError("Claude did not hand the draft to the external editor.")


class SpeechWarmup:
    """Wait for two seconds of live PCM after the speech connection opens."""

    def __init__(self):
        self.started = None
        self.initial_bytes = 0

    def ready(self, counts, now):
        sent = counts.get("audio_bytes", 0)
        if not counts.get("stream_open") or sent < 3200:
            return False
        if self.started is None:
            self.started = now
            self.initial_bytes = sent
        # 16 kHz mono s16 PCM: require fresh audio as well as elapsed time.
        # Buffered startup chunks alone must never announce readiness.
        return now - self.started >= 2 and sent - self.initial_bytes >= 64000


def run(offline, availability=False, diagnostics=None):
    diagnostics = diagnostics or Diagnostics()
    binary = binary_path()
    if not Path("/usr/bin/bwrap").is_file():
        raise ValueError("This experiment requires bubblewrap on Linux.")
    with tempfile.TemporaryDirectory(prefix="tauros-claude-dictation-") as temporary:
        root = Path(temporary)
        config = root / ".claude"
        config.mkdir()
        # Private, original config; no user hooks, plugins, MCP or project data.
        (root / ".claude.json").write_text(json.dumps({
            "hasCompletedOnboarding": True, "theme": "dark", "numStartups": 1,
            "projects": {"/work": {"hasTrustDialogAccepted": True}},
            "customApiKeyResponses": {"approved": ["tauros-offline-probe"], "rejected": []}}))
        (config / "keybindings.json").write_text(json.dumps({"bindings": [
            {"context": "Chat", "bindings": {"space": None, "meta+k": "voice:pushToTalk"}}]}))
        if not offline:
            # A credential file can exist while the official CLI is logged out.
            # Ask that exact runtime, with neither network nor audio available,
            # before starting a TUI or sending any recording key.
            check_sign_in(root, binary)
            diagnostics.event("subscription_auth_ok")
        diagnostics.phase = "composer"
        session = Session(sandbox(root, binary, offline, audio=not availability), diagnostics)
        try:
            deadline = time.monotonic() + 35
            while "shift+tab" not in session.screen:
                session.pump(.1)
                if time.monotonic() > deadline:
                    raise ValueError("Claude did not reach its composer. Check sign-in in the official CLI.")
            # Prove the editor callback is active before opening the microphone.
            diagnostics.phase = "handshake"
            session.write(MARKER.encode())
            session.pump(.5)
            export(session, root, MARKER)
            if offline:
                diagnostics.phase = "done"
                emit(probe="passed", text=MARKER, network=False, microphone=False)
                return
            session.pump(.5)
            session.write(b"\x15")  # Ctrl+U clears the synthetic readiness marker.
            session.pump(.2)
            # A modifier binding avoids the default Space double-tap-to-submit.
            # No terminal input from the user or transcript is ever forwarded.
            started = time.monotonic()
            diagnostics.phase = "warmup"
            warmup = SpeechWarmup()
            recording_started = None
            session.screen = ""
            active = False
            while recording_started is None or time.monotonic() - recording_started < 60:
                if not availability and select.select([sys.stdin], [], [], 0)[0]:
                    action = sys.stdin.readline().strip()
                    if action != "finish":
                        raise ValueError("Dictation cancelled.")
                    break
                session.write(b"\x1bk")
                session.pump(.05)
                failures = {
                    "Voice mode needs a Claude.ai account": "Claude voice needs an active Claude.ai sign-in.",
                    "Microphone access is denied": "Claude cannot access the microphone.",
                    "No audio detected from microphone": "Claude detected no microphone audio.",
                    "Voice connection failed": "Claude speech connection failed; retry in the official CLI.",
                    "No speech detected": "Claude detected no speech.",
                    "Audio recording is not available": "Claude audio recording is unavailable.",
                    "Voice mode is disabled by your organization": "Claude voice is disabled by organization policy.",
                }
                for fragment, message in failures.items():
                    if fragment in session.screen:
                        raise ValueError(message)
                counts = diagnostics.counts
                if counts.get("capture_error") or counts.get("capture_unavailable") or counts.get("capture_breaker"):
                    raise ValueError("Claude microphone capture failed. See the local voice diagnostic log.")
                if counts.get("capture_stop"):
                    raise ValueError("Claude recorder stopped before Stop was requested. See the local voice diagnostic log.")
                if counts.get("stream_error") or counts.get("stream_auth_error"):
                    raise ValueError("Claude speech connection failed. See the local voice diagnostic log.")
                if not active and warmup.ready(counts, time.monotonic()):
                    active = True
                    recording_started = time.monotonic()
                    diagnostics.phase = "recording"
                    diagnostics.event("speech_ready")
                    if not availability:
                        emit(phase="recording")
                if not active and time.monotonic() - started > 12:
                    if availability:
                        screen = session.screen.lower()
                        emit(probe="availability", microphone=False, recording=False,
                             microphone_error="microphone" in screen or "audio capture" in screen,
                             sign_in_error="sign in" in screen or "claude.ai account" in screen,
                             voice_feedback="voice" in screen,
                             modifier_typed="kkkk" in screen)
                        return
                    raise ValueError("Claude did not start dictation. Verify /voice and sign-in in the official CLI.")
                if availability and active:
                    emit(probe="availability", microphone=False, recording_indicator=True)
                    return
            # Release the hold and allow the pinned client's bounded stream
            # finalization to finish before asking its editor for the draft.
            diagnostics.phase = "finalization"
            session.pump(8)
            diagnostics.phase = "export"
            text = export(session, root)
            if not text or MARKER in text or re.fullmatch(r"[kK\s]+", text):
                raise ValueError("Claude returned no transcript. See the local voice diagnostic log.")
            diagnostics.phase = "done"
            diagnostics.save("completed")
            emit(text=text)
        finally:
            session.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    probes = parser.add_mutually_exclusive_group()
    probes.add_argument("--probe-draft", action="store_true", help="No network, auth or audio")
    probes.add_argument("--probe-availability", action="store_true", help="Official runtime sign-in/voice check with network but no audio devices")
    parser.add_argument("--editor", metavar="OUTPUT")
    parser.add_argument("source", nargs="?")
    args = parser.parse_args()
    if args.editor:
        editor(args.source, args.editor)
        return
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    operation = "draft_probe" if args.probe_draft else "availability_probe" if args.probe_availability else "dictation"
    diagnostics = Diagnostics(operation=operation)
    try:
        run(args.probe_draft, args.probe_availability, diagnostics)
        diagnostics.save("completed")
    except (OSError, ValueError) as error:
        # Never dump a provider screen, environment, exception path or credentials.
        message = str(error) if isinstance(error, ValueError) else "Cannot run the isolated Claude dictation helper."
        emit(error=message)
        diagnostics.save("failed")
        sys.exit(1)
    except SystemExit:
        if diagnostics.phase != "done":
            diagnostics.save("cancelled")
        raise
    except Exception:
        diagnostics.event("helper_exception")
        diagnostics.save("failed")
        emit(error="Unexpected Claude dictation helper failure. See the local voice diagnostic log.")
        sys.exit(1)


if __name__ == "__main__":
    main()
