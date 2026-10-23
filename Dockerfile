# syntax=docker/dockerfile:1

FROM rust:1.99-slim AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

FROM debian:trixie-slim AS runtime

RUN useradd --system --uid 10001 --create-home --home-dir /var/lib/asset-server asset-server \
    && mkdir -p /var/lib/asset-server/data /etc/asset-server \
    && chown -R asset-server:asset-server /var/lib/asset-server

COPY --from=builder /app/target/release/asset-server /usr/local/bin/asset-server

USER asset-server
WORKDIR /var/lib/asset-server
EXPOSE 3000

CMD ["asset-server", "--config", "/etc/asset-server/config.toml"]
