# Deployment Docker Compose

## Recommended Command Flow

```bash
git pull
docker compose build
docker compose up -d
docker compose ps
curl -fsS http://127.0.0.1:18080/healthz
```

## Important Runtime Defaults

- Host mapping: `18080:8080`.
- Tailscale funnel disabled: `WEBUI_AUTO_FUNNEL=0`.
- Cookie secure default: `WEBUI_COOKIE_SECURE=1`.
- Persistent runtime dir: `XDG_DATA_HOME=/data`.
- Volume: `imsa-web-data:/data`.

## Persistence Behavior

- Data survives rebuilds and normal restarts.
- `docker compose down` keeps volumes.
- Data is removed only with `docker compose down -v` or explicit `docker volume rm`.

## Common Ops

```bash
docker compose logs -f imsa-web
docker compose restart imsa-web
docker compose down
docker compose up -d
```

## Rotate Web Access Code

```bash
docker compose stop imsa-web
docker compose run --rm -e WEBUI_ROTATE_PASSWORD=1 imsa-web
docker compose up -d imsa-web
```
