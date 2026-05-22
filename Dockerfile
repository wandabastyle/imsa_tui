# syntax=docker/dockerfile:1.7

FROM rust:1.95-bookworm AS build-base

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && curl -fsSL https://vite.plus | bash \
    && cargo install cargo-chef --locked \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

ENV VP_HOME="/root/.vite-plus"
ENV PATH="${VP_HOME}/bin:${PATH}"

FROM build-base AS web-build

COPY web/package.json web/pnpm-lock.yaml ./web/
RUN --mount=type=cache,id=pnpm-store,target=/root/.local/share/pnpm/store \
    command -v vp && cd web && vp install --frozen-lockfile

COPY web ./web
RUN --mount=type=cache,id=pnpm-store,target=/root/.local/share/pnpm/store \
    command -v vp && cd web && vp build

FROM build-base AS rust-planner

COPY Cargo.toml Cargo.lock ./
COPY crates/web-shared/Cargo.toml ./crates/web-shared/Cargo.toml
COPY src ./src
COPY crates/web-shared/src ./crates/web-shared/src
RUN cargo chef prepare --recipe-path recipe.json

FROM build-base AS rust-cook

COPY --from=rust-planner /app/recipe.json ./recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin web_server --features embed-ui

FROM rust-cook AS builder

COPY Cargo.toml Cargo.lock ./
COPY crates/web-shared/Cargo.toml ./crates/web-shared/Cargo.toml
COPY src ./src
COPY crates/web-shared/src ./crates/web-shared/src
COPY --from=web-build /app/web/build ./web/build
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
