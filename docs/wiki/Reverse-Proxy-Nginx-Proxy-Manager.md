# Reverse Proxy Nginx Proxy Manager

## Proxy Host Settings

- Domain Names: your app domain.
- Scheme: `http`.
- Forward Hostname/IP: `127.0.0.1`.
- Forward Port: `18080`.
- Websockets Support: enabled.

## SSL Settings

- Request/attach certificate.
- Enable Force SSL.
- Enable HTTP/2.

## Advanced Config

```nginx
proxy_buffering off;
proxy_read_timeout 3600;
```

## Validation

- `curl -i http://127.0.0.1:18080/healthz` should return `200` + `ok`.
- Opening your domain should show the login page.
