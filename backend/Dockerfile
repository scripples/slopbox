# Stage 1: Build
FROM rust:1.93.1-bookworm AS builder

WORKDIR /app

# Copy workspace manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./
COPY crates/cb-api/Cargo.toml crates/cb-api/Cargo.toml
COPY crates/cb-db/Cargo.toml crates/cb-db/Cargo.toml
COPY crates/cb-infra/Cargo.toml crates/cb-infra/Cargo.toml
COPY crates/fly-api/Cargo.toml crates/fly-api/Cargo.toml
COPY crates/sprites-api/Cargo.toml crates/sprites-api/Cargo.toml

# Create dummy source files for dependency caching
RUN mkdir -p crates/cb-api/src crates/cb-db/src crates/cb-infra/src crates/fly-api/src crates/sprites-api/src && \
    echo "fn main() {}" > crates/cb-api/src/main.rs && \
    echo "" > crates/cb-db/src/lib.rs && \
    echo "" > crates/cb-infra/src/lib.rs && \
    echo "" > crates/fly-api/src/lib.rs && \
    echo "" > crates/sprites-api/src/lib.rs

# Build dependencies only (cached layer)
RUN cargo build --release --bin cb-api 2>/dev/null || true

# Copy real source code
COPY crates/ crates/

# Touch source files to invalidate previous build
RUN touch crates/cb-api/src/main.rs crates/cb-db/src/lib.rs crates/cb-infra/src/lib.rs crates/fly-api/src/lib.rs crates/sprites-api/src/lib.rs

# Build the real binary
RUN cargo build --release --bin cb-api

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/cb-api /usr/local/bin/cb-api

# Copy migrations (embedded in binary via sqlx, but keep for reference)
COPY --from=builder /app/crates/cb-db/migrations /app/migrations

EXPOSE 8080
EXPOSE 3128

CMD ["cb-api"]
