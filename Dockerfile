FROM rust:1.95-bookworm AS builder

# Install Vite+
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && curl -fsSL https://vite.plus | bash \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy package files first for better layer caching
COPY web/package.json web/pnpm-lock.yaml ./web/

# Install web dependencies
RUN cd web && vp install --frozen-lockfile

# Copy the rest of the project
COPY . .

# Build web assets with Vite+
RUN cd web && vp build

# Build the Rust binary with embedded UI
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
