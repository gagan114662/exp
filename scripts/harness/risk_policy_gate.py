#!/usr/bin/env python3
"""Deterministic PR preflight gate for OpenFang harness engineering."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
from pathlib import Path
from typing import Any, Dict, List

from browser_evidence_verify import verify_manifest
from checks_resolver import (
    compute_required_checks,
    compute_risk_tier,
    evaluate_docs_drift,
    get_rollout_settings,
    load_contract,
    read_changed_files,
    requires_browser_evidence,
)
from greptile_state import (
    ReviewState,
    count_actionable_findings,
    get_review_check_state_once,
    load_or_init_review_findings,
    review_state_as_dict,
    wait_for_review_check,
    write_review_findings,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="OpenFang risk policy gate")
    parser.add_argument("--pr", type=int, required=True, help="Pull request number")
    parser.add_argument("--head-sha", required=True, help="Current PR head SHA")
    parser.add_argument(
        "--changed-files",
        required=True,
        help="Path to newline-delimited changed files list",
    )
    parser.add_argument(
        "--contract",
        default=".harness/policy.contract.json",
        help="Path to machine-readable harness contract",
    )
    parser.add_argument("--repo", default=os.getenv("GITHUB_REPOSITORY", ""), help="owner/repo")
    parser.add_argument("--token-env", default="GITHUB_TOKEN", help="Environment variable that stores GitHub token")
    parser.add_argument(
        "--review-findings",
        default="artifacts/review-findings.json",
        help="Input/output JSON with normalized review findings",
    )
    parser.add_argument(
        "--browser-evidence-manifest",
        default="artifacts/browser-evidence-manifest.json",
        help="Browser evidence manifest path",
    )
    parser.add_argument(
        "--report-out",
        default="artifacts/risk-policy-report.json",
        help="Output path for risk-policy-report.json",
    )
    parser.add_argument("--poll-seconds", type=int, default=20, help="Review check polling interval")
    return parser.parse_args()


def _default_review_state(provider: str, reason: str) -> ReviewState:
    return ReviewState(provider=provider, status="missing", details=reason)


def _write_json(path: str, payload: Dict[str, Any]) -> None:
    out = Path(path)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    args = parse_args()

    contract = load_contract(args.contract)
    changed_files = read_changed_files(args.changed_files)
    risk_tier = compute_risk_tier(changed_files, contract.get("riskTierRules", {}))
    required_checks = compute_required_checks(contract, risk_tier)
    rollout_phase, rollout_settings = get_rollout_settings(contract)

    review_policy = contract.get("reviewPolicy", {})
    provider = str(review_policy.get("provider", "greptile"))
    check_name = str(review_policy.get("checkRunName", "greptile-review"))
    timeout_minutes = int(review_policy.get("timeoutMinutes", 20))
    weak_confidence = float(review_policy.get("weakConfidenceThreshold", 0.55))
    keywords = [str(k).lower() for k in review_policy.get("actionableSummaryKeywords", [])]
    enforce_review_state = bool(rollout_settings.get("enforceReviewState", False))

    token = os.getenv(args.token_env, "")
    review_state: ReviewState
    if args.repo and token:
        if enforce_review_state:
            review_state = wait_for_review_check(
                repo=args.repo,
                sha=args.head_sha,
                token=token,
                check_name=check_name,
                timeout_minutes=timeout_minutes,
                poll_seconds=args.poll_seconds,
                provider=provider,
            )
        else:
            review_state = get_review_check_state_once(
                repo=args.repo,
                sha=args.head_sha,
                token=token,
                check_name=check_name,
                provider=provider,
            )
    else:
        missing_reason = "review API unavailable (missing repo or token); cannot verify current-head review state"
        review_state = _default_review_state(provider, missing_reason)

    review_findings = load_or_init_review_findings(
        args.review_findings,
        head_sha=args.head_sha,
        provider=provider,
        weak_confidence_threshold=weak_confidence,
        actionable_keywords=keywords,
    )
    review_findings["status"] = review_state.status
    write_review_findings(args.review_findings, review_findings)
    actionable_findings_count = count_actionable_findings(review_findings)

    docs_violations = evaluate_docs_drift(changed_files, contract.get("docsDriftRules", []))

    evidence_policy = contract.get("evidencePolicy", {})
    evidence_needed = requires_browser_evidence(changed_files, contract)
    evidence_errors: List[str] = []
    if evidence_needed:
        evidence_ok, evidence_errors, _ = verify_manifest(
            args.browser_evidence_manifest,
            head_sha=args.head_sha,
            required_flows=evidence_policy.get("requiredFlows", []),
            required_assertions=evidence_policy.get("requiredAssertions", []),
        )
        if evidence_ok:
            evidence_errors = []

    decision = "pass"
    reasons: List[str] = []

    enforce_docs_drift = bool(rollout_settings.get("enforceDocsDrift", False))
    enforce_evidence = bool(rollout_settings.get("enforceEvidence", False))
    enable_remediation = bool(rollout_settings.get("enableRemediation", False))

    def record_reason(message: str, *, enforced: bool) -> None:
        if enforced:
            reasons.append(message)
        else:
            reasons.append(f"advisory: {message}")

    if review_state.status == "timeout":
        record_reason("review check timed out on current head SHA", enforced=enforce_review_state)
        if enforce_review_state:
            decision = "timeout"
    elif review_state.status == "pending":
        record_reason("current-head review is still pending", enforced=enforce_review_state)
        if enforce_review_state:
            decision = "stale-review"
    elif review_state.status in {"missing"}:
        record_reason("current-head review is missing", enforced=enforce_review_state)
        if enforce_review_state:
            decision = "stale-review"
    elif review_state.status in {"failure", "error"}:
        record_reason("review check is not successful on current head SHA", enforced=enforce_review_state)
        if enforce_review_state:
            decision = "fail"

    if actionable_findings_count > 0:
        if enable_remediation:
            record_reason(f"{actionable_findings_count} actionable review finding(s) detected", enforced=True)
            if decision == "pass":
                decision = "needs-remediation"
        else:
            record_reason(
                f"{actionable_findings_count} actionable review finding(s) detected",
                enforced=False,
            )

    if docs_violations:
        for violation in docs_violations:
            record_reason(violation, enforced=enforce_docs_drift)
        if enforce_docs_drift and decision == "pass":
            decision = "fail"

    if evidence_needed and evidence_errors:
        for error in evidence_errors:
            record_reason(f"browser evidence: {error}", enforced=enforce_evidence)
        if enforce_evidence and decision == "pass":
            decision = "fail"

    if not reasons:
        reasons.append("all policy checks passed")

    report = {
        "pr_number": args.pr,
        "head_sha": args.head_sha,
        "risk_tier": risk_tier,
        "changed_files": changed_files,
        "required_checks": required_checks,
        "review_state": review_state_as_dict(review_state),
        "actionable_findings_count": actionable_findings_count,
        "decision": decision,
        "reasons": reasons,
        "timestamp": dt.datetime.now(tz=dt.timezone.utc).isoformat(),
        "rollout_phase": rollout_phase,
        "rollout": rollout_settings,
    }

    _write_json(args.report_out, report)

    enforce_merge_block = bool(rollout_settings.get("enforceMergeBlock", False))
    should_fail_job = enforce_merge_block and decision != "pass"

    print(json.dumps(report, indent=2, sort_keys=True))
    return 1 if should_fail_job else 0


if __name__ == "__main__":
    raise SystemExit(main())
