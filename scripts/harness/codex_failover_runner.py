#!/usr/bin/env python3
"""Run remediation with Codex auth failover (primary -> secondary)."""

from __future__ import annotations

import argparse
import base64
import json
import os
import stat
import subprocess
import tempfile
from pathlib import Path
from typing import Any

RATE_LIMIT_SIGNATURES = (
    "you've hit your usage limit",
    "rate limit",
    "429",
    "too many requests",
    "try again at",
)


def _write_json(path: str, payload: dict[str, Any]) -> None:
    out = Path(path)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _read_json(path: str) -> dict[str, Any]:
    p = Path(path)
    if not p.exists():
        return {}
    try:
        raw = json.loads(p.read_text(encoding="utf-8"))
        if isinstance(raw, dict):
            return raw
    except Exception:
        return {}
    return {}


def _detect_rate_limit(*messages: str) -> bool:
    merged = "\n".join(messages).lower()
    return any(signature in merged for signature in RATE_LIMIT_SIGNATURES)


def _auth_from_env() -> tuple[str, str]:
    primary = os.environ.get("CODEX_AUTH_JSON_B64_PRIMARY", "").strip()
    legacy = os.environ.get("CODEX_AUTH_JSON_B64_LEGACY", "").strip()
    secondary = os.environ.get("CODEX_AUTH_JSON_B64_SECONDARY", "").strip()
    resolved_primary = primary or legacy
    return resolved_primary, secondary


def _install_auth(auth_b64: str) -> None:
    home = Path.home()
    codex_home = home / ".codex"
    codex_home.mkdir(parents=True, exist_ok=True)
    auth_path = codex_home / "auth.json"
    try:
        decoded = base64.b64decode(auth_b64, validate=True).decode("utf-8")
    except Exception as exc:
        raise ValueError("failed to decode auth payload") from exc
    if not decoded.strip():
        raise ValueError("failed to decode auth payload")
    auth_path.write_text(decoded, encoding="utf-8")
    auth_path.chmod(stat.S_IRUSR | stat.S_IWUSR)


def _build_runner_cmd(
    *,
    findings: str,
    head_sha: str,
    contract: str,
    result_out: str,
    apply_cmd: str,
    validation_cmds: list[str],
    attempt_log: str,
    max_attempts: int,
) -> list[str]:
    cmd = [
        "python3",
        "scripts/harness/remediation_runner.py",
        "--findings",
        findings,
        "--head-sha",
        head_sha,
        "--contract",
        contract,
        "--result-out",
        result_out,
        "--apply-cmd",
        apply_cmd,
        "--attempt-log",
        attempt_log,
    ]
    if max_attempts > 0:
        cmd.extend(["--max-attempts", str(max_attempts)])
    for item in validation_cmds:
        cmd.extend(["--validation-cmd", item])
    return cmd


def _run_runner(cmd: list[str]) -> tuple[int, dict[str, Any], str]:
    proc = subprocess.run(cmd, text=True, capture_output=True, check=False)
    result_path = cmd[cmd.index("--result-out") + 1]
    payload = _read_json(result_path)
    errors = payload.get("errors", [])
    error_text = "\n".join(str(item) for item in errors if isinstance(item, str))
    combined = "\n".join(part for part in (proc.stdout, proc.stderr, error_text) if part)
    return proc.returncode, payload, combined


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run remediation with Codex primary->secondary failover")
    parser.add_argument("--findings", required=True, help="Findings JSON path")
    parser.add_argument("--head-sha", required=True, help="Current head SHA")
    parser.add_argument("--contract", default=".harness/policy.contract.json", help="Harness contract path")
    parser.add_argument("--result-out", default="artifacts/remediation-result.json", help="Remediation result path")
    parser.add_argument("--failover-out", default="artifacts/codex-failover-result.json", help="Failover metadata path")
    parser.add_argument("--apply-cmd", required=True, help="Remediation apply command")
    parser.add_argument(
        "--validation-cmd",
        action="append",
        default=[],
        help="Validation command to run after apply (repeatable)",
    )
    parser.add_argument(
        "--attempt-log",
        default=".harness/state/remediation-attempts.json",
        help="Remediation attempts log path",
    )
    parser.add_argument("--max-attempts", type=int, default=0, help="Max attempts override")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    primary_auth, secondary_auth = _auth_from_env()

    failover: dict[str, Any] = {
        "used_primary": False,
        "used_secondary": False,
        "failover_triggered": False,
        "primary_exit_code": -1,
        "secondary_exit_code": None,
        "final_exit_code": 1,
        "final_account": "none",
        "trigger_reason": "none",
        "rate_limit_detected": False,
    }

    if not primary_auth:
        _write_json(
            args.result_out,
            {
                "head_sha_in": args.head_sha,
                "head_sha_out": args.head_sha,
                "applied": False,
                "files_touched": [],
                "validation_passed": False,
                "errors": ["missing CODEX_AUTH_JSON_B64_PRIMARY and legacy CODEX_AUTH_JSON_B64 secret"],
            },
        )
        _write_json(args.failover_out, failover)
        return 1

    failover["used_primary"] = True
    try:
        _install_auth(primary_auth)
    except Exception:
        _write_json(
            args.result_out,
            {
                "head_sha_in": args.head_sha,
                "head_sha_out": args.head_sha,
                "applied": False,
                "files_touched": [],
                "validation_passed": False,
                "errors": ["failed to decode/install primary Codex auth payload"],
            },
        )
        _write_json(args.failover_out, failover)
        return 1

    primary_cmd = _build_runner_cmd(
        findings=args.findings,
        head_sha=args.head_sha,
        contract=args.contract,
        result_out=args.result_out,
        apply_cmd=args.apply_cmd,
        validation_cmds=args.validation_cmd,
        attempt_log=args.attempt_log,
        max_attempts=args.max_attempts,
    )
    primary_code, _, primary_text = _run_runner(primary_cmd)
    failover["primary_exit_code"] = primary_code
    failover["final_exit_code"] = primary_code
    failover["final_account"] = "primary" if primary_code == 0 else "none"

    if primary_code == 0:
        _write_json(args.failover_out, failover)
        return 0

    rate_limited = _detect_rate_limit(primary_text)
    failover["rate_limit_detected"] = rate_limited
    if not rate_limited:
        _write_json(args.failover_out, failover)
        return primary_code

    if not secondary_auth:
        failover["trigger_reason"] = "secondary_missing"
        _write_json(args.failover_out, failover)
        return primary_code

    failover["trigger_reason"] = "rate_limit"
    failover["failover_triggered"] = True
    failover["used_secondary"] = True

    try:
        _install_auth(secondary_auth)
    except Exception:
        _write_json(
            args.result_out,
            {
                "head_sha_in": args.head_sha,
                "head_sha_out": args.head_sha,
                "applied": False,
                "files_touched": [],
                "validation_passed": False,
                "errors": ["failed to decode/install secondary Codex auth payload"],
            },
        )
        failover["secondary_exit_code"] = 1
        failover["final_exit_code"] = 1
        failover["final_account"] = "none"
        _write_json(args.failover_out, failover)
        return 1

    # Keep same-sha attempt policy stable: secondary retry uses isolated attempt-log.
    with tempfile.NamedTemporaryFile(prefix="codex-secondary-attempt-log-", suffix=".json") as tmp:
        Path(tmp.name).write_text("{}\n", encoding="utf-8")
        secondary_cmd = _build_runner_cmd(
            findings=args.findings,
            head_sha=args.head_sha,
            contract=args.contract,
            result_out=args.result_out,
            apply_cmd=args.apply_cmd,
            validation_cmds=args.validation_cmd,
            attempt_log=tmp.name,
            max_attempts=args.max_attempts,
        )
        secondary_code, _, _ = _run_runner(secondary_cmd)

    failover["secondary_exit_code"] = secondary_code
    failover["final_exit_code"] = secondary_code
    failover["final_account"] = "secondary" if secondary_code == 0 else "none"
    _write_json(args.failover_out, failover)
    return secondary_code


if __name__ == "__main__":
    raise SystemExit(main())
