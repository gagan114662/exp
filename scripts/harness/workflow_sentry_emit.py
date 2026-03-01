#!/usr/bin/env python3
"""Emit sanitized workflow failure telemetry to Sentry (advisory-only)."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import uuid
from pathlib import Path
from typing import Any, Dict, Iterable, Tuple

from sentry_client import send_sentry_event

FAILURE_STATES = {
    "failure",
    "failed",
    "cancelled",
    "canceled",
    "error",
    "timed_out",
    "timed-out",
    "timeout",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Emit workflow failure telemetry to Sentry")
    parser.add_argument("--workflow", required=True, help="Workflow name")
    parser.add_argument("--job", required=True, help="Job name")
    parser.add_argument("--run-id", required=True, help="GitHub run id")
    parser.add_argument("--run-attempt", required=True, help="GitHub run attempt")
    parser.add_argument("--event", required=True, help="GitHub event name")
    parser.add_argument("--repo", required=True, help="owner/repo")
    parser.add_argument("--head-sha", default="", help="Head SHA")
    parser.add_argument("--pr-number", default="", help="PR number")
    parser.add_argument("--status", required=True, help="Workflow/job status")
    parser.add_argument("--emit", default="false", choices=["true", "false"], help="Enable Sentry emission")
    parser.add_argument(
        "--emit-on-failure-only",
        default="true",
        choices=["true", "false"],
        help="Emit only for failure/cancelled status",
    )
    parser.add_argument("--sentry-dsn-env", default="SENTRY_DSN", help="Sentry DSN env var name")
    parser.add_argument("--out", default="artifacts/sentry-workflow-emit.json", help="Output artifact path")

    parser.add_argument("--infra-preflight-report", default="", help="infra preflight report path")
    parser.add_argument("--live-provider-report", default="", help="live provider report path")
    parser.add_argument("--risk-policy-report", default="", help="risk policy report path")
    parser.add_argument("--eval-results", default="", help="eval-results path")
    parser.add_argument("--remediation-result", default="", help="remediation-result path")
    parser.add_argument("--codex-failover-result", default="", help="codex failover result path")
    parser.add_argument("--review-findings", default="", help="review findings path")
    parser.add_argument("--claude-findings", default="", help="claude findings path")
    parser.add_argument("--sentry-findings", default="", help="sentry findings path")
    parser.add_argument("--eval-findings", default="", help="eval findings path")
    return parser.parse_args()


def _read_json(path: str) -> Dict[str, Any]:
    if not path:
        return {}
    p = Path(path)
    if not p.exists():
        return {}
    try:
        payload = json.loads(p.read_text(encoding="utf-8"))
    except Exception:
        return {}
    return payload if isinstance(payload, dict) else {}


def _write_json(path: str, payload: Dict[str, Any]) -> None:
    out = Path(path)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _as_int(value: Any, default: int = 0) -> int:
    try:
        return int(value)
    except Exception:
        return default


def _terminal_failure(status: str) -> bool:
    return status.strip().lower() in FAILURE_STATES


def _count_actionable(findings_payload: Dict[str, Any]) -> int:
    findings = findings_payload.get("findings", [])
    if not isinstance(findings, list):
        return 0
    count = 0
    for item in findings:
        if isinstance(item, dict) and bool(item.get("actionable", False)):
            count += 1
    return count


def _classify_reason(
    *,
    status: str,
    infra: Dict[str, Any],
    live: Dict[str, Any],
    risk: Dict[str, Any],
    eval_results: Dict[str, Any],
    remediation: Dict[str, Any],
    failover: Dict[str, Any],
    findings_payloads: Iterable[Tuple[str, Dict[str, Any]]],
) -> Tuple[str, str, Dict[str, Any]]:
    terminal_failure = _terminal_failure(status)

    infra_status = str(infra.get("status", "missing")).lower()
    live_status = str(live.get("status", "missing")).lower()
    live_blocking = bool(live.get("blocking_applies", False))
    live_tier = str(live.get("risk_tier", "")).lower()
    risk_decision = str(risk.get("decision", "missing")).lower()
    risk_status = str(risk.get("status", "missing")).lower()

    summary = eval_results.get("summary", {}) if isinstance(eval_results.get("summary"), dict) else {}
    eval_failed = _as_int(summary.get("failed", 0), 0)
    all_blocking_passed = bool(summary.get("all_blocking_passed", True))
    eval_profile = str(eval_results.get("profile", ""))

    final_exit = _as_int(failover.get("final_exit_code", remediation.get("exit_code", 0)), 0)
    rate_limited = bool(failover.get("rate_limit_detected", False))

    findings_states: Dict[str, str] = {}
    actionable_by_provider: Dict[str, int] = {}
    for provider, payload in findings_payloads:
        findings_states[provider] = str(payload.get("status", "missing")).lower()
        actionable_by_provider[provider] = _count_actionable(payload)

    inputs: Dict[str, Any] = {
        "terminal_failure": terminal_failure,
        "infra_status": infra_status,
        "live_status": live_status,
        "live_blocking": live_blocking,
        "live_risk_tier": live_tier,
        "risk_decision": risk_decision,
        "risk_status": risk_status,
        "eval_profile": eval_profile,
        "eval_failed": eval_failed,
        "eval_all_blocking_passed": all_blocking_passed,
        "remediation_final_exit_code": final_exit,
        "failover_rate_limit_detected": rate_limited,
        "findings_states": findings_states,
        "actionable_by_provider": actionable_by_provider,
    }

    if infra_status in {"fail", "error"}:
        return "infra_preflight_failed", f"infra preflight reported status={infra_status}", inputs

    if live_status in {"fail", "error"} and (live_blocking or live_tier in {"critical", "high"}):
        return "live_provider_gate_failed", f"live provider gate reported status={live_status}", inputs

    if risk_decision in {"fail", "stale-review", "timeout"} or risk_status in {"fail", "error"}:
        return "risk_policy_failed", f"risk policy decision={risk_decision or risk_status}", inputs

    if eval_profile == "blocking" and (eval_failed > 0 or not all_blocking_passed):
        return "agent_blocking_evals_failed", f"blocking evals failed={eval_failed}", inputs

    if final_exit != 0 and rate_limited:
        return "codex_rate_limit_exhausted", "codex remediation failed after rate-limit failover", inputs

    remediation_errors = remediation.get("errors", [])
    remediation_failed = (
        final_exit != 0
        or (isinstance(remediation_errors, list) and len(remediation_errors) > 0 and not bool(remediation.get("applied", False)))
    )
    if remediation_failed:
        return "remediation_failed", "remediation workflow reported failure", inputs

    for provider, state in findings_states.items():
        if state == "error":
            return "findings_ingest_failed", f"{provider} findings status=error", inputs

    if terminal_failure:
        return "workflow_failed_unknown", "workflow failed without classified reason", inputs

    return "none", "workflow succeeded or no failure classification", inputs


def main() -> int:
    args = parse_args()

    status = str(args.status).strip().lower() or "unknown"
    emission_enabled = args.emit == "true"
    emit_on_failure_only = args.emit_on_failure_only == "true"
    terminal_failure = _terminal_failure(status)
    should_emit = emission_enabled and (terminal_failure or not emit_on_failure_only)

    infra = _read_json(args.infra_preflight_report)
    live = _read_json(args.live_provider_report)
    risk = _read_json(args.risk_policy_report)
    eval_results = _read_json(args.eval_results)
    remediation = _read_json(args.remediation_result)
    failover = _read_json(args.codex_failover_result)

    findings_payloads = [
        ("review", _read_json(args.review_findings)),
        ("claude", _read_json(args.claude_findings)),
        ("sentry", _read_json(args.sentry_findings)),
        ("eval", _read_json(args.eval_findings)),
    ]

    reason_code, detail, classification_inputs = _classify_reason(
        status=status,
        infra=infra,
        live=live,
        risk=risk,
        eval_results=eval_results,
        remediation=remediation,
        failover=failover,
        findings_payloads=findings_payloads,
    )

    payload: Dict[str, Any] = {
        "status": status,
        "emission_enabled": emission_enabled,
        "emit_on_failure_only": emit_on_failure_only,
        "should_emit": should_emit,
        "sent": False,
        "reason_code": reason_code,
        "workflow": args.workflow,
        "job": args.job,
        "run_id": str(args.run_id),
        "run_attempt": str(args.run_attempt),
        "event": args.event,
        "repo": args.repo,
        "head_sha": args.head_sha,
        "pr_number": str(args.pr_number),
        "timestamp": dt.datetime.now(tz=dt.timezone.utc).isoformat(),
        "detail": detail,
        "classification_inputs": classification_inputs,
        "tags": {
            "component": "harness-workflow",
            "workflow": args.workflow,
            "job": args.job,
            "reason_code": reason_code,
            "status": status,
        },
    }

    if not should_emit:
        if not emission_enabled:
            payload["detail"] = "emission disabled by OPENFANG_SENTRY_WORKFLOW_EVENTS"
        elif emit_on_failure_only and not terminal_failure:
            payload["detail"] = "status is non-failure and emit_on_failure_only=true"
        _write_json(args.out, payload)
        print(json.dumps(payload, indent=2, sort_keys=True))
        return 0

    dsn = os.getenv(args.sentry_dsn_env, "").strip()
    if not dsn:
        payload["detail"] = f"missing env {args.sentry_dsn_env}"
        _write_json(args.out, payload)
        print(json.dumps(payload, indent=2, sort_keys=True))
        return 0

    event_level = "error" if terminal_failure else "info"
    event = {
        "event_id": uuid.uuid4().hex,
        "timestamp": payload["timestamp"],
        "level": event_level,
        "platform": "python",
        "logger": "openfang.harness.workflow",
        "environment": os.getenv("OPENFANG_ENV", "ci"),
        "message": {
            "formatted": (
                f"OpenFang workflow={args.workflow} job={args.job} status={status} reason={reason_code}"
            )
        },
        "tags": payload["tags"],
        "extra": {
            "repo": args.repo,
            "run_id": str(args.run_id),
            "run_attempt": str(args.run_attempt),
            "event": args.event,
            "head_sha": args.head_sha,
            "pr_number": str(args.pr_number),
            "classification": classification_inputs,
        },
    }

    ok, send_detail, http_status = send_sentry_event(
        dsn,
        event,
        timeout=20,
        client_name="openfang-workflow-telemetry/1.0",
    )
    payload["sent"] = ok
    payload["detail"] = send_detail
    if http_status is not None:
        payload["sentry_http_status"] = int(http_status)

    _write_json(args.out, payload)
    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
