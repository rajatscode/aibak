FROM rust:1.88-slim AS builder

WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY crates/engine/Cargo.toml crates/engine/Cargo.toml
COPY crates/server/Cargo.toml crates/server/Cargo.toml
COPY crates/cli/Cargo.toml crates/cli/Cargo.toml
RUN mkdir -p crates/engine/src crates/server/src crates/cli/src crates/static
RUN echo "fn main() {}" > crates/server/src/main.rs && \
    echo "fn main() {}" > crates/cli/src/main.rs && \
    echo "" > crates/engine/src/lib.rs && \
    echo "" > crates/static/index.html
RUN cargo build --release --bin strat-server 2>/dev/null || true

# Build for real
COPY . .
RUN cargo build --release --bin strat-server

# Runtime image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/strat-server /usr/local/bin/
COPY --from=builder /app/maps /app/maps
COPY --from=builder /app/boards /app/boards

WORKDIR /app
ENV RUST_LOG=info
EXPOSE 3000

CMD ["strat-server"]
