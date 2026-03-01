#!/usr/bin/env python3
"""Emit structured eval metrics and optional sanitized Sentry event."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import uuid
from pathlib import Path
from typing import Any, Dict

from sentry_client import send_sentry_event


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Emit OpenFang eval metrics")
    parser.add_argument("--eval-results", required=True, help="Path to eval-results.json")
    parser.add_argument("--out", default="artifacts/agent-evals/eval-metrics.json", help="Metrics output JSON")
    parser.add_argument("--sentry-dsn-env", default="SENTRY_DSN", help="Sentry DSN env var name")
    parser.add_argument("--emit-sentry", default="false", choices=["true", "false"], help="Emit Sentry event")
    parser.add_argument("--environment", default=os.getenv("OPENFANG_ENV", "ci"), help="Environment tag")
    return parser.parse_args()


def _write_json(path: str, payload: Dict[str, Any]) -> None:
    out = Path(path)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    args = parse_args()

    eval_results = json.loads(Path(args.eval_results).read_text(encoding="utf-8"))
    summary = eval_results.get("summary", {}) if isinstance(eval_results, dict) else {}

    metrics = {
        "generated_at": dt.datetime.now(tz=dt.timezone.utc).isoformat(),
        "head_sha": str(eval_results.get("head_sha", "")),
        "profile": str(eval_results.get("profile", "")),
        "total": int(summary.get("total", 0) or 0),
        "passed": int(summary.get("passed", 0) or 0),
        "failed": int(summary.get("failed", 0) or 0),
        "pass_rate": float(summary.get("pass_rate", 0) or 0),
        "failure_classes": summary.get("failure_classes", {}),
        "blocking_threshold": float(summary.get("blocking_threshold", 1.0) or 1.0),
        "all_blocking_passed": bool(summary.get("all_blocking_passed", False)),
        "sentry": {
            "enabled": args.emit_sentry == "true",
            "sent": False,
            "detail": "disabled",
        },
    }

    if args.emit_sentry == "true":
        dsn = os.getenv(args.sentry_dsn_env, "").strip()
        if not dsn:
            metrics["sentry"] = {"enabled": True, "sent": False, "detail": f"missing env {args.sentry_dsn_env}"}
        else:
            event = {
                "event_id": uuid.uuid4().hex,
                "timestamp": dt.datetime.now(tz=dt.timezone.utc).isoformat(),
                "level": "error" if metrics["failed"] > 0 else "info",
                "platform": "python",
                "logger": "openfang.agent-evals",
                "environment": args.environment,
                "message": {
                    "formatted": f"OpenFang agent-evals {metrics['profile']} failed={metrics['failed']} total={metrics['total']}"
                },
                "tags": {
                    "component": "agent-evals",
                    "profile": metrics["profile"],
                    "all_blocking_passed": str(metrics["all_blocking_passed"]).lower(),
                },
                "extra": {
                    "failed": metrics["failed"],
                    "total": metrics["total"],
                    "pass_rate": metrics["pass_rate"],
                    "failure_class_count": len(metrics["failure_classes"]),
                },
            }
            ok, detail, _ = send_sentry_event(
                dsn,
                event,
                timeout=20,
                client_name="openfang-eval-metrics/1.0",
            )
            metrics["sentry"] = {"enabled": True, "sent": ok, "detail": detail}

    _write_json(args.out, metrics)
    print(json.dumps(metrics, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
