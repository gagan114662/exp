#!/usr/bin/env python3
"""Upsert PR body checklist block and sticky harness comment."""

from __future__ import annotations

import argparse
import json
import os
import re
import urllib.request
from pathlib import Path
from typing import Any, Dict, List


def _request_json(url: str, token: str, method: str = "GET", payload: Dict[str, Any] | None = None) -> Dict[str, Any]:
    headers = {
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "Authorization": f"Bearer {token}",
    }
    data = None
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, headers=headers, method=method, data=data)
    with urllib.request.urlopen(req, timeout=30) as resp:
        body = resp.read().decode("utf-8")
        if not body:
            return {}
        return json.loads(body)


def _request_list(url: str, token: str) -> List[Dict[str, Any]]:
    payload = _request_json(url, token, method="GET")
    return payload if isinstance(payload, list) else []


def _upsert_pr_body(
    repo: str,
    pr_number: int,
    token: str,
    block_body: str,
    marker_start: str,
    marker_end: str,
) -> bool:
    pr_url = f"https://api.github.com/repos/{repo}/pulls/{pr_number}"
    pr = _request_json(pr_url, token, method="GET")
    existing = str(pr.get("body", "") or "")

    pattern = re.compile(re.escape(marker_start) + r".*?" + re.escape(marker_end), re.DOTALL)
    if pattern.search(existing):
        updated = pattern.sub(block_body.strip(), existing).strip() + "\n"
    else:
        updated = (existing.rstrip() + "\n\n" + block_body.strip() + "\n").strip() + "\n"

    if updated == existing:
        return False

    _request_json(pr_url, token, method="PATCH", payload={"body": updated})
    return True


def _upsert_sticky_comment(
    repo: str,
    pr_number: int,
    token: str,
    marker: str,
    comment_body: str,
) -> bool:
    comments_url = f"https://api.github.com/repos/{repo}/issues/{pr_number}/comments?per_page=100"
    comments = _request_list(comments_url, token)

    existing_id = 0
    existing_body = ""
    for comment in comments:
        body = str(comment.get("body", ""))
        if marker in body:
            existing_id = int(comment.get("id", 0))
            existing_body = body
            break

    if existing_id:
        if existing_body.strip() == comment_body.strip():
            return False
        patch_url = f"https://api.github.com/repos/{repo}/issues/comments/{existing_id}"
        _request_json(patch_url, token, method="PATCH", payload={"body": comment_body})
        return True

    post_url = f"https://api.github.com/repos/{repo}/issues/{pr_number}/comments"
    _request_json(post_url, token, method="POST", payload={"body": comment_body})
    return True


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Upsert PR body block and sticky harness comment")
    parser.add_argument("--repo", required=True, help="owner/repo")
    parser.add_argument("--pr-number", type=int, required=True, help="Pull request number")
    parser.add_argument("--body-block", required=True, help="Path to pr-body-block markdown")
    parser.add_argument("--comment-body", required=True, help="Path to sticky comment markdown")
    parser.add_argument("--token-env", default="GITHUB_TOKEN", help="Env var containing GitHub token")
    parser.add_argument("--comment-marker", default="<!-- pr-review-harness -->", help="Sticky comment marker")
    parser.add_argument("--body-marker-start", default="<!-- pr-review-checklist:start -->", help="Body block start marker")
    parser.add_argument("--body-marker-end", default="<!-- pr-review-checklist:end -->", help="Body block end marker")
    parser.add_argument("--out", default="artifacts/pr-review/upsert-result.json", help="Result output JSON")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    token = os.getenv(args.token_env, "").strip()
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)

    result = {
        "ok": False,
        "repo": args.repo,
        "pr_number": args.pr_number,
        "body_updated": False,
        "comment_updated": False,
        "error": "",
    }

    if not token:
        result["error"] = f"missing token in env var: {args.token_env}"
        out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(json.dumps(result, indent=2, sort_keys=True))
        return 1

    body_block = Path(args.body_block).read_text(encoding="utf-8")
    comment_body = Path(args.comment_body).read_text(encoding="utf-8")
    if args.comment_marker not in comment_body:
        comment_body = f"{args.comment_marker}\n{comment_body.strip()}\n"

    body_updated = _upsert_pr_body(
        repo=args.repo,
        pr_number=args.pr_number,
        token=token,
        block_body=body_block,
        marker_start=args.body_marker_start,
        marker_end=args.body_marker_end,
    )
    comment_updated = _upsert_sticky_comment(
        repo=args.repo,
        pr_number=args.pr_number,
        token=token,
        marker=args.comment_marker,
        comment_body=comment_body,
    )

    result["ok"] = True
    result["body_updated"] = body_updated
    result["comment_updated"] = comment_updated
    out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
