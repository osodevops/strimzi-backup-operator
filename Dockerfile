# Build stage
FROM rust:1.88-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock ./

# Create dummy main.rs to build dependencies
RUN mkdir -p src/bin && \
    echo "fn main() {}" > src/main.rs && \
    echo "fn main() {}" > src/bin/crdgen.rs

# Build dependencies only (will be cached)
RUN cargo build --release && rm -rf src

# Copy actual source code
COPY src ./src

# Touch main.rs to invalidate the cache for final build
RUN touch src/main.rs

# Build the actual binary
RUN cargo build --release --bin strimzi-backup-operator

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    tini \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -u 1000 -g root operator

# Copy the binary
COPY --from=builder /workspace/target/release/strimzi-backup-operator /usr/local/bin/strimzi-backup-operator

# Set ownership and permissions
RUN chown operator:root /usr/local/bin/strimzi-backup-operator && \
    chmod 755 /usr/local/bin/strimzi-backup-operator

USER 1000

# Expose metrics port
EXPOSE 9090

ENTRYPOINT ["tini", "--"]
CMD ["strimzi-backup-operator"]
