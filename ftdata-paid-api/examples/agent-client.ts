#!/usr/bin/env node
/**
 * ftdata-paid TS Agent client (reference example).
 *
 * Demonstrates the full x402 payment flow against `ftdata-paid-api`:
 * 1. POST /v1/download without payment → receive 402 with challenge
 * 2. Build a PaymentProof covering the challenge's max_amount
 * 3. Retry with X-PAYMENT header → receive 202 + job_id
 * 4. Poll /v1/jobs/{id} until completed → read result.download_url
 *
 * Run with:
 *   npx ts-node examples/agent-client.ts
 *   # or
 *   FTDATA_URL=http://localhost:8080 node --import tsx examples/agent-client.ts
 *
 * Requirements: a running `ftdata-paid-server` (cargo run --bin ftdata-paid-server).
 *
 * NOTE: This file is intentionally NOT compiled by `cargo test`. It is a
 * developer-facing example that demonstrates the wire-level flow against
 * the mock facilitator shipped with the binary. Replace the `signPaymentProof`
 * stub with a real signer (viem, ethers, etc.) when wiring a production agent.
 */

const FTDATA_URL = process.env.FTDATA_URL || "http://127.0.0.1:8080";

// A minimal "signer" — for production use viem/ethers to produce a real
// EIP-3009 authorization. The mock facilitator accepts any signature.
function signPaymentProof(challenge: any, payerAddress: string): string {
  const proof = {
    scheme: "exact",
    network: challenge.network,
    asset: challenge.asset,
    payer: payerAddress,
    amount: challenge.max_amount,
    quote_id: challenge.quote_id,
    signature: `0xMOCK_SIG_${Date.now()}`,
    nonce: `n_${Date.now()}`,
    valid_until: Math.floor(Date.now() / 1000) + 600,
  };
  // The server accepts the X-PAYMENT header as either raw JSON or
  // base64-url(JSON). Raw JSON is fine for development.
  return JSON.stringify(proof);
}

interface QuoteRequest {
  exchange: string;
  pairs: string[];
  timeframes: string[];
  timerange: string;
  market?: string;
}

interface PaymentRequired {
  scheme: string;
  network: string;
  asset: string;
  pay_to: string;
  max_amount: string;
  quote_id: string;
  expires_at: number;
}

async function getChallenge(body: QuoteRequest): Promise<PaymentRequired> {
  const res = await fetch(`${FTDATA_URL}/v1/download`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (res.status !== 402) {
    throw new Error(`Expected 402, got ${res.status}: ${await res.text()}`);
  }
  const json: any = await res.json();
  return json.payment_required;
}

async function submitWithPayment(
  body: QuoteRequest,
  proofJson: string
): Promise<any> {
  const res = await fetch(`${FTDATA_URL}/v1/download`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-payment": proofJson,
    },
    body: JSON.stringify(body),
  });
  if (res.status !== 202) {
    throw new Error(`Expected 202, got ${res.status}: ${await res.text()}`);
  }
  return res.json();
}

async function pollJob(jobId: string): Promise<any> {
  for (let i = 0; i < 100; i++) {
    await new Promise((r) => setTimeout(r, 250));
    const res = await fetch(`${FTDATA_URL}/v1/jobs/${jobId}`);
    if (!res.ok) {
      throw new Error(`Job poll failed: ${res.status}`);
    }
    const job: any = await res.json();
    if (job.status === "completed") return job;
    if (job.status === "failed") throw new Error(`Job failed: ${job.error}`);
  }
  throw new Error(`Job ${jobId} did not complete in time`);
}

async function main() {
  const body: QuoteRequest = {
    exchange: "binance",
    pairs: ["BTC/USDT"],
    timeframes: ["1m"],
    timerange: "20230101-20230201",
  };

  console.log("[1/4] requesting challenge...");
  const challenge = await getChallenge(body);
  console.log(`       quote_id=${challenge.quote_id} amount=${challenge.max_amount}`);

  console.log("[2/4] signing payment proof...");
  const proofJson = signPaymentProof(challenge, "0xAGENT_WALLET");

  console.log("[3/4] submitting with X-PAYMENT...");
  const accepted = await submitWithPayment(body, proofJson);
  console.log(`       job_id=${accepted.job_id} amount=${accepted.amount_paid_usdc}`);

  console.log("[4/4] polling job until completed...");
  const job = await pollJob(accepted.job_id);
  console.log(`       status=${job.status} progress=${job.progress}`);
  if (job.result?.files?.length > 0) {
    const file = job.result.files[0];
    console.log(`       file=${file.name} bytes=${file.bytes}`);
    console.log(`       download_url=${file.download_url}`);
  }
}

main().catch((err) => {
  console.error("ERROR:", err.message);
  process.exit(1);
});
