# Design Decisions (Q1–Q11)

**Date**: 2026-08-13
**Status**: All 11 open questions from `docs/PAID_API_DESIGN.md` §9 resolved by the project owner.
**Effective**: All future implementation work should follow these decisions.

| ID | Question | Decision | Implementation impact |
|---|---|---|---|
| **Q1** | Repo layout: standalone `fq-data-paid/` repo vs monorepo `paid/` subdir? | **Standalone repo** | Plan repo extraction (see §1 below) |
| **Q2** | Launch exchange coverage: Binance only or all three? | **Binance only** | Restrict exchange validation to `binance`; mark bybit/okx as future expansion |
| **Q3** | Launch network: Base / Polygon / Solana / multi-chain? | **Base** | Confirm default; M2M auth on Base |
| **Q4** | Facilitator choice: Cloudflare / Coinbase / self-hosted? | **Cloudflare (MGW integration, Workers-friendly)** | Real `HttpFacilitator` impl hits CF MGW API; Workers-compatible |
| **Q5** | Free tier granularity: IP / wallet / dual? | **Wallet primary; dual = IP coarse + wallet fine** | Per-wallet rate limiter; IP as secondary fallback |
| **Q6** | MCP endpoint exposure: Phase 2 or now? | **Now: do web console first** | Build web dashboard (Phase 1 signal #5) |
| **Q7** | Data value-add SKUs day-1 vs Phase 2? | **Day-1: cleaned data** | Add cleaning step in origin pipeline; add `cleaned` flag to quote / receipt |
| **Q8** | API key auth path? | **Current: app state hasn't integrated API key module** | Add `ApiKeyStore` + middleware for x402 OR API key |
| **Q9** | Real origin: shell out `ftdata-cli` vs direct core crates? | **Direct core crates** | Rewrite origin to call `ftdata-core` directly; reuse checkpoint / validator |
| **Q10** | Settlement reconciliation: real-time / daily / on-demand? | **Real-time: every receipt reconciles immediately (already implemented but only at admin endpoint)** | Expose `/v1/reconcile` to customers with proper auth; keep admin endpoint |
| **Q11** | Workers pricing rule storage: KV / D1? | **KV: simple, low latency** | Move pricing rules from code-defined to KV-loaded config |

---

## 1. Q1 Plan: Standalone `fq-data-paid/` Repo Extraction

**Current state**: 23 commits on `feat/paid-mvp` in the monorepo (`/Users/jie/code/fq-data-downloader`).

**Crate surface to extract** (3 new crates + assets):
```
ftdata-paid-pricing/      # pure pricing library
ftdata-paid-facilitator/  # x402 trait + MockFacilitator + (Q4) Cloudflare impl
ftdata-paid-api/          # Axum server + 5 routes + (Q6) web console
ftdata-paid-api/examples/agent-client.ts
ftdata-paid-api/docker/Dockerfile
ftdata-paid-api/README.md
scripts/smoke.sh
docs/PAID_API_DESIGN.md   # the design doc itself (or a copy)
docs/DECISIONS.md         # this file
```

**CLI crates STAY in monorepo** (MIT-licensed, no payment logic):
```
ftdata-cli/ ftdata-core/ ftdata-http/ ftdata-sources/ ftdata-storage/ ftdata-analysis/
```

**Recommended extraction steps** (human-gated, requires `git push` + new repo creation):
1. Use `git filter-repo --path ftdata-paid-pricing/ --path ftdata-paid-facilitator/ --path ftdata-paid-api/ --path scripts/smoke.sh --path docs/PAID_API_DESIGN.md --path docs/DECISIONS.md` on a fresh clone of `feat/paid-mvp`
2. Push the filtered branch to a new GitHub repo `gyc567/fq-data-paid`
3. New repo gets its own CI, LICENSE (commercial), CHANGELOG
4. Original monorepo PR removes the 3 paid crates and the design doc (keep DECISIONS.md here as project memory)
5. Add a brief pointer in the monorepo README: "The paid API is now in `gyc567/fq-data-paid`"

**Status of this plan**: documented here. Actual move is L3 work requiring push approval. Current iteration is preparing the code on `feat/paid-mvp` so the move is mechanical when approved.

---

## 2. Decision-Action Matrix

| Decision | Action this iteration | Crate / File | Status |
|---|---|---|---|
| Q1 | Document extraction plan (above) | `docs/DECISIONS.md` | ✅ documented |
| Q2 | Restrict exchange validation to `binance` only | `ftdata-paid-api/src/routes/quote.rs` validate() | TODO |
| Q3 | Confirm `Network::Base` default | already default | ✅ done |
| Q4 | Scaffold `CloudflareFacilitator` behind trait | `ftdata-paid-facilitator/src/cloudflare.rs` | TODO |
| Q5 | Per-wallet rate limiter + IP fallback | `ftdata-paid-api/src/rate_limit.rs` (new) | TODO |
| Q6 | Web console for `/v1/reconcile` + `/v1/jobs` | `ftdata-paid-api/src/web/` (new) | TODO |
| Q7 | Cleaning step in origin | `ftdata-paid-api/src/origin.rs` add `cleaned` field | TODO |
| Q8 | `ApiKeyStore` + hybrid auth middleware | `ftdata-paid-api/src/auth.rs` (new) | TODO |
| Q9 | Direct `ftdata-core` call from origin | `ftdata-paid-api/src/origin.rs` rewrite | TODO |
| Q10 | Move reconcile from admin-only to customer-facing | `ftdata-paid-api/src/routes/reconcile.rs` add auth | TODO |
| Q11 | KV-loaded pricing config | `ftdata-paid-api/src/policy.rs` (new) | TODO |

## 3. Dependencies Between Decisions

- Q8 (API key) is a prerequisite for Q10 (customer-facing reconcile) — same auth pattern
- Q5 (rate limit) needs wallet resolution that comes from Q8's auth chain
- Q4 (Cloudflare) is independent but large; can be a follow-up
- Q6 (web console) needs Q5 + Q10 working to be useful
- Q9 (real origin) is the deepest change and depends on Q7 (cleaning) for value-add SKUs

## 4. Recommended Iteration Order

1. **Q2** (5 min) — restrict exchange; cheap and foundational
2. **Q8** (15 min) — API key + auth middleware; unlocks Q5 + Q10
3. **Q5** (10 min) — per-wallet rate limit using auth identity
4. **Q10** (10 min) — make reconcile customer-facing (uses Q8's auth)
5. **Q7** (15 min) — cleaning flag in origin + receipt; small but visible
6. **Q6** (30 min) — web console (now powered by Q8 + Q5 + Q10)
7. **Q9** (60 min) — real origin via ftdata-core; largest single piece
8. **Q11** (20 min) — KV-loaded policy file; touches pricing + facilitator + API
9. **Q4** (60 min) — Cloudflare facilitator impl; needs CF account / API key

Total estimated work: ~3.5 hours of focused implementation. Will be split across multiple loop iterations as budget allows.
