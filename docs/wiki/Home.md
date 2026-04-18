# imsa_tui Wiki

Production-facing docs for deploying and operating the web server.

## Quick Start (5 minutes)

Recommended path: Docker Compose + Nginx Proxy Manager.

1. Clone repo and enter it.
2. Build and start:
   ```bash
   docker compose up -d --build
   ```
3. Verify local service:
   ```bash
   curl -i http://127.0.0.1:18080/healthz
   ```
   Expected body: `ok`.
4. In Nginx Proxy Manager, point the host to `http://127.0.0.1:18080`.
5. Open your subdomain and log in with the printed shared access code.

## Day-2 Ops Checklist

| Task | Commands | Success check |
| --- | --- | --- |
| Update | `git pull` then `docker compose up -d --build` | `curl -fsS http://127.0.0.1:18080/healthz` returns `ok` |
| Logs | `docker compose logs -f imsa-web` | Startup prints local URL and auth status |
| Backup volume | `docker run --rm -v imsa_tui_imsa-web-data:/data -v "$PWD":/backup busybox tar -czf /backup/imsa-web-data.tgz -C /data .` | Backup archive exists |
| Restore volume | `docker compose down` then untar into volume mountpoint or with helper container | Restart loads saved access code |
| Rollback | Checkout prior commit/tag, then `docker compose up -d --build` | App and login work as expected |

## Navigation

- [[Quick-Start-Compose-and-NPM]]
- [[Deployment-Docker-Compose]]
- [[Deployment-Docker]]
- [[Reverse-Proxy-Nginx-Proxy-Manager]]
- [[Reverse-Proxy-Nginx]]
- [[Web-Auth-and-Sessions]]
- [[NLS-WebSocket-Field-Map]]
- [[Operations-Runbook]]
- [[Troubleshooting]]
- [[Development-and-CI]]
