# moca/pty-proxy — Servidor de la terminal remota

Servidor websocket → PTY que corre en la XPS. Reemplaza al `warpdotdev/ssh-proxy-server` que espera `app/src/terminal/remote_tty/event_loop.rs` (abre `/create?num_rows=N&num_cols=M` y recibe `WindowSizeChange` para redimensionar).

Proyecto Cargo aparte, fuera del workspace de Warp. Todavía vacío: paso 3 del plan de [`docs/moca/ios.md`](../../docs/moca/ios.md).

## Uso

Proyecto Cargo independiente (no forma parte del workspace de Warp). Compilar dentro de `moca/pty-proxy`:

```
cd moca/pty-proxy
cargo build --release
```

Generar un token de acceso:

```
openssl rand -hex 32
```

Correr (lanza `/bin/zsh` como login shell en el HOME del usuario; `MOCA_PTY_SHELL` permite cambiarlo):

```
MOCA_PTY_LISTEN=<ip-de-tailscale>:3030 MOCA_PTY_TOKEN=<token> ./target/release/moca-pty-proxy
```

Variables de entorno:

| Variable | Obligatoria | Descripción |
|---|---|---|
| `MOCA_PTY_LISTEN` | sí | Dirección donde escuchar, p. ej. `<ip-de-tailscale>:3030`. No hay valor por defecto a propósito: el servidor entrega un shell. |
| `MOCA_PTY_TOKEN` | sí | Token que el cliente debe pasar en la URL (`?token=...`). Se compara en tiempo constante. |
| `MOCA_PTY_SHELL` | no | Shell a lanzar (por defecto `/bin/zsh`; si no existe, `/bin/bash`). |

Protocolo (lo habla el cliente de Warp, ver `app/src/terminal/remote_tty/event_loop.rs`):

1. El cliente abre `ws://HOST:PUERTO/create?num_rows=N&num_cols=M&token=T`.
2. Los mensajes binarios del cliente se escriben en la PTY (teclas y el script de arranque de zsh).
3. Los mensajes de texto del cliente son JSON `{"width", "height", "width_px", "height_px"}`; `width` son columnas y `height` filas, y redimensionan la PTY.
4. Todo lo que emite la PTY se manda al cliente como mensajes binarios.
5. Cuando el shell termina se cierra el websocket; cuando el cliente se desconecta se mata el shell.

### Servicio de systemd de usuario (en la XPS)

`~/.config/systemd/user/moca-pty-proxy.service` (ajustar la ruta del binario al checkout real):

```ini
[Unit]
Description=Moca PTY proxy (terminal remota para Warp)
After=network-online.target
Wants=network-online.target

[Service]
EnvironmentFile=%h/.config/moca/pty-proxy.env
ExecStart=%h/dev/warp/moca/pty-proxy/target/release/moca-pty-proxy
Restart=on-failure

[Install]
WantedBy=default.target
```

`~/.config/moca/pty-proxy.env` con la IP de Tailscale de la XPS y el token generado:

```
MOCA_PTY_LISTEN=<ip-de-tailscale>:3030
MOCA_PTY_TOKEN=<token-generado>
```

Activar y seguir el log:

```
systemctl --user daemon-reload
systemctl --user enable --now moca-pty-proxy.service
journalctl --user -u moca-pty-proxy -f
```

### Advertencia

Escuchar **solo** en la IP de Tailscale de la máquina, nunca en `0.0.0.0`: el servidor entrega un shell del usuario que lo corre y el token de la URL es la única autenticación.
