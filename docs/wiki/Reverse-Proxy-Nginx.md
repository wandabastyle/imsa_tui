# Reverse Proxy Nginx

## Minimal Server Block

```nginx
server {
    listen 80;
    server_name your.domain.example;

    location / {
        proxy_pass http://127.0.0.1:18080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_buffering off;
        proxy_read_timeout 3600;
    }
}
```

Terminate TLS at nginx in your SSL-enabled server block, or upstream if your environment already handles TLS.

## Reload and Verify

```bash
nginx -t
sudo systemctl reload nginx
curl -i http://127.0.0.1:18080/healthz
```
