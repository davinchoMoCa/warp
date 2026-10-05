# Inventario técnico del cliente Warp (fork sin servidores de Warp)

Fecha: 2026-10-04 · Repo: clon de `github.com/warpdotdev/Warp` en la raíz del workspace.
Conclusión rápida: **el cliente compila desde fuente sin herramientas privadas** (canal `warp-oss`), **el terminal/PTY/render/editor funcionan 100% local**, y **el build OSS ya trae telemetría, crash-reporting y auto-update desactivados por config**. Pero el binario OSS **apunta por defecto a los servidores de producción de Warp** (`app.warp.dev`) para login/IA/Drive/sesiones compartidas, y no existe un flag "offline/sin servidor": un fork que quiera cortar la nube tiene que parchear la config de canal. La licencia es AGPL v3 para el app (forks deben mantenerse abiertos) y MIT para `warpui`/`warpui_core`.

---

## 1. Cómo se compila

- **Toolchain**: Rust **1.92.0** con `rustfmt`, `clippy`, `rust-analyzer`, perfil `minimal` — `rust-toolchain.toml` (raíz).
- **Comando de build/run macOS** (documentado en `README.md` líneas 71-81 y `FAQ.md` líneas 29-37):
  ```bash
  ./script/bootstrap   # setup de plataforma
  ./script/run         # build + run (en macOS delega a script/macos/run: .app bundle + codesign)
  ./script/presubmit   # fmt, clippy, tests
  ```
  o directamente `cargo run`. `script/run` elige binario según disponibilidad de `warp-channel-config` (herramienta **privada**, en `ssh://git@github.com/warpdotdev/warp-channel-config.git` — `script/install_channel_config` líneas 13-15): sin acceso (caso de un fork) construye el **canal OSS**: `cargo run --bin warp-oss --features gui` (`script/run`, bloque final). El TUI headless: `./script/run-tui` (`script/run-tui`, cae al canal OSS si no hay `warp-channel-config`).
- **Dependencias de sistema (macOS)** — `script/bootstrap` + `script/macos/bootstrap`: Xcode (requerido, hace `xcode-select --switch`), Homebrew, `rustup` + target `aarch64-apple-darwin`, y via brew: `jq`, `shellcheck`, `sentry-cli`, `clang-format`, `create-dmg`, `multitime`, PowerShell (cask), `pkgconf`, `llvm`, `protobuf`, Docker (cask), gcloud SDK. Además `script/install_cargo_test_deps`, `install_cargo_release_deps`, `install_cargo_bundle`. El root `Cargo.toml` (líneas 137-396) usa varios forks propios de crates (p. ej. `warpdotdev/vte`, `warpdotdev/winit`, `warpdotdev/font-kit`) y `wgpu` 30 para render GPU.
- **Testing**: `cargo nextest run --no-fail-fast --workspace` (AGENTS.md, sección Testing).

## 2. Mapa de crates (~80 en `crates/` + `app/`)

| Crate | Rol (1 línea) |
|---|---|
| `app/` | App GUI principal (`warp`) + bins por canal (`src/bin/oss.rs`, `dev.rs`, `stable.rs`, `preview.rs`, `local.rs`, `integration.rs`): superficies de terminal, IA, auth, Drive, settings, workspaces. |
| `crates/warp_terminal` | Núcleo de emulación de terminal y PTY local (`src/local_tty`, `src/event_listener`, `src/writeable_ty`; linaje Alacritty/vte). |
| `crates/warpui` | Framework UI de píxeles para GUI: render GPU (WGSL/wgpu), windowing, fonts, browser (`src/rendering`, `src/windowing`). Licencia MIT. |
| `crates/warpui_core` | Núcleo UI compartido GUI+TUI: Entity/App/AppContext, actions, elements; además librería TUI de celdas (`src/elements/tui`). Licencia MIT. |
| `crates/warp_tui` | Front-end TUI headless (binario consola; root view "login-gated", `src/root_view.rs` línea 1). |
| `crates/editor` | Editor de texto del input (modelo, multiline, search, decorations). |
| `crates/ai` | Integración IA: API keys, custom endpoints (BYOK), skills (`src/skills`), soporte del Agent SDK. |
| `crates/graphql` (`warp_graphql`) | Cliente GraphQL (cynic) contra `app.warp.dev`: mutaciones/queries de IA, billing, objetos, teams (`src/api/mutations/`, `src/api/queries/`). |
| `crates/firebase` | Cliente REST de Firebase Auth (identitytoolkit) para tokens de login. |
| `crates/warp_server_auth` | Estado de auth, credenciales, `AuthStateProvider` (`src/auth_state.rs`). |
| `crates/warp_server_client` | Cliente HTTP/WS de los servidores Warp (auth, drive, graphql helpers, IAP). |
| `crates/cloud_objects` (+ `cloud_object_client`, `cloud_object_persistence`, `cloud_object_models`) | Sustrato de objetos de Warp Drive (IDs, metadata, permisos, sync). |
| `crates/warp_features` | Feature flags de runtime (~200 flags en `src/lib.rs`, 1342 líneas) + listas LOCAL/DOGFOOD/PREVIEW/RELEASE. |
| `crates/warp_core` | Utilidades core: estado/config de canal (`src/channel/config.rs`, `state.rs`), features, platform. |
| `crates/warp_cli` | CLI `oz`: despachar agentes con harnesses Claude/OpenCode/Gemini/Codex, secrets, MCP (`src/agent.rs`, `src/secret.rs`). |
| `crates/warp_completer` | Completions/autosuggestions de comandos (specs de Fig). |
| `crates/command` | Definiciones/ejecución de comandos de shell e integración con el terminal. |
| `crates/persistence` | Persistencia local SQLite con Diesel (migrations en `crates/persistence/migrations/`). |
| `crates/remote_server` | Desarrollo remoto por SSH: binario "remote server" que se **descarga de los servidores Warp** (`src/setup.rs` líneas 603-608). |
| `crates/http_server` | Servidor HTTP local pequeño dentro del cliente (puerto base 9277, `src/lib.rs`). |
| `crates/onboarding` | Slides de onboarding (incluye slide de agentes third-party, `src/slides/third_party_slide.rs`). |
| `crates/input_classifier` | Clasificación de input (comando vs. pregunta a IA) heurística/ONNX. |
| `crates/warp_harness_usage` | Parseo de usage de Claude Code/Codex desde transcripts locales (`src/claude.rs`, `src/codex.rs`). |
| `crates/integration` | Framework de tests de integración GUI-only (excluido de default-members, root `Cargo.toml` líneas 8-23). |

## 3. Dependencia del servidor propietario

- **Qué NO está en el repo** (confirmado en `FAQ.md` líneas 91-99): el **server**, el **backend de Warp Drive**, la **auth hospedada** y **Oz** (orquestación de agentes). `README.md` línea 26: los workflows agénticos corren con modelos GPT vía OpenAI (sponsor fundador).
- **¿Arranca sin login/cuenta?** Sí en GUI, con matices:
  - `FAQ.md` líneas 101-103: "Some functionality works fully locally; other features (Drive sync, hosted-model agents, team features) require Warp's backend."
  - El login solo se exige post-onboarding si: account-first activo, o IA activada, o Drive activado — `app/src/root_view.rs` líneas 634-639 (`requires_post_onboarding_login`) y tests en `app/src/root_view_tests.rs` líneas 80-93. **Ojo**: el flujo "account-first" **viene activado por defecto** (cargo feature `account_first_onboarding` en la lista `default` de `app/Cargo.toml` línea 609 → `app/src/features.rs` línea 92), así que el onboarding OSS **empuja login al usuario** desde el arranque.
  - "Login later"/skip: `app/src/auth/login_slide.rs` líneas 486-505 — si no está el flag `SkipFirebaseAnonymousUser`, el skip **crea un usuario anónimo vía mutación GraphQL al server** (`app/src/auth/auth_manager.rs` líneas 668-731, mutación `crates/graphql/src/api/mutations/create_anonymous_user.rs` + Firebase). Sin server, eso falla (`CreateAnonymousUserFailed`, manejado en `app/src/auth/auth_view_modal.rs` líneas 314-317). **No encontrado**: evidencia de que el app quede bloqueado sin red; el flujo fallback sin IA/Drive no exige login.
  - El **TUI sí es login-gated**: `crates/warp_tui/src/root_view.rs` línea 1 ("login-gated root view") y flujo de device-authorization contra `app.warp.dev/device` (`app/src/tui/mod.rs`, `TuiLoginPhase::AwaitingLogin`).
- **El binario OSS apunta a producción**: `app/src/bin/oss.rs` líneas 16-21 usa `WarpServerConfig::production()` → `https://app.warp.dev`, RTC `wss://rtc.app.warp.dev/graphql/v2`, sesiones `wss://sessions.app.warp.dev`, Firebase API key embebida (`crates/warp_core/src/channel/config.rs` líneas 56-66) y Oz `https://oz.warp.dev` (líneas 79-86).
- **Qué se rompe sin el server propietario**:
  - **IA integrada (modelos hospedados)**: la generación de diálogo es mutación GraphQL al server — `app/src/server/server_api/ai.rs` líneas 1973-2022 (`GenerateDialogue`), créditos/limits en `crates/graphql/src/api/ai.rs`.
  - **Warp Drive / sync de objetos**: WS a `rtc.app.warp.dev` (`config.rs` línea 60) + queries de objetos (`crates/graphql/src/api/queries/get_updated_cloud_objects.rs`); UI de Drive gated por auth (`app/src/drive/index.rs` líneas 2323, 5000).
  - **Sesiones compartidas**: `sessions.app.warp.dev` (`config.rs` línea 61).
  - **Login/anonymous/Firebase**: mutaciones `create_anonymous_user`, `mint_custom_token` (`crates/graphql/src/api/mutations/`) + `crates/firebase/src/lib.rs`.
  - **Teams/billing/settings-sync**: `crates/graphql/src/api/billing.rs`, `queries/get_user_settings.rs`, `queries/get_workspaces_metadata_for_user.rs`.
  - **Agentes cloud/Oz**: orquestación multi-agente contra `oz.warp.dev` (`config.rs` líneas 68-86; `crates/warp_multi_agent_client`).
  - **Remote server (SSH dev)**: el binario se descarga de `{server_root_url}/download/cli` — `crates/remote_server/src/setup.rs` líneas 603-608.
  - **Auto-update**: desactivado en OSS (ver sección 4), así que no aplica.
- **Qué funciona local**: PTY/terminal/blocks (`crates/warp_terminal`), render/editor/completions (`crates/warp_completer`), workflows, settings locales, MCP (`rmcp`), y **agentes CLI externos como procesos hijos locales** (`app/src/ai/agent_sdk/driver/harness/claude_code.rs`, `codex.rs`, `gemini.rs`).

## 4. ¿Feature flag / cfg / modo para desactivar server/nube/telemetría?

- **No existe un flag "offline" / "no-server" / "skip_login" / "self_hosted" como feature**. Búsqueda en `app/Cargo.toml` (`[features]` líneas 464+) y en código (`offline_mode`, `self_hosted`, `skip_login`): no encontrado. Las features cargo son gates de producto (p. ej. `agent_mode`, `account_first_onboarding`), no un interruptor de nube.
- **Telemetría / crash-reporting / auto-update YA vienen apagados en el canal OSS por configuración**: `app/src/bin/oss.rs` líneas 18-20 (`telemetry_config: None`, `crash_reporting_config: None`, `autoupdate_config: None`). `crates/warp_core/src/channel/state.rs` líneas 192-211 (`is_telemetry_available` / `is_crash_reporting_available` devuelven `false` y la UI de los toggles se oculta: "Builds like OpenWarp intentionally ship with `telemetry_config: None`"). Toggle de privacidad del usuario: `app/src/settings/privacy.rs`.
- **Env vars de redirección existen pero no aplican a un fork**: `WITH_LOCAL_SERVER`, `SERVER_ROOT_URL`, `WS_SERVER_URL` (AGENTS.md "Running with local warp-server"; `script/run` mapea `with_local_server`→`WITH_LOCAL_SERVER`; `app/build.rs` líneas 175-181). Solo tienen efecto en builds de canales internos vía `warp-channel-config` (privado — `script/install_channel_config` líneas 13-15; `crates/warp_channel_config/src/lib.rs` líneas 1-9), que un fork no puede instalar. **Para redirigir/cortar el server en un fork hay que parchear** `WarpServerConfig::production()` (`crates/warp_core/src/channel/config.rs`) o `app/src/bin/oss.rs`.
- **Feature flags de runtime**: `crates/warp_features/src/lib.rs` (~1342 líneas). Relevantes: `SkipFirebaseAnonymousUser` (líneas 795-800: el skip de login no crea usuario Firebase, queda "fully logged out") — **no encontrado** en ninguna lista default (DEBUG/LOCAL/DOGFOOD/PREVIEW/RELEASE, líneas 1014-1096); `LocalClaudeCodexChildHarnesses` (líneas 715-717, solo `LOCAL_FLAGS` interno).

## 5. Soporte de agentes CLI externos (Claude Code, Codex, etc.)

- **Política**: `FAQ.md` líneas 59-69 — puedes usar cualquier agente propio (Claude Code, Codex, Gemini CLI, Cursor); el harness del agente built-in de Warp corre **server-side** y no está abierto; planean ACP (`agentclientprotocol.com`) para conectar modelos/suscripciones propias.
- **Harnesses soportados** — `crates/warp_cli/src/agent.rs` líneas 276-291: enum `Harness { Oz (default, server), Claude, OpenCode, Gemini, Codex }`; `"claude"`/`"claude-code"` delegan al CLI (`agent.rs` líneas 306-308, 374, 391).
- **Implementaciones `ThirdPartyHarness`** (procesos hijos locales gestionados por el cliente): `app/src/ai/agent_sdk/driver/harness/claude_code.rs`, `codex.rs`, `gemini.rs`, más `claude_transcript.rs`, `codex_transcript.rs`, `save_coordinator.rs` (checkpoints/SavePoints), `process_control.rs`, `exit_escalation.rs`, `usage_reporting.rs`. Ejemplo: `CodexHarness` levanta un `JSONMCPServer` (codex.rs línea 39) y le inyecta config de modelo (`set_codex_model`, líneas 844-906).
- **Qué hace Warp de especial**: los corre como child processes con MCP, transcripciones/checkpoints persistentes, reanudación (`ResumeOptions::ThirdParty`, `app/src/ai/agent_sdk/driver.rs` líneas 594, 1117), telemetría de runtime, exit-escalation, y **skills compartidas**: carga skills de `.agents/skills/`, `.warp/skills/`, `.claude/skills/`, `.codex/skills/` (`crates/warp_cli/src/agent.rs` líneas 483, 700; `crates/ai/src/skills/skill_provider.rs` líneas 17-123).
- **Onboarding y extras**: slide de defaults para "Claude Code, Codex, and Gemini" (`crates/onboarding/src/slides/third_party_slide.rs` línea 150); plugin de notificaciones de Codex (flags `CodexNotifications`/`CodexPlugin`, `crates/warp_features/src/lib.rs` líneas 785-791); conteo de usage leyendo transcripts locales (`crates/warp_harness_usage/src/claude.rs`, `codex.rs`); feature `external_agent_mode_context` (default ON, `app/Cargo.toml`).
- **Caveat**: la disponibilidad de harnesses en orquestación se consulta al server (`crates/graphql/src/api/queries/get_available_harnesses.rs`) y el harness default `Oz` es server-side; los third-party child harnesses corren local pero la UI de agentes tiene partes gated por login (p. ej. `app/src/settings/ai.rs` líneas 2268-2273).

## 6. Tamaño del código

- **Archivos `.rs`**: **4.068** (`find . -name '*.rs' -not -path './target/*' | wc -l`).
- **Líneas totales**: **~1.753.676** (~1,75 M) (`find . -name '*.rs' -not -path './target/*' | xargs cat | wc -l`).
- **Crates**: 80 directorios en `crates/` + `app/` (workspace `Cargo.toml` líneas 1-23; `serve-wasm` e `integration` excluidos de default-members).
- Raíz `Cargo.toml`: 587 líneas (deps + perfiles release/dev/wasm/cli/tui).

---

## Síntesis para un fork sin servidores Warp

1. **Compila**: sí, sin herramientas privadas (`warp-oss` + features default `gui`). La única pieza privada opcional es `warp-channel-config`, y su ausencia se maneja graceful (`script/run`: "Skipping internal channel config installation").
2. **Terminal local funciona**: PTY, render, blocks, editor, completions, workflows — sin server y (en flujo fallback) sin login.
3. **Hay que parchear para "des-nubificar"**: el OSS bin hardcodea `app.warp.dev`/Firebase/Oz (`app/src/bin/oss.rs`, `crates/warp_core/src/channel/config.rs`). Sin parche, el app seguirá llamando home para login/IA/Drive (fallirán como features no disponibles, no como crash de arranque, según FAQ).
4. **Telemetría/crash/updates ya vienen OFF en OSS** — no hay que hacer nada.
5. **Legal**: AGPL v3 para el app (derivados hospedados/distribuidos deben mantenerse abiertos — `FAQ.md` líneas 111-131); MIT solo para `warpui`/`warpui_core`.
