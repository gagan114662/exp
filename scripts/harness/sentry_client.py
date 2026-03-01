#!/usr/bin/env python3
"""Shared Sentry transport helpers for harness scripts."""

from __future__ import annotations

import datetime as dt
import json
import urllib.error
import urllib.parse
import urllib.request
import uuid
from typing import Any, Dict, Tuple


def parse_dsn(dsn: str) -> Dict[str, str]:
    parsed = urllib.parse.urlparse(dsn)
    if not parsed.scheme or not parsed.hostname or not parsed.path:
        raise ValueError("invalid Sentry DSN")
    if "@" not in parsed.netloc:
        raise ValueError("invalid Sentry DSN auth segment")

    user_info = parsed.netloc.split("@", 1)[0]
    key = user_info.split(":", 1)[0].strip()
    project_id = parsed.path.strip("/").strip()
    if not key or not project_id:
        raise ValueError("invalid Sentry DSN key/project")

    return {
        "scheme": parsed.scheme,
        "host": parsed.hostname,
        "port": str(parsed.port or (443 if parsed.scheme == "https" else 80)),
        "key": key,
        "project_id": project_id,
    }


def _envelope_body(dsn: str, event: Dict[str, Any]) -> bytes:
    envelope_headers = {
        "event_id": event.get("event_id") or uuid.uuid4().hex,
        "dsn": dsn,
        "sent_at": dt.datetime.now(tz=dt.timezone.utc).isoformat(),
    }
    item_headers = {"type": "event", "content_type": "application/json"}
    body = (
        json.dumps(envelope_headers, separators=(",", ":"))
        + "\n"
        + json.dumps(item_headers, separators=(",", ":"))
        + "\n"
        + json.dumps(event, separators=(",", ":"))
    )
    return body.encode("utf-8")


def send_sentry_event(
    dsn: str,
    event: Dict[str, Any],
    *,
    timeout: int = 20,
    client_name: str = "openfang-harness/1.0",
) -> Tuple[bool, str, int | None]:
    """Send one Sentry envelope event.

    Returns: (sent, detail, http_status)
    """
    parsed = parse_dsn(dsn)
    envelope_url = (
        f"{parsed['scheme']}://{parsed['host']}:{parsed['port']}/api/{parsed['project_id']}/envelope/"
    )
    req = urllib.request.Request(
        envelope_url,
        data=_envelope_body(dsn, event),
        headers={
            "Content-Type": "application/x-sentry-envelope",
            "X-Sentry-Auth": (
                f"Sentry sentry_version=7, sentry_key={parsed['key']}, sentry_client={client_name}"
            ),
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            status = int(resp.status)
            return 200 <= status < 300, f"http_status={status}", status
    except urllib.error.HTTPError as exc:
        code = int(exc.code)
        return False, f"http_error={code}", code
    except urllib.error.URLError as exc:
        return False, f"url_error={exc.reason}", None
    except Exception as exc:  # pragma: no cover
        return False, f"unexpected_error={exc}", None
