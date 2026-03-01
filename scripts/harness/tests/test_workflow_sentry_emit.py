#!/usr/bin/env python3

from __future__ import annotations

import http.server
import json
import os
import socketserver
import subprocess
import tempfile
import threading
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "scripts/harness/workflow_sentry_emit.py"


class _EnvelopeHandler(http.server.BaseHTTPRequestHandler):
    request_count = 0

    def do_POST(self) -> None:  # noqa: N802
        _EnvelopeHandler.request_count += 1
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, fmt: str, *args) -> None:  # noqa: A003
        return


class _ErrorEnvelopeHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self) -> None:  # noqa: N802
        self.send_response(429)
        self.end_headers()
        self.wfile.write(b"rate-limited")

    def log_message(self, fmt: str, *args) -> None:  # noqa: A003
        return


class WorkflowSentryEmitTests(unittest.TestCase):
    def _run_emit(self, extra_args: list[str], env: dict[str, str] | None = None) -> tuple[int, dict[str, object], str, str]:
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / "emit.json"
            cmd = [
                "python3",
                str(SCRIPT),
                "--workflow",
                "unit-test-workflow",
                "--job",
                "unit-test-job",
                "--run-id",
                "123",
                "--run-attempt",
                "1",
                "--event",
                "pull_request",
                "--repo",
                "owner/repo",
                "--head-sha",
                "deadbeefcafebabe",
                "--pr-number",
                "9",
                "--status",
                "failure",
                "--emit",
                "false",
                "--out",
                str(out),
            ]
            cmd.extend(extra_args)
            merged_env = os.environ.copy()
            if env:
                merged_env.update(env)
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                cwd=str(REPO_ROOT),
                env=merged_env,
                check=False,
            )
            payload = json.loads(out.read_text(encoding="utf-8"))
            return proc.returncode, payload, proc.stdout, proc.stderr

    def test_classifies_infra_failure(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            infra = Path(td) / "infra.json"
            infra.write_text(json.dumps({"status": "fail"}), encoding="utf-8")
            code, payload, _, _ = self._run_emit([
                "--infra-preflight-report",
                str(infra),
            ])
            self.assertEqual(code, 0)
            self.assertEqual(payload["reason_code"], "infra_preflight_failed")

    def test_classifies_live_provider_failure(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            live = Path(td) / "live.json"
            live.write_text(
                json.dumps({"status": "fail", "risk_tier": "high", "blocking_applies": True}),
                encoding="utf-8",
            )
            code, payload, _, _ = self._run_emit([
                "--live-provider-report",
                str(live),
            ])
            self.assertEqual(code, 0)
            self.assertEqual(payload["reason_code"], "live_provider_gate_failed")

    def test_classifies_risk_policy_failure(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            risk = Path(td) / "risk.json"
            risk.write_text(json.dumps({"decision": "fail"}), encoding="utf-8")
            code, payload, _, _ = self._run_emit([
                "--risk-policy-report",
                str(risk),
            ])
            self.assertEqual(code, 0)
            self.assertEqual(payload["reason_code"], "risk_policy_failed")

    def test_classifies_blocking_eval_failure(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            eval_results = Path(td) / "eval-results.json"
            eval_results.write_text(
                json.dumps(
                    {
                        "profile": "blocking",
                        "summary": {"failed": 2, "all_blocking_passed": False},
                    }
                ),
                encoding="utf-8",
            )
            code, payload, _, _ = self._run_emit([
                "--eval-results",
                str(eval_results),
            ])
            self.assertEqual(code, 0)
            self.assertEqual(payload["reason_code"], "agent_blocking_evals_failed")

    def test_classifies_codex_rate_limit_exhaustion(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            failover = Path(td) / "codex-failover.json"
            failover.write_text(
                json.dumps({"final_exit_code": 1, "rate_limit_detected": True}),
                encoding="utf-8",
            )
            code, payload, _, _ = self._run_emit([
                "--codex-failover-result",
                str(failover),
            ])
            self.assertEqual(code, 0)
            self.assertEqual(payload["reason_code"], "codex_rate_limit_exhausted")

    def test_classifies_unknown_failure_when_no_reports(self) -> None:
        code, payload, _, _ = self._run_emit([])
        self.assertEqual(code, 0)
        self.assertEqual(payload["reason_code"], "workflow_failed_unknown")

    def test_missing_dsn_is_advisory(self) -> None:
        code, payload, _, _ = self._run_emit([
            "--emit",
            "true",
            "--sentry-dsn-env",
            "MISSING_DSN_ENV",
        ])
        self.assertEqual(code, 0)
        self.assertFalse(payload["sent"])
        self.assertIn("missing env MISSING_DSN_ENV", str(payload["detail"]))

    def test_transport_success_sends_event(self) -> None:
        try:
            httpd = socketserver.TCPServer(("127.0.0.1", 0), _EnvelopeHandler)
        except PermissionError:
            self.skipTest("socket bind not permitted in this environment")

        with httpd:
            _EnvelopeHandler.request_count = 0
            port = httpd.server_address[1]
            thread = threading.Thread(target=httpd.serve_forever, daemon=True)
            thread.start()
            dsn = f"http://public@127.0.0.1:{port}/42"
            code, payload, _, _ = self._run_emit(
                [
                    "--emit",
                    "true",
                    "--sentry-dsn-env",
                    "TEST_SENTRY_DSN",
                ],
                env={"TEST_SENTRY_DSN": dsn},
            )
            httpd.shutdown()
            thread.join(timeout=5)

        self.assertEqual(code, 0)
        self.assertTrue(payload["sent"])
        self.assertGreaterEqual(_EnvelopeHandler.request_count, 1)

    def test_transport_http_error_is_advisory(self) -> None:
        try:
            httpd = socketserver.TCPServer(("127.0.0.1", 0), _ErrorEnvelopeHandler)
        except PermissionError:
            self.skipTest("socket bind not permitted in this environment")

        with httpd:
            port = httpd.server_address[1]
            thread = threading.Thread(target=httpd.serve_forever, daemon=True)
            thread.start()
            dsn = f"http://public@127.0.0.1:{port}/42"
            code, payload, _, _ = self._run_emit(
                [
                    "--emit",
                    "true",
                    "--sentry-dsn-env",
                    "TEST_SENTRY_DSN",
                ],
                env={"TEST_SENTRY_DSN": dsn},
            )
            httpd.shutdown()
            thread.join(timeout=5)

        self.assertEqual(code, 0)
        self.assertFalse(payload["sent"])
        self.assertIn("http_error=429", str(payload["detail"]))


if __name__ == "__main__":
    unittest.main()
