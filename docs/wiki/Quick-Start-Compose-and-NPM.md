# Quick Start Compose and NPM

## Prerequisites

- Docker engine with Compose plugin.
- Nginx Proxy Manager already running.
- DNS record for your subdomain pointing to this host.

## Deploy

```bash
git clone git@github.com:wandabastyle/imsa_tui.git
cd imsa_tui
docker compose up -d --build
docker compose ps
curl -i http://127.0.0.1:18080/healthz
```

Expected health response body is `ok`.

## Configure Nginx Proxy Manager

1. Add Proxy Host.
2. Domain Names: your subdomain.
3. Scheme: `http`.
4. Forward Hostname/IP: `127.0.0.1`.
5. Forward Port: `18080`.
6. Enable Websockets Support.
7. SSL tab: request/attach certificate and Force SSL.
8. Advanced block:

```nginx
proxy_buffering off;
proxy_read_timeout 3600;
```

## Verify End-to-End

```bash
docker compose logs -f imsa-web
```

Open your subdomain and confirm login page is reachable.
