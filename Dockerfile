FROM rust:1.95-bookworm AS builder

ARG TRUNK_VERSION=0.21.14

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown
RUN curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
    | tar -xz -C /usr/local/bin trunk

WORKDIR /app

COPY . .

RUN trunk build --release --config web/Trunk.toml
RUN cargo build --release --bin web_server --features embed-ui

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=builder /app/target/release/web_server /web_server

ENV WEBUI_AUTO_FUNNEL=0
ENV WEBUI_COOKIE_SECURE=1
ENV BIND_ADDR=0.0.0.0
ENV PORT=8080
ENV XDG_DATA_HOME=/data

EXPOSE 8080
VOLUME ["/data"]

ENTRYPOINT ["/web_server"]
