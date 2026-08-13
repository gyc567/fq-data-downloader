#!/usr/bin/env bash
# Extract ftdata-paid crates from the monorepo into a standalone repo.
#
# Implements DECISIONS.md Q1 (standalone `fq-data-paid/` repo).
# Per loop-constraints: the resulting repo is NOT pushed automatically.
# After running this script, the user manually:
#
#   cd /path/to/standalone
#   git remote add origin git@github.com:gyc567/fq-data-paid.git
#   git push -u origin main
#
# Usage:
#   bash scripts/extract-standalone.sh /path/to/standalone
#
# The script:
#   1. Verifies the source repo (must be on feat/paid-mvp or have 30 commits)
#   2. Creates the destination dir + a fresh git repo
#   3. Uses `git archive` to extract the current HEAD of feat/paid-mvp
#   4. Filters to keep only paid crates + relevant docs/scripts
#   5. Rewrites the root Cargo.toml to drop non-paid workspace members
#   6. Adds a README pointing back to the monorepo for the MIT CLI crates
#   7. Creates a fresh initial commit
#   8. Prints the push command

set -euo pipefail

DEST="${1:?usage: bash scripts/extract-standalone.sh /path/to/standalone}"
SRC="$(pwd)"

# 1. Sanity check the source
if ! git rev-parse --verify feat/paid-mvp >/dev/null 2>&1; then
    echo "ERROR: feat/paid-mvp branch not found. Run from the ftdata monorepo root." >&2
    exit 1
fi
COMMIT_COUNT=$(git rev-list --count feat/paid-mvp)
echo "[src] feat/paid-mvp has $COMMIT_COUNT commits"
if [[ "$COMMIT_COUNT" -lt 25 ]]; then
    echo "WARNING: expected ~30 commits on feat/paid-mvp. Run on the latest." >&2
fi

# 2. Create destination
if [[ -e "$DEST" ]]; then
    echo "ERROR: $DEST already exists. Remove it first or pick another path." >&2
    exit 1
fi
mkdir -p "$DEST"
echo "[dest] creating fresh git repo at $DEST"
cd "$DEST"
git init -q -b main
git config user.email "extraction@ftdata.local"
git config user.name "ftdata extraction"

# 3 + 4. git archive then filter
# Note: Cargo.lock is gitignored upstream, so it isn't in the archive.
# `cargo build` will regenerate it on the first run in the standalone repo.
echo "[extract] using git archive to copy HEAD of feat/paid-mvp"
TMP_TAR="$(mktemp -t ftdata-archive-XXXXXX).tar"
trap 'rm -f "$TMP_TAR"' EXIT
git -C "$SRC" archive --format=tar feat/paid-mvp > "$TMP_TAR"

# Keep only the paid surface
echo "[filter] extracting paid crates + relevant docs/scripts"
tar -xf "$TMP_TAR" \
    ftdata-paid-pricing \
    ftdata-paid-facilitator \
    ftdata-paid-api \
    scripts/smoke.sh \
    docs/PAID_API_DESIGN.md \
    docs/DECISIONS.md \
    Cargo.toml

# 5. Rewrite root Cargo.toml: drop non-paid members, add new [package]
echo "[rewrite] root Cargo.toml: drop non-paid workspace members"
cat > Cargo.toml << 'EOF'
[workspace]
resolver = "2"
members = [
    "ftdata-paid-pricing",
    "ftdata-paid-facilitator",
    "ftdata-paid-api",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["ftdata contributors"]
license = "Commercial"
repository = "https://github.com/gyc567/fq-data-paid"

[workspace.dependencies]
tokio = { version = "1.40", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0"
async-trait = "0.1"
axum = { version = "0.7", features = ["macros"] }
tower = { version = "0.5", features = ["util"] }
http = "1"
http-body-util = "0.1"
uuid = { version = "1.10", features = ["v4"] }
blake3 = "1.5"
chrono = { version = "0.4", features = ["serde"] }
indicatif = "0.17"
dashmap = "6"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
rand = "0.8"
once_cell = "1.19"
parking_lot = "0.12"
futures = "0.3"
url = "2.5"
tokio-util = { version = "0.7", features = ["io"] }
EOF

# 6. Standalone README
cat > README.md << 'EOF'
# fq-data-paid — x402 paid HTTP layer for ftdata

The **paid** commercial layer on top of the MIT-licensed `ftdata` CLI.

## What's in this repo

```
ftdata-paid-pricing/       # pure pricing library (§3.1 formula)
ftdata-paid-facilitator/   # x402 PaymentVerifier trait + MockFacilitator
ftdata-paid-api/           # Axum HTTP API + ftdata-paid-server binary
ftdata-paid-api/docker/    # Dockerfile for Cloudflare Workers deploy
ftdata-paid-api/examples/  # TS Agent client reference
scripts/smoke.sh           # bash end-to-end smoke test
docs/
    PAID_API_DESIGN.md     # design doc (x402 + Cloudflare MGW)
    DECISIONS.md           # Q1-Q11 decisions log
```

## Build

```bash
cargo build --release
./target/release/ftdata-paid-server
```

## Test

```bash
cargo test --workspace        # cargo + integration
bash scripts/smoke.sh         # bash + curl E2E
```

## Related

- Upstream MIT-licensed `ftdata` CLI monorepo: gyc567/fq-data-downloader
- Design rationale: docs/PAID_API_DESIGN.md
- Q1-Q11 decisions: docs/DECISIONS.md

## License

Commercial. The MIT-licensed CLI core (ftdata-cli, ftdata-core, etc.)
remains in the upstream monorepo.
EOF

# Initial commit
git add -A
git commit -q -m "Extract paid crates from ftdata monorepo

Auto-generated by scripts/extract-standalone.sh.

Source: gyc567/fq-data-downloader @ feat/paid-mvp (HEAD = $(git -C "$SRC" rev-parse --short feat/paid-mvp))

This is the first commit of the standalone fq-data-paid/ repo.
The upstream monorepo retains the MIT-licensed CLI crates (ftdata-cli,
ftdata-core, ftdata-http, ftdata-sources, ftdata-storage, ftdata-analysis)
plus the docs/PAID_API_DESIGN.md and docs/DECISIONS.md shared between
both repos.

Next step for the operator: run 'cargo build' to regenerate Cargo.lock,
then push the repo to GitHub (requires L3 gate approval per loop-constraints)."

# 8. Print the push command
echo ""
echo "=========================================="
echo "Extraction complete. Standalone repo at:"
echo "  $DEST"
echo ""
echo "Commits in standalone repo:"
git log --oneline | head -5
echo ""
echo "To push (requires L3 gate approval):"
echo "  cd $DEST"
echo "  git remote add origin git@github.com:gyc567/fq-data-paid.git"
echo "  git push -u origin main"
echo "=========================================="
