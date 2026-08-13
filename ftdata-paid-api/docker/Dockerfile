# Multi-stage Dockerfile for ftdata-paid-server.
#
# Build:   docker build -t ftdata-paid-server -f ftdata-paid-api/docker/Dockerfile .
# Run:     docker run --rm -p 8080:8080 ftdata-paid-server
# Custom:  docker run --rm -p 9090:8080 -e FTDATA_BIND=0.0.0.0:8080 ftdata-paid-server

# ---- builder ----
FROM rust:1.82-slim AS builder

WORKDIR /build

# Cache dependency layer
COPY Cargo.toml Cargo.lock ./
COPY ftdata-cli/Cargo.toml ftdata-cli/
COPY ftdata-core/Cargo.toml ftdata-core/
COPY ftdata-http/Cargo.toml ftdata-http/
COPY ftdata-sources/Cargo.toml ftdata-sources/
COPY ftdata-storage/Cargo.toml ftdata-storage/
COPY ftdata-analysis/Cargo.toml ftdata-analysis/
COPY ftdata-paid-pricing/Cargo.toml ftdata-paid-pricing/
COPY ftdata-paid-facilitator/Cargo.toml ftdata-paid-facilitator/
COPY ftdata-paid-api/Cargo.toml ftdata-paid-api/

# Create stub src files so `cargo metadata` resolves deps without full source.
RUN mkdir -p ftdata-cli/src ftdata-core/src ftdata-http/src ftdata-sources/src \
    ftdata-storage/src ftdata-analysis/src \
    ftdata-paid-pricing/src ftdata-paid-facilitator/src ftdata-paid-api/src && \
    for crate in ftdata-cli ftdata-core ftdata-http ftdata-sources \
        ftdata-storage ftdata-analysis \
        ftdata-paid-pricing ftdata-paid-facilitator ftdata-paid-api; do \
      echo "fn main() {}" > $crate/src/main.rs; \
      echo "// stub" > $crate/src/lib.rs; \
    done
RUN cargo metadata --format-version 1 > /dev/null

# Now copy the real source and build only the server binary.
COPY ftdata-cli/ ftdata-cli/
COPY ftdata-core/ ftdata-core/
COPY ftdata-http/ ftdata-http/
COPY ftdata-sources/ ftdata-sources/
COPY ftdata-storage/ ftdata-storage/
COPY ftdata-analysis/ ftdata-analysis/
COPY ftdata-paid-pricing/ ftdata-paid-pricing/
COPY ftdata-paid-facilitator/ ftdata-paid-facilitator/
COPY ftdata-paid-api/ ftdata-paid-api/

RUN cargo build --release --bin ftdata-paid-server

# ---- runtime ----
FROM debian:bookworm-slim AS runtime

# ca-certificates for any HTTPS calls (e.g. real facilitator client).
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Run as non-root.
RUN useradd --create-home --shell /bin/bash ftdata

COPY --from=builder /build/target/release/ftdata-paid-server /usr/local/bin/ftdata-paid-server

USER ftdata
EXPOSE 8080

ENV FTDATA_BIND=0.0.0.0:8080 \
    RUST_LOG=info

ENTRYPOINT ["/usr/local/bin/ftdata-paid-server"]
