# Loop Configuration — ftdata-paid

## Active Loops

| Pattern | Cadence | Status | Command |
|---------|---------|--------|---------|
| Daily Triage | 1d | L1 report-only | See README |
| spec-kit SDD | per-feature | Phase 2 | See §spec-kit below |

## Human Gates

- No auto-fix until L2 checklist complete
- All high-risk paths: human review required
- Phase 2 spec-kit SDD requires Constitution approval before Specify phase

## Budget

- Max sub-agent spawns per run: 0 (L1) / 2 (L2)
- Max tokens/day: 100k (see `loop-budget.md`)
- Append each run to `loop-run-log.md`; use `loop-budget` skill at start/end
- Kill switch: `loop-pause-all` — pause schedulers and notify human
- Estimate: `npx @cobusgreyling/loop-cost --pattern daily-triage`

## spec-kit SDD Integration

spec-kit (Spec-Driven Development) workflow is integrated into the loop engineering process for Phase 2+ features.

### SDD Phases

| Phase | Description | Gate |
|-------|-------------|------|
| **Constitution** | Define core principles + success criteria | Human approval required |
| **Specify** | Detailed specs: API, data models, acceptance criteria | Human review |
| **Plan** | Task breakdown, dependencies, risks | Human approval |
| **Tasks** | Implement per-task with test-first | Auto with verification |
| **Implement** | Full acceptance check, performance benchmarks | Human sign-off |

### SDD Loop Pattern

```
loop: spec-kit-sdd
├── Phase 1: Constitution (human gate)
│   └── Output: docs/PHASE2_SPEC.md §1 (Constitution)
├── Phase 2: Specify (human gate)
│   └── Output: docs/PHASE2_SPEC.md §2 (API specs, data models)
├── Phase 3: Plan (human gate)
│   └── Output: docs/PHASE2_SPEC.md §3 (tasks, dependencies)
├── Phase 4: Tasks (auto)
│   └── Output: implemented code + tests
└── Phase 5: Implement (human gate)
    └── Output: verified acceptance criteria

Each phase: human gate → spec doc update → proceed or escalate
```

### SDD + Loop Engineering Combined

| Decision Type | Method | Example |
|---------------|--------|---------|
| Design decisions | spec-kit Constitution | C1-C5 core principles |
| API design | spec-kit Specify | §2.2 API specs |
| Implementation | Loop Engineering | Q6, Q9 execution |
| Testing | Both | E2E tests + acceptance criteria |

### spec-kit SDD Loop Commands

- `/loopx spec-kit:constitution <feature>` — Start Constitution phase
- `/loopx spec-kit:specify <feature>` — Start Specify phase (requires Constitution)
- `/loopx spec-kit:plan <feature>` — Start Plan phase (requires Specify)
- `/loopx spec-kit:implement <feature>` — Execute Tasks phase
- `/loopx spec-kit:verify <feature>` — Run Implement verification

### AI Usage in SDD

| Phase | Human | AI |
|-------|-------|-----|
| Constitution | 100% | 0% |
| Specify | 50% | 50% (OpenAPI, schemas) |
| Plan | 70% | 30% (task breakdown) |
| Implement | 30% | 70% (code generation) |
| Verify | 80% | 20% (test review) |

## Links

- Pattern: [daily-triage](../../patterns/daily-triage.md)
- Checklist: [loop-design-checklist](../../docs/loop-design-checklist.md)
- Phase 2 Spec: [docs/PHASE2_SPEC.md](./PHASE2_SPEC.md)