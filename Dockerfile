FROM rust:1.85-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN cargo build --release --bin strimzi-backup-operator

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    tini \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r operator && useradd -r -g operator -u 1001 operator

COPY --from=builder /app/target/release/strimzi-backup-operator /usr/local/bin/strimzi-backup-operator

USER 1001

ENTRYPOINT ["tini", "--"]
CMD ["strimzi-backup-operator"]
