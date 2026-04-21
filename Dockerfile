FROM debian:bookworm-slim AS binary-fetch

ARG RELEASE_TAG=latest

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tar \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /tmp

RUN if [ "$RELEASE_TAG" = "latest" ]; then \
      curl -fL "https://github.com/wandabastyle/imsa_tui/releases/latest/download/imsa-web-linux-amd64.tar.gz" -o imsa-web-linux-amd64.tar.gz; \
    else \
      curl -fL "https://github.com/wandabastyle/imsa_tui/releases/download/${RELEASE_TAG}/imsa-web-linux-amd64.tar.gz" -o imsa-web-linux-amd64.tar.gz; \
    fi \
    && tar -xzf imsa-web-linux-amd64.tar.gz \
    && test -f web_server

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=binary-fetch /tmp/web_server /web_server

ENV WEBUI_AUTO_FUNNEL=0
ENV WEBUI_COOKIE_SECURE=1
ENV BIND_ADDR=0.0.0.0
ENV PORT=8080
ENV XDG_DATA_HOME=/data

EXPOSE 8080
VOLUME ["/data"]

ENTRYPOINT ["/web_server"]
