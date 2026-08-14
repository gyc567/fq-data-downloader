# ftdata Paid API — x402 Monetization Design

> Status: **L1 report-only design draft** (revised after Cloudflare Monetization Gateway audit)
> Loop: `design-triage` (custom)
> Date: 2026-08-13 (revised 2026-08-14)
> Author: ftdata contributors
> Reviewers: pending
> Audit reference: <https://blog.cloudflare.com/monetization-gateway/>

A design proposal for wrapping the existing `ftdata` CLI into a paid, x402-native HTTP data service. The CLI stays MIT/free. A new commercial layer (`fq-data-paid`) provides paid HTTP access on top.

---

## 0. Audit Summary (Cloudflare Monetization Gateway review)

The existing design is solid on pricing mechanics and x402 flow. After comparing against
Cloudflare's announced Monetization Gateway (MGW), three architectural gaps stand out:

| Gap | Impact | Fix needed |
|-----|--------|------------|
| No edge-native payment enforcement | Workers CPU wasted on x402 verification | Move x402 handshake to Cloudflare edge (Workers) before hitting origin |
| No declarative pricing rules | Hard to evolve pricing without code deploys | Adopt rules-as-code (JSON/YAML policy files, like MGW Terraform) |
| No hybrid auth (x402 + API key) | Enterprise customers can't use x402 without wallet infra | Add optional API key auth alongside x402 |
| Async notification is polling-only | Inefficient for agents, adds latency | Add webhook + SSE as first-class notification |
| No settlement reconciliation design | Revenue accuracy unknown post-deploy | Job receipts + periodic reconciliation endpoint |

Additional smaller gaps: Workers CPU 10ms cap not explicitly addressed, quote TTL
should be tied to Workers' own cost window, no multi-exchange phased roadmap,
free-tier as lead-gen is undermonetized (MGW would charge crawlers something).

---

## 0a. Loop Engineering Meta

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
| G7 *(new)* | Edge-native x402 enforcement | x402 verification happens at Cloudflare Workers edge, not origin |
| G8 *(new)* | Declarative pricing rules | Pricing policies expressed as versioned config, deployable via API/Terraform |

---

## 1. Business Positioning

**Core principle**: the CLI stays MIT and free forever. New value layer is independently monetized.

```
┌──────────────────────────────────────────┐
│  ftdata open-source CLI (forever free)   │
│  - 6 crates, zero changes                │
│  - GitHub Releases continue              │
└──────────────────────────────────────────┘
                    ↓ wrapper layer (new)
┌──────────────────────────────────────────┐
│  fq-data-paid (new crate, commercial)   │
│  - HTTP API + x402 middleware            │
│  - standalone repo or subdir            │
│  - independent commercial LICENSE        │
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
- Edge enforcement (Workers) means origin never sees unpaid traffic — the pattern
  Cloudflare's Monetization Gateway endorses: **payment evidence lives in the request,
  not in your origin's billing system.**

---

## 2. API Surface (CLI → HTTP mapping)

| CLI command | HTTP path | Method | Auth | Notes |
|---|---|---|---|---|
| `download` | `/v1/download` | POST | x402 / API key | main entry, billed by data volume |
| `update` | `/v1/update` | POST | x402 / API key | incremental, flat low price |
| `plan` | `/v1/quote` | POST | free | returns price preview, no download |
| `verify` | `/v1/verify` | POST | x402 / API key | verify local or R2 file |
| `gaps` | `/v1/gaps` | POST | free | probe only, lead-magnet |
| `repair` | `/v1/repair` | POST | x402 / API key | fill gaps, billed by gap size |
| `info` | `/v1/info` | GET | free | service meta + price table |
| `cancel` | `/v1/jobs/{id}` | DELETE | x402 / API key | cancel queued job |
| `status` | `/v1/jobs/{id}` | GET | job_id | job status query |
| *(new)* `reconcile` | `/v1/reconcile` | GET | admin | settlement reconciliation (internal) |
| *(new)* `policies` | `/v1/policies` | GET | admin | current pricing rules (internal) |

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
  "policy_id": "pol_default_v1",
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
  },
  "x402_headers": {
    "scheme": "x402",
    "pay_to": "0x...",
    "max_payment_amount": "0.087500",
    "asset": "USDC",
    "nonce": "0x..."
  }
}
```

> **Note on edge enforcement**: In the Cloudflare Workers path, `/v1/quote` runs
> edge-side. The Worker intercepts the POST, runs the pricing logic, returns the
> `PaymentRequired` challenge without ever forwarding to origin. Origin only receives
> requests that have already passed x402 verification.

#### `POST /v1/download` (x402 → returns data)

Flow (revised for edge enforcement):
1. Agent sends request to Workers edge
2. Workers checks `X-Payment` header; if absent, returns `402 Payment Required`
   + challenge directly from edge (no origin call)
3. Workers validates payment via x402 facilitator (sub-second, edge-proximate)
4. If valid: Workers forwards to origin (ftdata-paid server) OR serves from R2 cache
5. Small sync responses (< Workers CPU budget) return 200 directly
6. Large responses return 202 with job_id; Workers handles SSE progress stream

Response 200 / 202:
```json
{
  "job_id": "job_xyz789",
  "status": "queued",
  "estimated_completion_s": 45,
  "poll_url": "/v1/jobs/job_xyz789",
  "stream_url": "/v1/jobs/job_xyz789/stream",
  "webhook_url": "https://agent.example/webhook/job_xyz789"
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
  "amount_paid_usdc": "0.087500",
  "receipt": {
    "receipt_id": "rcpt_xyz789",
    "tx_hash": "0x...",
    "settled_at": "2026-08-13T04:30:00Z",
    "facilitator": "cloudflare"
  }
}
```

#### `GET /v1/jobs/{id}/stream` (SSE — revised)

SSE stream for real-time progress (binds to Workers SSE support):
```
event: progress
data: {"progress": 0.25, "stage": "downloading", "bytes_done": 13107200}

event: progress
data: {"progress": 0.50, "stage": "validating", "bytes_done": 26214400}

event: complete
data: {"status": "completed", "download_url": "https://r2.../signed?..."}

event: error
data: {"code": "upstream_unavailable", "message": "Binance API rate limited"}
```

### 2.2 Declarative Pricing Rules (new section)

Inspired by Cloudflare's rules-as-code model, pricing policies are versioned JSON
documents deployed via API or Terraform, not hard-coded in the binary.

**Policy structure** (`pol_default_v1`):
```json
{
  "policy_id": "pol_default_v1",
  "version": 1,
  "effective_since": "2026-09-01T00:00:00Z",
  "rules": [
    {
      "route": "/v1/download",
      "method": ["POST"],
      "auth": ["x402", "api_key"],
      "pricing": {
        "model": "volume_based",
        "base_fee_usdc": "0.010",
        "per_million_rows_usdc": "0.010",
        "max_price_usdc": "10.000"
      }
    },
    {
      "route": "/v1/repair",
      "method": ["POST"],
      "auth": ["x402", "api_key"],
      "pricing": {
        "model": "volume_based",
        "base_fee_usdc": "0.005",
        "per_million_rows_usdc": "0.005"
      }
    },
    {
      "route": "/v1/gaps",
      "method": ["POST"],
      "auth": ["free"],
      "pricing": {
        "model": "free",
        "rate_limit_per_hour": 10
      }
    }
  ],
  "exemptions": [
    {
      "condition": "wallet == 0x_WHITELISTED",
      "discount": 1.0
    }
  ],
  "updated_at": "2026-08-13T12:00:00Z",
  "updated_by": "ops@ftdata"
}
```

**Policy lifecycle**:
- Stored in D1 / KV with version history
- Workers reads policy at request time (KV, <1ms)
- Policy changes take effect at `effective_since` (no sudden price surprises)
- Admin API (`GET /v1/policies`) for introspection; no write endpoint in MVP

**Why this matters**: Pricing becomes a deployment artifact. The team can adjust
prices, add SKU-specific rules (e.g., futures surcharge), or add promotional
discounts without redeploying binary code — exactly the model Cloudflare's MGW enables.

### 2.3 Error responses

| HTTP | Code | When |
|---|---|---|
| 400 | `bad_request` | malformed timerange / unknown pair / unsupported timeframe |
| 401 | `unauthorized` | missing or invalid API key (enterprise path) |
| 402 | `payment_required` | no payment header on paid route |
| 402 | `payment_insufficient` | payment amount < quoted price |
| 402 | `payment_invalid` | facilitator rejected signature |
| 402 | `payment_expired` | x402 nonce reused (replay protection) |
| 404 | `job_not_found` | unknown job id |
| 409 | `quote_expired` | quote_id older than TTL |
| 429 | `rate_limited` | per-IP / per-wallet / per-API-key quota exceeded |
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
      + compute_bonus            -- NEW: CPU-time surcharge for large requests
      - free_tier_discount

where:
  base_fee              = $0.01                              # minimum charge per download
  rows_fee              = rows / 1_000_000 * $0.01           # per million K-lines
  pair_premium          = (pairs_count - 1) * $0.01          # multi-pair surcharge
  timeframe_multiplier  = {1m: 1.0, 5m: 0.6, 15m: 0.4, 1h: 0.25, 4h: 0.15, 1d: 0.05}
  market_multiplier     = {spot: 1.0, futures: 1.2}         # futures data costs more
  compute_bonus         = max(0, cpu_ms - 5) * $0.0001      # Workers CPU overage (new)
  free_tier_discount    = max(0, free_quota_remaining)       # first-N free
```

> **Cloudflare MGW parallel**: MGW charges based on compute/resource consumed.
> For ftdata, the natural proxy is rows fetched + CPU time on Workers. The
> `compute_bonus` term ensures large requests that exceed Workers' 10ms soft cap
> pay proportionally more — or route to async (202) automatically.

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
| Max concurrent jobs | 1 |

> **MGW parallel**: Cloudflare's MGW explicitly enables "charge crawlers for content"
> even at low volumes. The free tier here is a lead-gen funnel, not free-for-all.
> Consider adding a "crawler tier" ($0.001/crawl) for high-volume bot access
> (MGW's model: charge something, not nothing).

---

## 4. Deployment Architecture

### 4.1 Path comparison

| Dimension | Cloudflare Workers + R2 | Self-hosted VPS + Rust binary |
|---|---|---|
| Cold start | < 50ms | depends on deployment |
| x402 compatibility | native + edge-native (MGW-compatible) | self-host facilitator client |
| Storage cost | R2 $0.015/GB/month | varies with VPS tier |
| Compute cost | Workers request billing | flat VPS fee |
| Data residency | R2 (edge cached) | local disk + backup |
| Payment enforcement | Edge (Workers intercepts before origin) | Origin-side (facilitator SDK) |
| Sweet spot | < 1000 jobs/day, bursty traffic, agents | steady large traffic, full control |
| Risk | Workers CPU soft cap (10ms, long tasks need chunking) | higher ops overhead |
| Recommended stage | MVP → early productize | growth, enterprise customers |

**MVP recommendation**: Cloudflare Workers + R2 + D1 (job queue / audit log) + KV (pricing policy cache).

### 4.2 Component layout (revised for edge enforcement)

```
Agent / SDK
    │ HTTPS + x402 headers
    ▼
┌─────────────────────────────────────────────┐
│  Cloudflare Workers (edge — payment gate)   │
│  - pricing policy (from KV)                 │
│  - x402 payment verification (sub-ms)       │
│  - Workers KV (quote cache, TTL=300s)       │
│  - D1 (job queue, receipts, audit log)     │
│  - SSE handler (real-time progress)        │
│  - R2 signed URLs (download delivery)      │
└──────┬──────────────────────────┬────────────┘
       │                          │
  ┌────▼────┐              ┌──────▼──────┐
  │  R2     │              │  D1 / KV   │
  │(data)   │              │(queue,     │
  │         │              │ policy,    │
  │         │              │ receipts)  │
  └─────────┘              └────────────┘
       │
  ┌────▼───────────────────────────────────┐
  │  ftdata-paid origin (Rust server)     │
  │  - wraps ftdata-cli into server mode  │
  │  - receives only pre-verified requests │
  │  - chunked async output               │
  │  - writes to R2 directly              │
  └────────────────────────────────────────┘
```

**Key edge vs. origin separation**:
- Workers: x402 verify + quote compute + SSE + signed URL generation + rate limit
- Origin (Rust): actual data download + validation + R2 write + job state update
- This means origin never sees a request that hasn't already committed to pay.
  Matches MGW's stated benefit: *"protect your origin from high payment volumes."*

### 4.3 Workers CPU budget design (new)

Workers has a 10ms CPU soft cap per request. To handle requests that exceed this:

| Request type | Strategy |
|---|---|
| Quote (`/v1/quote`) | Always sync (<1ms CPU) |
| Small download (1 pair, 1 month) | Sync (Workers serves from R2 cache or origin) |
| Medium (1 pair, 1 year) | Sync if <10ms; else async 202 |
| Large (multi-pair, multi-year) | Always async 202; SSE stream for progress |
| Repair | Async 202; chunked by gap region |

Detection: Workers measures CPU time before forwarding; if >8ms, switch to async
and return 202 immediately. The SSE `stream_url` provides real-time progress.

---

## 5. Directory Structure (minimum invasion)

```
ftdata/                              # existing repo (stays MIT)
├── ftdata-cli/                      # unchanged
├── ftdata-core/                     # unchanged
├── ftdata-http/                     # unchanged (client lib, reuses)
├── ftdata-sources/                  # unchanged
├── ftdata-storage/                  # unchanged
├── ftdata-analysis/                 # unchanged
├── docs/
│   ├── DESIGN.md                    # existing
│   └── PAID_API_DESIGN.md           # this file (revised L1 output)
└── LICENSE                          # MIT (unchanged)

fq-data-paid/                        # NEW repo (independent commercial license)
├── ftdata-paid-cli/                 # wraps ftdata-cli into server mode
├── ftdata-paid-api/                 # Axum + x402 middleware
├── ftdata-paid-pricing/             # pricing library (pure fn, testable)
├── ftdata-paid-queue/               # D1/Postgres job queue
├── ftdata-paid-storage/             # R2/S3 signed download links
├── ftdata-paid-facilitator/         # x402 verification client
├── ftdata-paid-edge/                # Cloudflare Workers (new, edge layer)
├── policies/                        # declarative pricing rules (JSON)
│   ├── pol_default_v1.json
│   └── pol_enterprise_v1.json
├── terraform/                      # Terraform for policy deployment (new)
│   └── policies.tf
├── examples/
│   └── agent-client.ts              # reference Agent (TS)
└── LICENSE                          # commercial / dual
```

**Key decision (needs human input)**:
- standalone repo `fq-data-paid/` (recommended) vs monorepo subdir `paid/` (clearer community separation)
- Workers code in `ftdata-paid-edge/` (recommended for MVP clarity)

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
- [ ] **Workers edge enforces x402 before origin is called** (new)
- [ ] **Quote result cached in Workers KV with TTL=300s** (new)

### Phase 2 — Productize (3-6 weeks)

Done signals:
- [ ] Full CLI → HTTP endpoint coverage (G2 100%)
- [ ] Dynamic pricing + free tier + rate limits
- [ ] Async job queue + progress webhook + SSE stream
- [ ] MCP server (stdio + http) → Agent-native integration
- [ ] Public API doc site (OpenAPI 3.1 + Swagger UI)
- [ ] Registered on x402 Bazaar / discovery service
- [ ] **Declarative pricing rules in KV (pol_default_v1)** (new)
- [ ] **Settlement reconciliation endpoint (`/v1/reconcile`)** (new)
- [ ] **API key auth path for enterprise** (new)

### Phase 3 — Growth (ongoing)

Done signals:
- [ ] Freqtrade community post + integration examples
- [ ] Pre-packaged dataset marketplace (BTC/ETH full history bundles)
- [ ] Enterprise private deployment license (Helm chart + Docker)
- [ ] Integration with Cloudflare Monetization Gateway (GA, when available)
- [ ] Revenue dashboard (MRR, ARPU, top requests, renewal rate)
- [ ] **Multi-exchange support (Bybit, OKX)** (new)
- [ ] **Crawler-tier micro-pricing** ($0.001/crawl for high-volume bots) (new)

---

## 7. Settlement & Reconciliation (new section)

MGW settles peer-to-peer via blockchain. For revenue accounting, ftdata-paid needs
a reconciliation layer.

### 7.1 Receipt model

Every completed job emits a `receipt`:
```json
{
  "receipt_id": "rcpt_xyz789",
  "job_id": "job_xyz789",
  "paid_by": "0xabc...def",
  "amount_usdc": "0.087500",
  "tx_hash": "0x...",
  "network": "base",
  "facilitator": "cloudflare",
  "settled_at": "2026-08-13T04:30:00Z",
  "policy_id": "pol_default_v1",
  "quote_id": "qt_abc123",
  "exchange": "binance",
  "pairs": ["BTC/USDT"],
  "rows": 525600
}
```

### 7.2 Reconciliation endpoint

`GET /v1/reconcile?since=2026-08-01T00:00:00Z&until=2026-08-31T23:59:59Z`

Returns:
```json
{
  "period": {"since": "2026-08-01T00:00:00Z", "until": "2026-08-31T23:59:59Z"},
  "jobs_total": 1423,
  "jobs_completed": 1398,
  "jobs_failed": 25,
  "revenue_total_usdc": "87.543210",
  "receipts": [/* array of receipts */],
  "facilitator_fees_usdc": "0.875432",
  "net_revenue_usdc": "86.667778",
  "by_exchange": {"binance": "87.543210"},
  "by_policy": {"pol_default_v1": "87.543210"}
}
```

This is internal/admin only (no external exposure in MVP).

---

## 8. Risk Register (revised)

| ID | Risk | Prob | Impact | Mitigation |
|---|---|---|---|---|
| R1 | Users bypass paid layer by hitting exchanges directly | High | High | Keep price low + add value (cleaning / merging) |
| R2 | x402 facilitator outage | Med | High | Multi-facilitator fallback + manual webhook fallback |
| R3 | Long jobs hit Workers CPU soft cap | High | Med | Auto-async at 8ms threshold + SSE for progress |
| R4 | FX / price drift causes margin loss | Med | Med | 5-min quote TTL + price lock |
| R5 | Compliance (KYC/AML) exposure | Med | High | Initially serve only wallets > $10 / per-tx limits |
| R6 | Exchange anti-bot blocks service IP | Med | Med | Reuse existing rate_limit + IP pool + residential proxy fallback |
| R7 | Free tier abuse | High | Med | IP + wallet dual-axis rate limit + request size cap |
| R8 | Signed download link leaks | Med | Med | Short TTL (5min) + single-use token + Referer check |
| R9 *(new)* | Pricing policy deployed with errors | Med | High | Policy version history in D1 + rollback endpoint |
| R10 *(new)* | Workers edge x402 SDK mismatch with facilitator | Med | High | Pin facilitator version + integration tests in CI |
| R11 *(new)* | Settlement discrepancy (Jobs completed ≠ on-chain receipts) | Med | Med | Daily reconciliation job + alerting on >1% delta |

---

## 9. Budget & Constraints (loop-budget view)

| Item | Limit | Notes |
|---|---|---|
| This design token | ≤ 25k | one-shot design |
| L2 implementation token | 100k/day | consistent with existing loop |
| Sub-agent spawn | 0 (L1) / 2 (L2) | per `loop-constraints.md` |
| High-risk paths | all human review | pricing, facilitator, keys, compliance |
| Untouched paths | `.env`, `payments/`, `auth/` | declared in `loop-constraints.md` |

---

## 10. Open Questions (need human decision)

1. **Repo layout**: standalone `fq-data-paid/` repo (recommended) vs monorepo `paid/` subdir?
2. **Launch exchange coverage**: Binance only (recommended) or all three?
3. **Launch network**: Base (recommended) / Polygon / Solana / multi-chain?
4. **Facilitator choice**: Cloudflare (recommended) / Coinbase / self-hosted?
5. **Free tier granularity**: IP / wallet / dual (recommended)?
6. **MCP endpoint exposure**: enable in Phase 2 (recommended) or later?
7. **Data value-add SKUs from day 1**: cleaning/merging/validation (recommended Phase 2 split)?
8. **Enterprise API key path**: include in Phase 1 (recommended) or Phase 2?
9. **Settlement reconciliation**: admin-only in Phase 1 (recommended) or expose to customers?
10. **Crawler-tier pricing**: add $0.001/crawl micro-tier in Phase 3 (recommended)?
11. **Workers pricing rule storage**: KV (recommended) or D1 for policy + version history?

---

## 11. Next Actions (L1 → L2 gate)

| Action | Do now? | Notes |
|---|---|---|
| Land this design at `docs/PAID_API_DESIGN.md` | **awaiting approval** | ready for draft PR |
| Open issue tracking Phase 1 | **awaiting approval** | break into sub-tasks |
| Scaffold `ftdata-paid-pricing` crate | L2 only | no code in L1 |
| Evaluate x402 SDKs (Workers SDK vs Rust facilitator) | **awaiting approval** | can produce tech eval report |
| Pricing calculator unit tests | L2 only | with crate scaffold |
| Write `ftdata-paid-edge` Workers scaffolding | L2 only | edge payment enforcement spec |
| Write Terraform module for policy deployment | L2 only | Cloudflare MGW-compatible rules-as-code |

---

## 12. Current State Report

This is L1 report-only output:
- No code changed
- No branch created
- No PR opened
- No sub-agent spawned
- Token used: ≤ 25k (within budget)

Design is ready for human review. Next loop iteration requires explicit L1 → L2 gate approval.

**Revisions from this audit**:
- Section 0a: Added G7 (edge-native enforcement) and G8 (declarative pricing rules)
- Section 2: Added `/v1/reconcile` and `/v1/policies` endpoints; revised download flow for edge enforcement; added SSE stream spec; added `receipt` to job response; added error `payment_expired`
- Section 2.2: **New section** — Declarative Pricing Rules (JSON policies, KV storage, Terraform-ready)
- Section 3.1: Added `compute_bonus` term for Workers CPU overage
- Section 3.3: Added crawler-tier micro-pricing note (MGW parallel)
- Section 4.2: Revised component layout showing edge vs. origin separation
- Section 4.3: **New section** — Workers CPU budget design (sync vs. async threshold)
- Section 6: Added done signals for edge enforcement, KV caching, reconciliation, API keys, multi-exchange, crawler-tier
- Section 7: **New section** — Settlement & Reconciliation (receipt model + endpoint)
- Risk register: Added R9 (policy deployment error), R10 (SDK mismatch), R11 (reconciliation discrepancy)
- Open questions: Added Q8–Q11
- Next actions: Added Workers scaffolding and Terraform policy module
