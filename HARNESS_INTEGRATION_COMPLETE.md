# Harness CI Integration - Implementation Complete

**Issue #5 from Audit:** Harness control-plane unintegrated in tracked CI

**Status:** ✅ IMPLEMENTED & READY TO ACTIVATE

---

## Problem

Harness workflows and infrastructure existed but were untracked in git, preventing them from running in GitHub Actions CI.

**Impact:** PR flow lacked risk-based gating, automated remediation, and deterministic ordering.

---

## Solution Implemented

### 1. Policy Contract Created

**File:** `.harness/policy.contract.json`

**Defines:**
- 4 risk tiers (critical/high/medium/low)
- Required checks per tier
- Review policies and approval requirements
- Rollout phases (phase-0 through phase-3)
- Remediation guardrails
- Browser evidence requirements
- Docs drift rules

**Current Phase:** `phase-0` (advisory mode, metrics collection)

---

### 2. GitHub Actions Workflows (All Present)

| Workflow | File | Purpose | Trigger |
|----------|------|---------|---------|
| Risk Policy Gate | `risk-policy-gate.yml` | Compute risk tier, validate review state | PR open/sync |
| CI Fanout | `ci-fanout.yml` | Run only required checks based on risk tier | After gate pass |
| Greptile Rerun | `greptile-rerun.yml` | Request review rerun for stale/timeout states | After gate detects stale |
| Remediation Agent | `remediation-agent.yml` | Auto-fix issues within guardrails | After gate detects remediable issues |
| Auto-Resolve Threads | `greptile-auto-resolve-threads.yml` | Resolve bot-only unresolved threads | After clean rerun |
| Weekly Metrics | `harness-weekly-metrics.yml` | Collect harness performance metrics | Weekly schedule |

---

### 3. Python Scripts (All Present)

| Script | Purpose |
|--------|---------|
| `risk_policy_gate.py` | Compute risk tier, validate review, emit decision |
| `checks_resolver.py` | Resolve required checks from policy contract |
| `greptile_state.py` | Fetch and validate Greptile review state |
| `remediation_runner.py` | Execute constrained remediation patches |
| `rerun_comment_dedupe.py` | Deduplicate rerun comments by SHA marker |
| `browser_evidence_verify.py` | Validate browser evidence for UI changes |

---

## Deterministic PR Flow

```
PR Opened/Updated
      ↓
risk-policy-gate (Step 1)
      ├─ Compute risk tier from changed files
      ├─ Validate Greptile review state (current HEAD SHA)
      ├─ Check docs drift rules
      └─ Emit decision: pass|fail|needs-remediation|stale-review|timeout
      ↓
[Decision: pass] → ci-fanout (Step 2)
      ├─ Run only required checks for risk tier
      └─ Report results
      ↓
[Decision: needs-remediation] → remediation-agent (Step 3)
      ├─ Apply constrained patches within guardrails
      └─ Commit fixes in-branch
      ↓
[Decision: stale-review] → greptile-rerun (Step 4)
      ├─ Post one deduped rerun comment per SHA
      └─ Wait for fresh review
      ↓
[After rerun] → greptile-auto-resolve-threads (Step 5)
      └─ Resolve bot-only unresolved threads
```

---

## Risk Tiers & Required Checks

### Critical (Runtime/Kernel/Memory)
**Paths:**
- `crates/openfang-runtime/**`
- `crates/openfang-kernel/**`
- `crates/openfang-memory/**`

**Required Checks:**
- build, test, clippy, security-audit, integration-tests, code-review

**Review Policy:** 2 approvals + owner review required

---

### High (API/Channels/CLI)
**Paths:**
- `crates/openfang-api/**`
- `crates/openfang-channels/**`
- `crates/openfang-cli/**`

**Required Checks:**
- build, test, clippy, integration-tests

**Review Policy:** 1 approval required

---

### Medium (Extensions/Skills/Hands)
**Paths:**
- `crates/openfang-extensions/**`
- `crates/openfang-skills/**`
- `agents/**`

**Required Checks:**
- build, test, clippy

**Review Policy:** 1 approval required

---

### Low (Docs/Configs/Scripts)
**Paths:**
- `docs/**`, `*.md`, `scripts/**`

**Required Checks:**
- docs-lint

**Review Policy:** No approvals required

---

## Rollout Phases

### Phase 0: Advisory (CURRENT)
- Metrics collection only
- No merge blocking
- No remediation

**Purpose:** Baseline data collection

---

### Phase 1: Soft Enforcement
- Block merges on stale review/docs drift
- Remediation only for PRs with label `harness-remediation-pilot`
- Fanout still runs on gate failure (observability)

**Activate:** Set `currentPhase: "phase-1"` in policy contract

---

### Phase 2: Evidence Required
- Full remediation enabled
- Browser evidence required for UI changes
- Fanout only after passing gate

**Activate:** Set `currentPhase: "phase-2"` in policy contract

---

### Phase 3: Hard Enforcement
- Strict review state validation
- Full enforcement of all policies

**Activate:** Set `currentPhase: "phase-3"` in policy contract

---

## Activation Steps

### 1. Add Files to Git

```bash
git add .harness/policy.contract.json
git add .github/workflows/risk-policy-gate.yml
git add .github/workflows/ci-fanout.yml
git add .github/workflows/greptile-rerun.yml
git add .github/workflows/remediation-agent.yml
git add .github/workflows/greptile-auto-resolve-threads.yml
git add .github/workflows/harness-weekly-metrics.yml
git add scripts/harness/*.py
```

---

### 2. Commit & Push

```bash
git commit -m "feat(ci): integrate harness control-plane for deterministic PR flow

- Add risk-based policy contract with 4 tiers
- Add GitHub Actions workflows for gate/fanout/remediation
- Add Python scripts for policy enforcement
- Configure rollout phases (starting in phase-0)
- Enable weekly metrics collection

Co-Authored-By: Claude Sonnet 4.5 (1M context) <noreply@anthropic.com>"

git push origin main
```

---

### 3. Configure GitHub Secrets (if needed)

Some workflows may need:
- `GITHUB_TOKEN` (automatically available)
- `GREPTILE_API_KEY` (for review integration)
- Other API keys as needed

Add these in repository settings → Secrets and variables → Actions

---

### 4. Test on a PR

1. Create a test PR with changes to a critical path
2. Verify `risk-policy-gate` runs and computes correct tier
3. Verify `ci-fanout` runs only required checks
4. Monitor workflow logs for correct behavior

---

### 5. Progress Through Phases

Once confident in phase-0 metrics:

```bash
# Edit .harness/policy.contract.json
# Change: "currentPhase": "phase-1"
git commit -am "chore(harness): advance to phase-1 enforcement"
git push
```

---

## Files Created/Verified

### New Files (This Implementation)
- ✅ `.harness/policy.contract.json` - Policy definitions

### Existing Files (Verified Present)
- ✅ `.github/workflows/risk-policy-gate.yml`
- ✅ `.github/workflows/ci-fanout.yml`
- ✅ `.github/workflows/greptile-rerun.yml`
- ✅ `.github/workflows/remediation-agent.yml`
- ✅ `.github/workflows/greptile-auto-resolve-threads.yml`
- ✅ `.github/workflows/harness-weekly-metrics.yml`
- ✅ `scripts/harness/risk_policy_gate.py`
- ✅ `scripts/harness/checks_resolver.py`
- ✅ `scripts/harness/greptile_state.py`
- ✅ `scripts/harness/remediation_runner.py`
- ✅ `scripts/harness/rerun_comment_dedupe.py`
- ✅ `scripts/harness/browser_evidence_verify.py`

---

## Local Testing

Test the risk policy gate locally:

```bash
python3 scripts/harness/risk_policy_gate.py \
  --pr 123 \
  --head-sha abc123 \
  --changed-files "crates/openfang-runtime/src/lib.rs,docs/README.md"

# Output: risk-policy-report.json with decision and required checks
```

---

## Benefits

1. **Deterministic:** PR flow has strict ordering, no race conditions
2. **Risk-Based:** Only run checks needed for changed files
3. **Automated:** Remediation can auto-fix common issues
4. **Phased:** Gradual rollout prevents disruption
5. **Observable:** Weekly metrics track harness performance

---

## Current Status

- ✅ All infrastructure files present
- ✅ Policy contract created
- ✅ Scripts verified executable
- ✅ Workflows configured correctly
- ⏳ **Awaiting activation** (add to git and push)

---

**Implementation Date:** February 27, 2026
**Status:** READY TO ACTIVATE ✅
**Next Action:** Commit and push files to enable in CI
