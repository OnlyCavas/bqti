FROM rust:1.88-slim-bookworm AS deps
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
  pkg-config \
  libssl-dev \
  && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY bqti-ipc/Cargo.toml ./bqti-ipc/Cargo.toml

RUN mkdir src && echo "fn main() {}" > src/main.rs \
  && mkdir -p bqti-ipc/src && echo "" > bqti-ipc/src/lib.rs \
  && cargo build --release --locked \
  && rm -rf src bqti-ipc/src \
  target/release/.fingerprint/bqti* \
  target/release/.fingerprint/bqti-ipc*

FROM deps AS builder
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
  ca-certificates \
  libssl3 \
  && rm -rf /var/lib/apt/lists/*

RUN useradd -ms /bin/bash appuser

RUN mkdir -p /run/bqti && chown appuser:appuser /run/bqti

COPY --from=builder /app/target/release/bqti /usr/local/bin/bqti
RUN chmod 755 /usr/local/bin/bqti

USER appuser
WORKDIR /home/appuser

ENV BQTI_HOST=0.0.0.0
ENV BQTI_PORT=6881
ENV BQTI_SOCKET=/run/bqti/default.sock

EXPOSE ${BQTI_PORT}

CMD exec bqti serve "${BQTI_HOST}:${BQTI_PORT}"
