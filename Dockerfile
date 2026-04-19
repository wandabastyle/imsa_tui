FROM node:22-alpine AS web-build
WORKDIR /app/web

COPY web/package.json web/pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile

COPY web/ ./
RUN pnpm run build

FROM rust:1.95-bookworm AS rust-build
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY tests/ ./tests/
COPY --from=web-build /app/web/build ./web/build

RUN cargo build --release --locked --bin web_server

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=rust-build /app/target/release/web_server /web_server

ENV WEBUI_AUTO_FUNNEL=0
ENV WEBUI_COOKIE_SECURE=1
ENV BIND_ADDR=0.0.0.0
ENV PORT=8080
ENV XDG_DATA_HOME=/data

EXPOSE 8080
VOLUME ["/data"]

ENTRYPOINT ["/web_server"]
