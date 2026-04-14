# Web Auth and Sessions

## Access Code

- First start: server generates a strong shared access code.
- If persistence works, code is saved and reused on next starts.
- Rotate manually with `WEBUI_ROTATE_PASSWORD=1` for one startup.

## Cookies

- Session cookie name: `imsa_session`.
- Browser-session scoped: closing browser requires login again.
- Cookie flags: `HttpOnly`, `SameSite=Lax`, optional `Secure`.

## Persistence Paths

With container deployment (`XDG_DATA_HOME=/data`):

- `/data/web_auth.toml`
- `/data/web_server.log`
- `/data/web_server.pid`
- `/data/web_server.info.toml`
- `/data/profiles/<profile_id>.toml`

## Security Notes

- Behind public TLS reverse proxy, keep `WEBUI_COOKIE_SECURE=1`.
- `/healthz` and `/readyz` are intentionally public.
