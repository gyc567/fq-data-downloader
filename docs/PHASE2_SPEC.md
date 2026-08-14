# ftdata-paid-api Phase 2 — Spec-Driven Development (spec-kit SDD)

> Method: Spec-Driven Development via spec-kit workflow
> Reference: https://www.topdigg.com/blog/2026-08-14-spec-kit-analysis
> Date: 2026-08-14
> Status: Draft — awaiting team consensus

---

## 1. Constitution (项目宪章)

### 1.1 核心原则

| # | 原则 | 描述 |
|---|------|------|
| C1 | CLI 保持 MIT | ftdata CLI 永远保持 MIT 协议和免费 |
| C2 | x402 原生支付 | 所有付费路由使用 x402 协议，payment evidence 在请求中 |
| C3 | Edge-first | x402 验证在 Cloudflare Workers edge 完成，origin 不处理未付费流量 |
| C4 | 声明式定价 | 定价规则作为版本化 JSON 配置，可通过 API/Terraform 部署 |
| C5 | AI-ready | 所有规格文档结构化，支持 AI 工具理解和生成代码 |

### 1.2 成功指标

- MRR ≥ $1000 (Phase 2 末)
- ≥ 10 个活跃 API 客户
- P99 延迟 < 500ms (quote), < 30s (download)
- 99.5% uptime

---

## 2. Specify (规格说明)

### 2.1 Phase 2 功能范围

#### P0 — 必须完成

| Feature | 描述 | 验收标准 |
|---------|------|---------|
| F1 | MCP Server | stdio + HTTP MCP 端点，Agent 可通过 MCP 调用所有 API |
| F2 | R2 集成 | 真实文件上传到 R2，生成 presigned URL |
| F3 | KV Pricing | Cloudflare KV 存储定价策略，实时热更新 |
| F4 | OpenAPI Docs | Swagger UI 公开，Phase 2 客户可自助接入 |
| F5 | Webhook 通知 | Job 完成/失败时主动推送 webhook |

#### P1 — 重要但非阻塞

| Feature | 描述 | 验收标准 |
|---------|------|---------|
| F6 | 多交易所支持 | Bybit + OKX 数据源（Q2 决策时只有 Binance） |
| F7 | 企业 API Key | 企业客户可使用 API Key 而非钱包认证 |
| F8 | 公开市场数据包 | 预打包数据集（BTC/ETH 完整历史）可购买 |

#### P2 — 优化

| Feature | 描述 | 验收标准 |
|---------|------|---------|
| F9 | Crawler Tier | $0.001/crawl 微定价层 |
| F10 | 实时数据 | 延迟 24h → 实时（额外收费） |

### 2.2 API 规格

#### MCP Server Endpoint

```
mcp STDIO: /v1/mcp (stdio mode)
mcp HTTP:  POST /v1/mcp (JSON-RPC 2.0)
```

**Tools:**
- `ftdata_quote` — 获取报价（免费）
- `ftdata_download` — 下载数据（x402 支付）
- `ftdata_jobs_list` — 列出任务
- `ftdata_jobs_status` — 查询任务状态
- `ftdata_reconcile` — 对账报告

**Request format:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "ftdata_download",
    "arguments": {
      "exchange": "binance",
      "pairs": ["BTC/USDT"],
      "timeframes": ["1m"],
      "timerange": "20230101-20230601"
    }
  }
}
```

#### R2 Upload Flow

```
1. Agent POST /v1/download (x402 verified)
2. Origin writes to /tmp/... then uploads to R2
3. R2 returns object key
4. Generate presigned URL (TTL=300s)
5. Update job.result.files[].download_url
6. Return presigned URL to agent
```

**R2 Configuration:**
- Bucket: `ftdata-paid-assets`
- Region: auto (Cloudflare global)
- Presigned TTL: 300 seconds
- Content-Type: based on format (feather/parquet/json)

#### Webhook Notification

```json
{
  "event": "job.completed",
  "job_id": "job_xxx",
  "timestamp": 1723612800,
  "data": {
    "status": "completed",
    "files": [...],
    "amount_paid_usdc": "0.010446"
  }
}
```

**Events:**
- `job.queued` — 任务已排队
- `job.running` — 开始处理
- `job.completed` — 完成
- `job.failed` — 失败

### 2.3 数据模型

#### JobResult (增强)

```rust
struct JobResult {
    files: Vec<FileEntry>,
    manifest_url: Option<String>,    // NEW: manifest.json presigned URL
    webhook_url: Option<String>,     // NEW: customer webhook
}
```

#### FileEntry (增强)

```rust
struct FileEntry {
    name: String,
    bytes: u64,
    sha256: String,
    download_url: String,            // R2 presigned URL
    r2_object_key: Option<String>,   // NEW: R2 object key
    expires_at: u64,
}
```

### 2.4 定价规格 (Phase 2)

**新增 SKU:**

| SKU | 公式 | 说明 |
|-----|------|------|
| 实时数据 | base + rows × 1.5 | 比延迟数据贵 50% |
| 多交易所 | base × 1.2 | 跨交易所聚合 |
| 企业包 | 自定义谈判 | 年度合同 |

**Policy JSON (KV):**
```json
{
  "policy_id": "pol_default_v2",
  "version": 2,
  "effective_since": "2026-09-01T00:00:00Z",
  "rules": [...],
  "skus": {
    "realtime": { "multiplier": 1.5 },
    "multi_exchange": { "multiplier": 1.2 }
  }
}
```

---

## 3. Plan (计划)

### 3.1 开发阶段

| 阶段 | 时间 | 任务 |
|------|------|------|
| Phase 2.1 | Week 1-2 | MCP Server + R2 集成 |
| Phase 2.2 | Week 3-4 | KV Pricing + Webhook |
| Phase 2.3 | Week 5-6 | OpenAPI Docs + 企业 API Key |
| Phase 2.4 | Week 7-8 | 集成测试 + 部署 |

### 3.2 依赖关系

```
MCP Server (F1)
    ↓
R2 Integration (F2) ← 需要 F1 完成
    ↓
KV Pricing (F3) ← 独立
    ↓
Webhook (F5) ← 需要 F2 完成
    ↓
OpenAPI Docs (F4) ← 独立
```

### 3.3 风险

| Risk | Impact | Mitigation |
|------|--------|------------|
| R2 权限配置错误 | 高 | 使用 wrangler dev 验证 |
| MCP 协议兼容性 | 中 | 参考官方 MCP SDK |
| KV 更新传播延迟 | 低 | 5min TTL + 显式 invalidate |

---

## 4. Tasks (任务清单)

### Phase 2.1 — MCP + R2

- [ ] [P0] 设计 MCP tool schema (5 tools)
- [ ] [P0] 实现 stdio MCP mode (`mcp` CLI mode)
- [ ] [P0] 实现 HTTP MCP mode (`POST /v1/mcp`)
- [ ] [P0] R2 client 集成 (wrangler R2 API)
- [ ] [P0] presigned URL 生成
- [ ] [P0] origin 输出写入 R2
- [ ] [P0] 更新 JobResult 添加 r2_object_key
- [ ] [P1] MCP E2E 测试 (mock agent)

### Phase 2.2 — KV + Webhook

- [ ] [P0] Cloudflare KV 绑定
- [ ] [P0] KV pricing policy 加载器
- [ ] [P0] policy 热更新机制
- [ ] [P0] Webhook 发送器 (async)
- [ ] [P0] Webhook 配置 (per-customer URL)
- [ ] [P1] Webhook 重试机制 (3 retries)
- [ ] [P2] Webhook 签名验证

### Phase 2.3 — Docs + API Key

- [ ] [P0] OpenAPI 3.1 spec 生成
- [ ] [P0] Swagger UI 挂载 (`/docs`)
- [ ] [P0] API Key 生成/验证
- [ ] [P0] API Key auth middleware
- [ ] [P1] API Key 管理 dashboard
- [ ] [P2] API Key 轮换

### Phase 2.4 — 集成 + 部署

- [ ] [P0] 完整 E2E 测试套件
- [ ] [P0] wrangler.toml 配置
- [ ] [P0] Workers 部署脚本
- [ ] [P0] D1 schema 迁移
- [ ] [P1] 监控 dashboard (KV stats, job counts)
- [ ] [P1] 告警规则 (error rate, latency)

---

## 5. Implement (实施检查)

*Phase 2 完成后填写的实施验证清单。*

### 5.1 验收检查

- [ ] MCP server 可通过 `npx @modelcontextprotocol/server-ftdata` 访问
- [ ] 下载的文件可在 R2 中找到，presigned URL 有效
- [ ] 定价策略可通过 KV 更新，无需重新部署
- [ ] Webhook 在任务完成时发送到配置 URL
- [ ] Swagger UI 可在 `/docs` 访问
- [ ] API Key 可在企业 dashboard 中生成和管理

### 5.2 性能基准

| 操作 | 目标 | 实际 |
|------|------|------|
| Quote | < 100ms | TBD |
| Download start | < 500ms | TBD |
| R2 presigned URL | < 50ms | TBD |
| Webhook delivery | < 1s | TBD |

---

## 6. AI Integration Notes (spec-kit SDD)

spec-kit 强调：明确的规格减少 AI "幻觉"，保持生成代码与需求一致。

**Phase 2 开发中 AI 使用指南：**

1. **Constitution** — 人类决策，不使用 AI
2. **Specify** — AI 可辅助编写 OpenAPI spec 和数据模型
3. **Plan** — AI 可生成任务分解，但需人类审核
4. **Implement** — AI 可辅助编写代码，但需规格对照验证

**规格优先原则：**
- 每次 PR 必须引用对应的规格条目
- AI 生成的代码必须标注来源规格
- 规格变更必须通过 PR review
