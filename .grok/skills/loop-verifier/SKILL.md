---
name: loop-verifier
description: >
  Verification workflow for loop agents. Run after completing a task to verify
  quality and completeness before reporting done or escalating.
user_invocable: true
---

# Loop Verifier Skill

Run this skill after completing any task in a loop iteration to verify quality.

## When to Invoke

- After `loop-triage` identifies an issue and a fix is applied
- Before closing a task as complete
- Before escalating to human review
- At end of loop iteration to verify all completed work

## Pre-flight Checks

1. Confirm the files that were changed
2. Identify the test command for the project
3. Check for any project-specific linting/formatting tools

## Verification Steps

### Step 1: Gather Context

Read these files if they exist:
- `loop-constraints.md` - understand what rules are active
- `loop-budget.md` - know token budget remaining
- Any relevant task files or issue trackers

### Step 2: Validate Task Completion

For each task being verified:

1. List all files modified
2. For each file, verify:
   - Core functionality is implemented
   - No placeholder code (`TODO`, `FIXME`, `unimplemented`)
   - Edge cases are handled
   - Error cases return proper errors

### Step 3: Code Quality Gate

Run in order:
```bash
# Formatting check
cargo fmt --check

# Linting
cargo clippy --all-targets --all-features -- -D warnings

# Type checking
cargo check
```

If any fail, report as FAIL with specific failures.

### Step 4: Test Gate

```bash
cargo test --all
```

Verify:
- All tests pass (no failures)
- No tests are ignored (`#[ignore]`)
- New code has corresponding test coverage

### Step 5: Documentation Check

- [ ] `*.md` files updated if behavior changed
- [ ] `CHANGELOG.md` updated if version-worthy change
- [ ] Code comments explain "why" not "what"

## Output Template

```markdown
## Verification Results

**Task:** [Brief description]
**Status:** ✅ PASS | ⚠️ PARTIAL | ❌ FAIL

### Completeness Check
| Item | Status | Notes |
|------|--------|-------|
| Core implementation | ✅/⚠️/❌ | |
| Error handling | ✅/⚠️/❌ | |
| Edge cases | ✅/⚠️/❌ | |

### Quality Gate
| Check | Result |
|-------|--------|
| Format | ✅/❌ |
| Clippy | ✅/❌ |
| Tests | ✅/❌ |

### Documentation
- [ ] Updated | [ ] N/A | [ ] Missing: [what]

### blockers
[Any blocking issues that must be fixed]

### recommendations
[Any non-blocking suggestions]
```

## Decision Rules

| Condition | Action |
|-----------|--------|
| Any quality gate fails | FAIL - must fix |
| Tests fail | FAIL - must fix |
| Task < 80% complete | PARTIAL - specify what's missing |
| Task 80-99% complete | PARTIAL - minor items noted |
| All checks pass | PASS - ready to close |

## Escalation Triggers

Escalate to human if:
- Security vulnerability found (crets, injection, etc.)
- Data corruption possible
- Breaking API changes without deprecation path
- More than 3 failures in any category

## Token Budget

Target: < 30k tokens for standard verification
Extended: < 50k tokens if deep investigation needed
Do not spawn sub-agents during verification
