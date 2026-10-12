# Reporte iOS — inventario antes de cambiar código

Fecha: 11 oct 2026 · Rama `moca-ios` · Responde el §9 de [`ios.md`](ios.md).
Inventario (secciones 1–2) hecho por GLM-5.3 y verificado por Claude en los puntos marcados ✔; plan (sección 4) de Claude.

## 0. Entorno de desarrollo

| Qué | Estado |
|---|---|
| iPad Pro 13" M4 (`iPad16,5`) | Visible para Xcode (`xcrun devicectl`), **sin emparejar** para desarrollo: falta confiar en la Mac y activar el Modo desarrollador en el iPad |
| Certificado | `Apple Development: Jose David Moreira (99GDMRU8K7)`, equipo de pago |
| Xcode | 26.3, SDK iOS 26.2 |
| Rust para iOS | **Falta**: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim` (toolchain 1.92.0 del repo) |

## 1. `local_tty` (la PTY local, que no existe en iOS)

**a) Quién la activa** ✔ — no está en las features `default` ni `gui`. La inyecta `app/build.rs:232-235` para todo target que no sea wasm (`local_fs` y `local_tty`). Un build para iOS la recibiría sin pedirla: **hay que excluir iOS en `build.rs`**. En `warp_terminal`, `local_tty = []` es solo un marcador (`crates/warp_terminal/Cargo.toml:130`) y el módulo se excluye por `cfg(not(target_family = "wasm"))` (`crates/warp_terminal/src/lib.rs:5-6`).

**b) Código bajo `cfg(feature = "local_tty")`** — 23 archivos con la forma directa; 183 apariciones en 30 archivos contando `not(...)`/`all(...)`. Los más cargados: `terminal/available_shells.rs` (23), `terminal/view/docker_sandbox/mod.rs` (19), `settings_view/features_page.rs` (15), `lib.rs` (13), `terminal/local_shell/mod.rs` (12), `terminal/view.rs` (12), `workspace/view.rs` (11), `terminal/model/session/command_executor.rs` (10), `pane_group/mod.rs` (10). Ya están aislados; solo hay que confirmar que compilan apagados.

**c) Ya existe un build sin PTY local: el web** — `script/wasm/bundle` compila `warp-oss` para `wasm32-unknown-unknown` con `release_bundle,gui`. `remote_tty` se compila siempre (`terminal/mod.rs:64`); su `TerminalManager` se usa cuando la feature `remote_tty` está activa (`pane_group/mod.rs:135-136`, `:6115`). `local_tty` y `remote_tty` son excluyentes en compilación (`pane_group/mod.rs:129`, `terminal/local_tty/mod.rs:10`).

**d) Usos de `local_tty` sin cfg (romperían)** ✔ — `app/src/tui_export.rs:227-230` re-exporta `local_tty::TerminalManager` sin condición. El resto (`settings/import/iterm_parser.rs:23`, `integration_testing/terminal/util.rs:14`, `bin/integration.rs:61`, `settings_view/features/startup_shell.rs:12`) son binarios o módulos que no entran al build de iOS, o hay que verificarlos al compilar.

## 2. macOS/AppKit en la UI

**a) Dependencias de macOS** — `crates/warpui/Cargo.toml:75-143`: cocoa, core-foundation, core-graphics, core-text, font-kit, objc2, **objc2-app-kit** (no existe en iOS), objc2-metal, objc2-quartz-core, dispatch. `crates/warpui_core` **no tiene dependencias de macOS**.

**b) Elección de plataforma** ✔ — `crates/warpui/src/platform/mod.rs:13-27` (`cfg_if`): wasm → linux/freebsd → macos → windows → si no, `warpui_core::platform::test`. **iOS hoy caería en la plataforma de pruebas**: hay que agregar su rama. Traits que implementa una plataforma (`crates/warpui_core/src/platform/mod.rs`): `Delegate` (:205), `DispatchDelegate` (:313), `LoadedSystemFonts` (:326), `TextLayoutSystem` (:332), `FontDB` (:366), `Window` (:464), `WindowContext` (:485), `WindowManager` (:572).

**c) Render** — macOS usa un renderer **Metal propio** (`platform/mac/rendering/metal/`); wgpu en macOS es opcional (`experimental-wgpu-renderer`). Todo lo que no es macOS usa wgpu, y wgpu corre sobre Metal en iOS.

**d) Texto** — macOS: core-text + core-graphics + font-kit (`platform/mac/fonts.rs`). Resto: cosmic-text + fontdb + swash (`windowing/winit/fonts/`). iOS puede usar la ruta de winit (la más probada).

**e) winit** ✔ — `crates/warpui/build.rs:17`: `winit: { not(macos) }` y `wgpu: { any(winit, ...) }`. **iOS (`target_os = "ios"`) entra automáticamente por winit + wgpu**, la misma ruta de Linux/Windows/web, no por AppKit. El repo usa un fork de winit (`warpdotdev/winit`, rev `14db95a`), que conserva el backend de iOS (`src/platform_impl/ios`) ✔.

## 3. Conteo de `target_os = "macos"` ✔

| Crate | Apariciones |
|---|---|
| `crates/warpui/src` | 58 |
| `crates/warpui_core/src` | 12 |

## 4. Plan para los pasos 2 y 3 (terminal remota en la XPS)

**Protocolo que ya habla la app** ✔ (`app/src/terminal/remote_tty/event_loop.rs`):
- Abre `ws://<host>/create?num_rows=N&num_cols=M` (hoy fijo a `127.0.0.1:3030`, línea 20).
- Mensajes **binarios** en ambos sentidos = bytes de la terminal.
- Mensaje de **texto** del cliente = `{"width","height","width_px","height_px"}` para redimensionar.
- Al conectar, la app manda sola `WARP_HONOR_PS1=…;` y el script de arranque de zsh de Warp. El servidor no tiene que saber nada de Warp.

**Paso 2 — endpoint configurable** (cambio pequeño en código compartido; entra a `moca` por separado):
1. `remote_tty/event_loop.rs`: reemplazar la constante por una función que lea el endpoint de una variable de entorno (`MOCA_REMOTE_TTY_URL`) y, si no está, use el valor actual. En iOS no hay variables de entorno: después se lee de un ajuste de la app.
2. Agregar un token a la URL (`&token=…`) para el paso 3.
3. Probar que el build de escritorio normal no cambia (no usa `remote_tty`).

**Paso 3 — `moca/pty-proxy` en la XPS:**
1. Proyecto Cargo aparte (fuera del workspace): `tokio` + `tokio-tungstenite` (websocket) + `portable-pty` (PTY).
2. Por cada conexión `/create`: valida el token, abre `zsh` en una PTY del tamaño pedido y copia bytes en ambos sentidos; los mensajes de texto redimensionan la PTY.
3. Escucha **solo en la IP de Tailscale** de la XPS (nunca en `0.0.0.0`), con token. Da acceso a un shell: no se expone fuera del tailnet.
4. Corre como servicio de usuario de systemd en la XPS.
5. Prueba desde la Mac antes del iPad: `cargo run --bin warp-oss --features gui,remote_tty` con `MOCA_REMOTE_TTY_URL` apuntando a la XPS.

**Pendiente para después (las dos terminales a la vez):** como `local_tty`/`remote_tty` son excluyentes al compilar, para tener en el iPad pestañas remotas e integradas hay que elegir el tipo de terminal al abrir cada pestaña (`pane_group/mod.rs:6115`). En escritorio no cambia nada.

## 5. Conclusiones

1. **Compila hoy sin cambios para iOS:** `warpui_core` completo, la ruta winit + wgpu + cosmic-text, `remote_tty`, `writeable_pty` y `moca_processes` (ya tiene rama sin tabla de procesos).
2. **Hay que aislar:** `app/build.rs:232-235` (no inyectar `local_tty`/`local_fs` en iOS), `tui_export.rs:227` y la rama de iOS en `platform/mod.rs:13-27`.
3. **Hay que escribir nuevo:** la rama iOS de la plataforma (reutilizando la de winit), teclado en pantalla (ideas de `platform/wasm/soft_keyboard.rs`), la app Xcode (`moca/ios/`) y el proxy (`moca/pty-proxy/`).
4. **Riesgo principal (bajó):** el fork `warpdotdev/winit` (rev `14db95a`) **sí trae el backend de iOS** (`src/platform_impl/ios`) ✔. Falta comprobar al compilar que funcione con los cambios de Warp.
5. **Orden recomendado:** instalar targets de Rust y emparejar el iPad → paso 2 → paso 3 probado desde la Mac → primer `cargo check --target aarch64-apple-ios` para ver la lista real de errores.
