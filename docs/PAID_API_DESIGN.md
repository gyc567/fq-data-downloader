# ftdata Paid API — x402 Monetization Design

> Status: **L1 report-only design draft**
> Loop: `design-triage` (custom)
> Date: 2026-08-13
> Author: ftdata contributors
> Reviewers: pending

A design proposal for wrapping the existing `ftdata` CLI into a paid, x402-native HTTP data service. The CLI stays MIT/free. A new commercial layer (`fq-data-paid`) provides paid HTTP access on top.

---

## 0. Loop Engineering Meta

| Field | Value |
|---|---|
| Pattern | `design-triage` (custom, not `daily-triage`) |
| Cadence | one-shot until L2 unlocked |
| Status | **L1 report-only** — output is documentation only |
| Human gate | design must be reviewed before code is written |
| Sub-agent spawn | 0 (L1) / cap 2 (L2) |
| Token budget | ≤ 25k for this design / 100k/day total (per `loop-budget.md`) |

### Goal Decomposition (with acceptance observations)

| ID | Goal | Acceptance observation |
|---|---|---|
| G1 | Preserve MIT + add x402-compatible monetization | Doc contains "no break to open source CLI" clause + "request = transaction" principle |
| G2 | API surface mirrors existing CLI capabilities | Every CLI command maps to ≥ 1 HTTP endpoint |
| G3 | Pricing model with concrete formula | Doc contains formula + worked examples + boundaries |
| G4 | Deployment architecture (two paths) | Cost + risk comparison table for each path |
| G5 | Three-phase rollout (MVP → productize → growth) | Each phase has a testable "done" signal |
| G6 | Risk register | ≥ 5 risks, each with mitigation |

### Closed-loop checks (per requirement)

| ID | Check | How to verify |
|---|---|---|
| C1 | "No break to open source CLI" in design | grep `MIT\|开源\|免费` in this file |
| C2 | CLI → HTTP mapping 100% | table row count = CLI command count |
| C3 | Pricing formula verifiable by hand | 3 examples: hand-computed = API-returned |
| C4 | Deployment cost comparison | ≥ 2-row table, each row with numbers |
| C5 | Phase completion signals testable | each bullet is boolean checkable |
| C6 | Risk register ≥ 5 with mitigations | row count ≥ 5 |
| C7 | Document length < 800 lines | suitable for single-PR review |

---

## 1. Business Positioning

**Core principle**: the CLI stays MIT and free forever. New value layer is independently monetized.

```
┌──────────────────────────────────────────┐
│  ftdata open-source CLI (forever free)    │
│  - 6 crates, zero changes                 │
│  - GitHub Releases continue               │
└──────────────────────────────────────────┘
                    ↓ wrapper layer (new)
┌──────────────────────────────────────────┐
│  ftdata-paid (new crate, commercial)     │
│  - HTTP API + x402 middleware             │
│  - standalone repo or subdir             │
│  - independent commercial LICENSE         │
└──────────────────────────────────────────┘
                    ↓ deployment
┌──────────────────────────────────────────┐
│  Deployment target (one or both)         │
│  A: Cloudflare Workers + R2 (recommended)│
│  B: self-hosted VPS + Nginx + Rust bin   │
└──────────────────────────────────────────┘
```

**Why this works**:
- Open-source contributors and self-hosters keep using the CLI for free.
- AI agents and quant teams who don't want to run infra pay per request.
- x402 turns every HTTP call into a micro-transaction; no accounts, no API keys, no signup.

---

## 2. API Surface (CLI → HTTP mapping)

| CLI command | HTTP path | Method | Auth | Notes |
|---|---|---|---|---|
| `download` | `/v1/download` | POST | x402 | main entry, billed by data volume |
| `update` | `/v1/update` | POST | x402 | incremental, flat low price |
| `plan` | `/v1/quote` | POST | free | returns price preview, no download |
| `verify` | `/v1/verify` | POST | x402 | verify local or R2 file |
| `gaps` | `/v1/gaps` | POST | free | probe only, lead-magnet |
| `repair` | `/v1/repair` | POST | x402 | fill gaps, billed by gap size |
| `info` | `/v1/info` | GET | free | service meta + price table |
| `cancel` | `/v1/jobs/{id}` | DELETE | x402 | cancel queued job |
| `status` | `/v1/jobs/{id}` | GET | job id | job status query |

### 2.1 Main endpoint detailed schema

#### `POST /v1/quote` (free → returns price)

Request:
```json
{
  "exchange": "binance",
  "pairs": ["BTC/USDT", "ETH/USDT"],
  "timeframes": ["1m", "5m"],
  "timerange": "20230101-20240601",
  "format": "feather",
  "market": "spot"
}
```

Response 200:
```json
{
  "quote_id": "qt_abc123",
  "estimated_rows": 5256000,
  "estimated_bytes": 89432100,
  "price_usdc": "0.087500",
  "pricing_breakdown": {
    "base_fee": "0.010000",
    "rows_fee": "0.052500",
    "pair_premium": "0.025000"
  },
  "ttl_seconds": 300,
  "payment_required": {
    "scheme": "x402",
    "network": "base",
    "asset": "USDC",
    "pay_to": "0x...",
    "max_amount": "0.087500"
  }
}
```

#### `POST /v1/download` (x402 → returns data)

Flow:
1. Agent sends request (same body as `/v1/quote`)
2. Service returns `402 Payment Required` + `PaymentRequired` challenge (with `quote_id`)
3. Agent retries with `X-PAYMENT` header (signed payment)
4. Service validates via facilitator, returns 200 (small sync) or 202 (async)
5. Agent polls `/v1/jobs/{id}` or receives webhook

Response 200 / 202:
```json
{
  "job_id": "job_xyz789",
  "status": "queued",
  "estimated_completion_s": 45,
  "poll_url": "/v1/jobs/job_xyz789",
  "stream_url": "/v1/jobs/job_xyz789/stream"
}
```

#### `GET /v1/jobs/{id}` (job_id auth → status / download link)

```json
{
  "job_id": "job_xyz789",
  "status": "completed",
  "progress": 1.0,
  "result": {
    "files": [
      {
        "name": "BTC_USDT-1m.feather",
        "bytes": 52428800,
        "sha256": "...",
        "download_url": "https://r2.../signed?expires=...",
        "expires_at": "2026-08-13T05:00:00Z"
      }
    ],
    "manifest_url": "https://r2.../manifest.json?..."
  },
  "payment_settled": true,
  "amount_paid_usdc": "0.087500"
}
```

### 2.2 Error responses

| HTTP | Code | When |
|---|---|---|
| 400 | `bad_request` | malformed timerange / unknown pair / unsupported timeframe |
| 402 | `payment_required` | no payment header on paid route |
| 402 | `payment_insufficient` | payment amount < quoted price |
| 402 | `payment_invalid` | facilitator rejected signature |
| 404 | `job_not_found` | unknown job id |
| 409 | `quote_expired` | quote_id older than TTL |
| 429 | `rate_limited` | per-IP / per-wallet quota exceeded |
| 500 | `internal_error` | upstream exchange failure |
| 503 | `upstream_unavailable` | binance/bybit/okx down |

---

## 3. Pricing Model (formula + examples)

### 3.1 Formula

```
price = base_fee
      + rows_fee
      + pair_premium
      + timeframe_multiplier
      + market_multiplier
      - free_tier_discount

where:
  base_fee              = $0.01                              # minimum charge per download
  rows_fee              = rows / 1_000_000 * $0.01           # per million K-lines
  pair_premium          = (pairs_count - 1) * $0.01          # multi-pair surcharge
  timeframe_multiplier  = {1m: 1.0, 5m: 0.6, 15m: 0.4, 1h: 0.25, 4h: 0.15, 1d: 0.05}
  market_multiplier     = {spot: 1.0, futures: 1.2}          # futures data costs more
  free_tier_discount    = max(0, free_quota_remaining)       # first-N free
```

### 3.2 Price table (worked examples)

| Scenario | rows | base | rows_fee | pair | total (USDC) |
|---|---|---|---|---|---|
| BTC/USDT 1 month 1m | ~43,200 | 0.01 | 0.000432 | 0.00 | **$0.010432** |
| BTC/USDT 1 year 1m | ~525,600 | 0.01 | 0.005256 | 0.00 | **$0.015256** |
| 5 majors 1 year 1m | ~2,628,000 | 0.01 | 0.02628 | 0.04 | **$0.07628** |
| 10 majors 1y 1m+5m | ~3,153,600 | 0.01 | 0.03153 | 0.09 | **$0.13153** |
| All coins 5y 1m (extreme) | ~50,000,000 | 0.01 | 0.50 | 0.95 | **$1.46** |

> Prices in USDC, 6 decimals, settled on-chain via x402.

### 3.3 Free tier (lead generation)

| Dimension | Free quota |
|---|---|
| Single-request rows | ≤ 50,000 (≈ 1 month 1m or 1 year 1h) |
| Per-IP/wallet rate | 10 requests/hour |
| Data freshness | delayed 24h (real-time paid only) |
| Export format | feather only (json/parquet paid) |

---

## 4. Deployment Architecture

### 4.1 Path comparison

| Dimension | Cloudflare Workers + R2 | Self-hosted VPS + Rust binary |
|---|---|---|
| Cold start | < 50ms | depends on deployment |
| x402 compatibility | native (CF has payment gateway integration) | self-host facilitator client |
| Storage cost | R2 $0.015/GB/month | varies with VPS tier |
| Compute cost | Workers request billing | flat VPS fee |
| Data residency | R2 (edge cached) | local disk + backup |
| Sweet spot | < 1000 jobs/day, bursty traffic | steady large traffic, full control |
| Risk | Workers CPU soft cap (10ms, long tasks need chunking) | higher ops overhead |
| Recommended stage | MVP → early productize | growth, enterprise customers |

**MVP recommendation**: Cloudflare Workers + R2 + D1 (job queue / audit log) + KV (quote cache).

### 4.2 Component layout

```
                ┌───────────────┐
                │  Agent / SDK  │
                └───────┬───────┘
                        │ HTTPS + x402
                        ↓
        ┌───────────────────────────────┐
        │  Cloudflare Worker (Axum-ish) │
        │  - quote handler              │
        │  - download handler           │
        │  - x402 middleware            │
        │  - job dispatcher             │
        └───────┬───────────┬───────────┘
                │           │
        ┌───────↓──┐   ┌────↓─────┐
        │ D1 (jobs)│   │ R2 (data)│
        └──────────┘   └──────────┘
                │
        ┌───────↓──────────────────┐
        │ ftdata-paid-cli (server) │
        │ - wraps existing ftdata  │
        │ - chunked async output   │
        └──────────────────────────┘
```

---

## 5. Directory Structure (minimum invasion)

```
ftdata/                              # existing repo (stays MIT)
├── ftdata-cli/                      # unchanged
├── ftdata-core/                     # unchanged
├── ftdata-http/                     # unchanged
├── ftdata-sources/                  # unchanged
├── ftdata-storage/                  # unchanged
├── ftdata-analysis/                 # unchanged
├── docs/
│   ├── DESIGN.md                    # existing
│   └── PAID_API_DESIGN.md           # this file (L1 output)
└── LICENSE                          # MIT (unchanged)

fq-data-paid/                        # NEW repo (independent commercial license)
├── ftdata-paid-cli/                 # wraps ftdata-cli into server mode
├── ftdata-paid-api/                 # Axum + x402 middleware
├── ftdata-paid-pricing/             # pricing library (pure fn, testable)
├── ftdata-paid-queue/               # D1/Postgres job queue
├── ftdata-paid-storage/             # R2/S3 signed download links
├── ftdata-paid-facilitator/         # x402 verification client
├── examples/
│   └── agent-client.ts              # reference Agent (TS)
└── LICENSE                          # commercial / dual
```

**Key decision (needs human input)**:
- standalone repo `fq-data-paid/` (recommended) vs monorepo subdir `paid/` (clearer community separation)

---

## 6. Three-Phase Rollout (with done signals)

### Phase 1 — MVP (1-2 weeks)

Done signals:
- [ ] `/v1/quote` works, free tier never bills
- [ ] `/v1/download` works with x402 middleware, Binance single-pair 1 month
- [ ] One TS Agent script can complete "quote → pay USDC → receive data" end-to-end
- [ ] Job lands on R2, signed link valid 5 minutes
- [ ] Simple web dashboard: revenue + request counts
- [ ] ≥ 3 real end-to-end tests including 402 retry

### Phase 2 — Productize (3-6 weeks)

Done signals:
- [ ] Full CLI → HTTP endpoint coverage (G2 100%)
- [ ] Dynamic pricing + free tier + rate limits
- [ ] Async job queue + progress webhook + SSE stream
- [ ] MCP server (stdio + http) → Agent-native integration
- [ ] Public API doc site (OpenAPI 3.1 + Swagger UI)
- [ ] Registered on x402 Bazaar / discovery service

### Phase 3 — Growth (ongoing)

Done signals:
- [ ] Freqtrade community post + integration examples
- [ ] Pre-packaged dataset marketplace (BTC/ETH full history bundles)
- [ ] Enterprise private deployment license (Helm chart + Docker)
- [ ] Integration with Cloudflare Monetization Gateway (if available)
- [ ] Revenue dashboard (MRR, ARPU, top requests, renewal rate)

---

## 7. Risk Register

| ID | Risk | Prob | Impact | Mitigation |
|---|---|---|---|---|
| R1 | Users bypass paid layer by hitting exchanges directly | High | High | Keep price low + add value (cleaning / merging) |
| R2 | x402 facilitator outage | Med | High | Multi-facilitator + manual fallback |
| R3 | Long jobs hit Workers CPU soft cap | High | Med | Chunk + async job + streaming |
| R4 | FX / price drift causes margin loss | Med | Med | 5-min quote TTL + price lock |
| R5 | Compliance (KYC/AML) exposure | Med | High | Initially serve only wallets > $10 |
| R6 | Exchange anti-bot blocks service IP | Med | Med | Reuse existing rate_limit + IP pool |
| R7 | Free tier abuse | High | Med | IP + wallet dual-axis rate limit |
| R8 | Signed download link leaks | Med | Med | Short TTL + single-use token + Referer check |

---

## 8. Budget & Constraints (loop-budget view)

| Item | Limit | Notes |
|---|---|---|
| This design token | ≤ 25k | one-shot design |
| L2 implementation token | 100k/day | consistent with existing loop |
| Sub-agent spawn | 0 (L1) / 2 (L2) | per `loop-constraints.md` |
| High-risk paths | all human review | pricing, facilitator, keys, compliance |
| Untouched paths | `.env`, `payments/`, `auth/` | declared in `loop-constraints.md` |

---

## 9. Open Questions (need human decision)

1. **Repo layout**: standalone `fq-data-paid/` repo (recommended) vs monorepo `paid/` subdir?
2. **Launch exchange coverage**: Binance only (recommended) or all three?
3. **Launch network**: Base (recommended) / Polygon / Solana / multi-chain?
4. **Facilitator choice**: Cloudflare (recommended) / Coinbase / self-hosted?
5. **Free tier granularity**: IP / wallet / dual (recommended)?
6. **MCP endpoint exposure**: enable in Phase 2 (recommended) or later?
7. **Data value-add SKUs from day 1**: cleaning/merging/validation (recommended Phase 2 split)?

---

## 10. Next Actions (L1 → L2 gate)

| Action | Do now? | Notes |
|---|---|---|
| Land this design at `docs/PAID_API_DESIGN.md` | **awaiting approval** | ready for draft PR |
| Open issue tracking Phase 1 | **awaiting approval** | break into sub-tasks |
| Scaffold `ftdata-paid-pricing` crate | L2 only | no code in L1 |
| Evaluate x402 SDKs | **awaiting approval** | can produce tech eval report |
| Pricing calculator unit tests | L2 only | with crate scaffold |

---

## 11. Current State Report

This is L1 report-only output:
- No code changed
- No branch created
- No PR opened
- No sub-agent spawned
- Token used: ≤ 25k (within budget)

Design is ready for human review. Next loop iteration requires explicit L1 → L2 gate approval.
