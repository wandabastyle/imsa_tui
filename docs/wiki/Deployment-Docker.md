# Deployment Docker

Use this when you do not want Compose.

## Build Image

```bash
docker build -t imsa-web:local .
docker volume create imsa-web-data
```

## Run Container

```bash
docker run -d \
  --name imsa-web \
  --restart unless-stopped \
  -p 18080:8080 \
  -e WEBUI_AUTO_FUNNEL=0 \
  -e WEBUI_COOKIE_SECURE=1 \
  -e BIND_ADDR=0.0.0.0 \
  -e PORT=8080 \
  -e XDG_DATA_HOME=/data \
  -v imsa-web-data:/data \
  imsa-web:local
```

## Verify

```bash
docker ps --format 'table {{.Names}}\t{{.Ports}}'
curl -i http://127.0.0.1:18080/healthz
docker logs imsa-web
```

## Update

```bash
docker stop imsa-web
docker rm imsa-web
docker build -t imsa-web:local .
docker run -d --name imsa-web --restart unless-stopped -p 18080:8080 -e WEBUI_AUTO_FUNNEL=0 -e WEBUI_COOKIE_SECURE=1 -e BIND_ADDR=0.0.0.0 -e PORT=8080 -e XDG_DATA_HOME=/data -v imsa-web-data:/data imsa-web:local
```
