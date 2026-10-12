# Moca Warp → iOS (iPad) — Paquete de contexto para Claude Code

> Vive en `docs/moca/ios.md` de la rama `moca-ios`. Para arrancar una sesión:
> `claude "Lee docs/moca/ios.md y AGENTS.md y repórtame antes de cambiar nada."`

Fecha: 11 oct 2026 · Repo revisado en el commit `ab4dd67` (rama por defecto).

---

## 1. Objetivo

Llevar **Moca Warp** (fork propio de Warp, Rust, open source) a una **app nativa de iPadOS** para un iPad Pro 13" M4.

## 2. Decisiones ya tomadas

| Tema | Decisión |
|---|---|
| Tipo de app | **Nativa** de iOS. Se descartó la versión web en Safari (se percibe inestable). |
| Distribución | **Sin App Store.** Firmada con la cuenta de desarrollador de pago e instalada desde Xcode (cable o Wi‑Fi). TestFlight personal como alternativa. |
| Máquina de build | Mac (Apple Silicon). El iPad no compila ni firma. |
| Hardware destino | iPad Pro 13" M4. El hardware no es el cuello de botella. |
| Herramientas | No hace falta el MCP de GitHub (`api.githubcopilot.com/mcp/`) para leer el repo; solo serviría para issues/PRs. |

## 3. Dos modos de terminal en la app

La app va a tener **los dos**, como pestañas distintas. Se construye primero A (reutiliza más código) y después B.

**A. Terminal remota**
- El iPad solo dibuja y manda teclas. El shell real (zsh, git, etc.) corre en la **XPS** (robco9310, mini‑servidor Linux del tailnet, encendido con la tapa cerrada; la dirección no va en este repo público). No se usa colmena (ThinkPad) para esto.
- Conexión por el tailnet de Tailscale.
- Ventaja: reutiliza código que ya existe en el fork (ver §4) y da acceso a repos y herramientas reales.

**B. Terminal integrada (shell local embebido en la app)**
- iOS bloquea `fork`/`exec` incluso en apps firmadas por uno mismo: no se puede lanzar el `zsh` del sistema.
- Solución: empaquetar dentro de la app un shell/herramientas compiladas para iOS arm64 (estilo BusyBox o un shell en Rust), corriendo **nativo, sin emulación**. Emular un Linux completo (estilo iSH/Alpine) se descartó por lento.
- Archivos: la app tiene su propio sandbox completo (crear, borrar, clonar repos). Carpetas fuera del sandbox (iCloud, externas) solo vía la app **Archivos** con permiso explícito del usuario (security-scoped bookmarks).

## 4. Hallazgos en el código (lo importante)

### 4.1 La PTY ya está separada por features de Cargo
- `app/Cargo.toml`
  - `local_tty = ["warp_terminal/local_tty"]` (línea ~787)
  - `remote_tty = ["warp_terminal/remote_tty"]` (línea ~811)
- PTY local: `crates/warp_terminal/src/local_tty/` (`unix.rs`, `spawner.rs`, `windows/`, `server/`). Código adaptado de alacritty_terminal. **Esto no sirve en iOS.**
- PTY remota: `app/src/terminal/remote_tty/` (`terminal_manager.rs`, `event_loop.rs`). Su `TerminalManager` "alimenta bytes desde/hacia una PTY remota" por **websocket**. **Esta es la pieza a reutilizar.**
- Controlador común: `app/src/terminal/writeable_pty/` (`pty_controller.rs`, `terminal_manager_util.rs`).

### 4.2 El endpoint remoto está fijo
- `app/src/terminal/remote_tty/event_loop.rs`
  - `const CREATE_SESSION_ENDPOINT: &str = "ws://127.0.0.1:3030/create";`
  - Abre `…/create?num_rows=N&num_cols=M` y manda `WindowSizeChange { width, height }` como texto para redimensionar.
- Espera el **`warpdotdev/ssh-proxy-server`**, que **no está en este repo**. Hay que conseguirlo o reescribirlo: un servidor websocket → PTY en la XPS (va en `moca/pty-proxy/`).
- Tarea: hacer el endpoint configurable (apuntar a la XPS en el tailnet) y pensar autenticación.

### 4.3 Render y plataformas
- UI propia: `crates/warpui` + `crates/warpui_core`. Render por **wgpu** (`crates/warpui/src/rendering/wgpu`), con atlas de glifos propio.
- Plataformas existentes en `crates/warpui/src/platform/`: `mac`, `linux`, `windows`, `wasm`, `headless`. **No hay `ios`.**
- `platform/mac` usa AppKit/cocoa/objc2-app-kit → no compila para iOS. Hay que crear `platform/ios` (UIKit), probablemente sobre `winit` (el repo usa un fork: `warpdotdev/winit`) + wgpu/Metal.
- Pistas reutilizables del build web (`platform/wasm`):
  - `mobile_detection/` (ya detecta iPad aunque Safari diga "Macintosh").
  - `soft_keyboard.rs` + `hidden_input.rs` (manejo de teclado en pantalla).
- `cfg` en el código: ~2014 `target_family = "wasm"`, ~372 `macos`, ~247 `linux`, ~123 `windows`, **0 `ios`**. Los bloques `wasm` muestran cómo el código ya funciona sin PTY local ni tabla de procesos.

### 4.4 Cambios propios de Moca ya presentes
- `app/src/terminal/moca_processes.rs` (+ tests): monitor de procesos/CPU, con rama `wasm` vacía. En iOS habrá que darle el mismo trato.
- Binarios: `warp-oss` (`src/bin/oss.rs`, default-run) y `warp` (`src/bin/local.rs`).

### 4.5 Otros
- `AGENTS.md` en la raíz: comandos de build (`cargo run`, `./script/run`, `./script/run-tui`, `cargo bundle --bin warp`).
- `crates/warp_tui`: front‑end TUI headless (útil para probar la capa remota sin UI).
- CI: el workflow **"Populate Build Cache"** de tu fork falla al arrancar desde el 6 oct (commits `983e9b2`, `ab4dd67`). No bloquea esto, pero conviene revisarlo.

## 5. Plan propuesto (opción A primero)

1. **Reporte inicial** (sin cambios): confirmar la frontera `local_tty`/`remote_tty`, qué depende de `local_tty` y qué compila con solo `remote_tty`.
2. **Endpoint configurable** en `remote_tty/event_loop.rs` (variable/setting en vez de `127.0.0.1:3030`).
3. **Proxy en la XPS** (`moca/pty-proxy/`): websocket → PTY compatible con el protocolo de §4.2. Probarlo primero desde el build de Mac con `remote_tty`.
4. **Compilación para iOS**: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`; hacer que `warpui_core` + render + `remote_tty` compilen para `target_os = "ios"`, aislando lo que sea de macOS.
5. **`platform/ios` mínima**: ventana UIKit + superficie Metal + teclado (reutilizando ideas de `soft_keyboard`).
6. **App contenedora en Xcode** (Swift/SwiftUI) que enlaza la librería Rust, firmada con la cuenta de desarrollador, instalada en el iPad.
7. Después: terminal integrada (modo B, `moca/ios-shell/`): shell y herramientas compiladas para iOS arm64 + acceso a carpetas vía Archivos.

## 6. Ideas aparcadas
- **MCP de Tailscale**: servidor MCP que hable con la API de Tailscale (con una llave del usuario). Dos opciones: correrlo en la XPS para el Claude Code de esa máquina, o exponerlo como conector remoto en claude.ai.

## 7. Organización en el repo (para no romper el escritorio)

Mismo repo, rama aparte, carpetas nuevas. No hay copia del proyecto.

| Dónde | Qué |
|---|---|
| Rama `moca` | Escritorio estable (Mac/Windows). De aquí salen las versiones. |
| Rama `moca-ios` | Todo el trabajo de iPad. Trae `moca` seguido (`git merge moca`). |
| `crates/warpui/src/platform/ios/` | Plataforma UIKit + Metal (se crea en el paso 5). |
| `moca/ios/` | Proyecto Xcode (Swift) que envuelve la librería Rust. |
| `moca/pty-proxy/` | Servidor websocket → PTY que corre en la XPS (modo A). Proyecto Cargo aparte, fuera del workspace. |
| `moca/ios-shell/` | Shell y herramientas embebidas para la terminal integrada (modo B). |
| `docs/moca/ios.md` | Este documento. |

Reglas:
1. Código de iOS solo en archivos nuevos o detrás de `#[cfg(target_os = "ios")]` (hoy hay 0).
2. Un cambio a código compartido que iOS necesite (p. ej. el endpoint configurable) entra a `moca` por separado y solo si la Mac sigue compilando (`cargo check -p warp --bin warp-oss --features gui`).
3. Antes de unir `moca-ios` a `moca`: build de Mac + workflow manual de Windows.
4. Compilar iOS en la **M3 Pro** (pipboy-m3pro-2), no en la MacBook Air (sin ventilador).

## 8. Cómo trabajar conmigo (preferencias)
- **Reportar antes de cambiar**: primero diagnóstico y plan, luego cambios.
- Ejecución ordenada, por pasos numerados, claros y al grano.
- Explicaciones en español.

## 9. Primer prompt sugerido para Claude Code

```
Lee docs/moca/ios.md y AGENTS.md. No cambies nada todavía.
Repórtame:
1) Qué módulos y crates dependen de la feature local_tty y cuáles compilarían solo con remote_tty.
2) Qué partes de crates/warpui y warpui_core asumen macOS/AppKit y bloquearían un target aarch64-apple-ios.
3) Un plan por pasos para el paso 2 y 3 de la sección 5, con los archivos exactos a tocar.
```
