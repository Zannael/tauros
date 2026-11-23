"""External-editor boundary checks; optional real offline CLI draft probe."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/claude_dictation.py"
spec = importlib.util.spec_from_file_location("claude_dictation", SCRIPT)
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)


class EditorBoundary(unittest.TestCase):
    def test_speech_warmup_requires_connection_elapsed_time_and_fresh_pcm(self):
        warmup = bridge.SpeechWarmup()
        self.assertFalse(warmup.ready({"audio_bytes": 128000}, 0))
        self.assertFalse(warmup.ready({"stream_open": 1, "audio_bytes": 0}, 1))
        self.assertFalse(warmup.ready({"stream_open": 1, "audio_bytes": 128000}, 2))
        self.assertFalse(warmup.ready({"stream_open": 1, "audio_bytes": 192000}, 3))
        self.assertFalse(warmup.ready({"stream_open": 1, "audio_bytes": 128000}, 4))
        self.assertTrue(warmup.ready({"stream_open": 1, "audio_bytes": 192000}, 4))

    def test_runtime_diagnostics_keep_counters_and_discard_raw_content(self):
        with tempfile.TemporaryDirectory() as temporary:
            diagnostics = bridge.Diagnostics(temporary)
            raw = (b"[voice] Starting recording session\n"
                   b"private-account-and-transcript-content\n"
                   b"[voice_stream] Sending audio chunk: 640 bytes\n"
                   b"[voice_stream] WebSocket connected\n"
                   b"[voice] Final transcript assembled (18 chars)\n")
            diagnostics.feed(raw[:38])
            diagnostics.feed(raw[38:])
            diagnostics.save("failed")
            value = json.loads(diagnostics.path.read_text())
            self.assertEqual(value["counters"]["audio_bytes"], 640)
            self.assertEqual(value["counters"]["stream_open"], 1)
            self.assertEqual(value["counters"]["final_chars"], 18)
            self.assertNotIn("private-account-and-transcript-content", diagnostics.path.read_text())
            self.assertEqual(diagnostics.path.stat().st_mode & 0o777, 0o600)

    def test_official_auth_status_requires_subscription_and_never_echoes_account_fields(self):
        for value in [{}, {"loggedIn": False, "authMethod": "none"},
                      {"loggedIn": True, "authMethod": "api_key"},
                      {"loggedIn": True, "authMethod": "oauth_token"}]:
            with self.subTest(value=value):
                value["email"] = "private-account-field"
                with self.assertRaises(ValueError) as caught:
                    bridge.require_subscription_auth(value)
                self.assertIn("claude auth login --claudeai", str(caught.exception))
                self.assertNotIn("private-account-field", str(caught.exception))
        bridge.require_subscription_auth({"loggedIn": True, "authMethod": "claude.ai"})

    def test_preserves_unicode_draft_without_editing_the_cli_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, output = root / "prompt.txt", root / "export.txt"
            text = "Mostra la città.\nNon inviare ancora."
            source.write_text(text)
            bridge.editor(source, output)
            self.assertEqual(output.read_text(), text)
            self.assertEqual(source.read_text(), text)

    def test_rejects_symlinks_and_oversized_input(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, output = root / "prompt.txt", root / "export.txt"
            source.write_text("private input")
            link = root / "link.txt"
            link.symlink_to(source)
            with self.assertRaises(ValueError):
                bridge.editor(link, output)
            source.write_bytes(b"a" * (bridge.MAX_TEXT + 1))
            with self.assertRaises(ValueError):
                bridge.editor(source, output)
            self.assertFalse(output.exists())

    @unittest.skipUnless(os.environ.get("TAUROS_CLAUDE_DRAFT_PROBE") == "1", "explicit offline runtime opt-in")
    def test_real_cli_unsent_draft_and_cancel_cleanup(self):
        result = subprocess.run(["python3", str(SCRIPT), "--probe-draft"],
                                capture_output=True, text=True, timeout=55, check=True)
        value = json.loads(result.stdout)
        self.assertEqual(value, {"probe": "passed", "text": bridge.MARKER,
                                 "network": False, "microphone": False})
        before = set(Path(tempfile.gettempdir()).glob("tauros-claude-dictation-*"))
        process = subprocess.Popen(["python3", str(SCRIPT), "--probe-draft"],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if set(Path(tempfile.gettempdir()).glob("tauros-claude-dictation-*")) - before:
                    break
                time.sleep(.05)
            process.terminate()
            process.communicate(timeout=8)
            self.assertFalse(set(Path(tempfile.gettempdir()).glob("tauros-claude-dictation-*")) - before)
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()


if __name__ == "__main__":
    unittest.main()
