# moca/pty-proxy — Servidor de la terminal remota

Servidor websocket → PTY que corre en la XPS. Reemplaza al `warpdotdev/ssh-proxy-server` que espera `app/src/terminal/remote_tty/event_loop.rs` (abre `/create?num_rows=N&num_cols=M` y recibe `WindowSizeChange` para redimensionar).

Proyecto Cargo aparte, fuera del workspace de Warp. Todavía vacío: paso 3 del plan de [`docs/moca/ios.md`](../../docs/moca/ios.md).
