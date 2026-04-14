# Troubleshooting

## Health Check Shows No Output

- `curl http://127.0.0.1:18080/healthz` returns body `ok`.
- Use `curl -i` to also view status code.

## Access Code Could Not Be Saved

Symptom in logs:

`web auth enabled (generated access code but could not save).`

Fix with Compose setup in this repo:

```bash
docker compose down
docker compose up -d --build
docker compose logs -f imsa-web
```

The one-shot `imsa-web-data-perms` service repairs volume ownership for nonroot runtime.

## Build Error: Missing Cargo.lock in Docker Context

Symptom: Docker build fails at `COPY Cargo.toml Cargo.lock ./`.

Checks:

```bash
pwd
ls
ls Cargo.toml Cargo.lock Dockerfile compose.yml
```

Ensure you run `docker compose` from repository root and `Cargo.lock` exists there.

## Port Already In Use

```bash
docker ps --format 'table {{.Names}}\t{{.Ports}}'
ss -ltnp
```

If `18080` is occupied, pick another host port and update proxy target accordingly.
