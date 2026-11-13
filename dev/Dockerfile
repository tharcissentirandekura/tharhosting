#Building stage
FROM rust:1-slim-bookworm AS builder   


WORKDIR /app

COPY Cargo.toml Cargo.lock ./

RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src

RUN touch src/main.rs src/lib.rs && cargo build --release

#runtime
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends git docker.io ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/tharhosting /usr/local/bin/tharhosting

ENV THARHOSTING_HOST=0.0.0.0 \
    THARHOSTING_PORT=8900 \
    THARHOSTING_DATA=/etc/data

EXPOSE 8900
CMD ["tharhosting"]