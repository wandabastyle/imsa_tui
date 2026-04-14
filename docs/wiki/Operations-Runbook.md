# Operations Runbook

## Start / Stop / Restart (Compose)

```bash
docker compose up -d
docker compose down
docker compose restart imsa-web
```

## Update

```bash
git pull
docker compose up -d --build
docker compose ps
curl -fsS http://127.0.0.1:18080/healthz
```

## Logs

```bash
docker compose logs -f imsa-web
```

## Backup Named Volume

```bash
docker run --rm -v imsa_tui_imsa-web-data:/data -v "$PWD":/backup busybox tar -czf /backup/imsa-web-data.tgz -C /data .
```

## Restore Named Volume

```bash
docker compose down
docker run --rm -v imsa_tui_imsa-web-data:/data busybox sh -c "rm -rf /data/*"
docker run --rm -v imsa_tui_imsa-web-data:/data -v "$PWD":/backup busybox tar -xzf /backup/imsa-web-data.tgz -C /data
docker compose up -d
```

## Port Inventory

```bash
docker ps --format 'table {{.Names}}\t{{.Ports}}'
ss -ltnp
```
