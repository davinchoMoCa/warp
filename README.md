# Moca Warp

Fork de la terminal [Warp](https://github.com/warpdotdev/warp) que corre **100 % local**: sin cuenta de Warp, sin servidores de Warp y sin telemetría.

El objetivo es tener una terminal propia para trabajar con agentes de código (Claude Code, Codex, OpenCode, Gemini CLI…) que detecte qué agente corre en cada pestaña y en qué estado está, al estilo de Orca.

El README original de Warp está en [README.upstream.md](README.upstream.md).

## Qué cambiamos

| Cambio | Dónde |
|---|---|
| Configuración `offline()`: todas las llamadas al servidor y a Oz van a `127.0.0.1:1` y fallan al instante, sin salir a internet | `crates/warp_core/src/channel/config.rs`, `app/src/bin/oss.rs` |
| Ya no obliga a crear cuenta al arrancar (se quitó `account_first_onboarding` de las features por defecto) | `app/Cargo.toml` |
| Se ocultan todas las invitaciones a cuenta ("Sign in", "Sign up", "Log in", "Create an account", "Upgrade") y el botón `/remote-control`, que depende de la nube | 15 sitios marcados con `// moca: sin cuentas` |
| Constante `ACCOUNTS_DISABLED` que controla lo anterior | `crates/warp_core/src/moca.rs` |
| La bienvenida se salta "How do you want to work?" y entra en modo "solo terminal", con barra y avisos de agentes CLI encendidos | `crates/onboarding/src/model.rs` |
| Nombre de la app: **Moca Warp**; identificador `gt.moca.MocaWarp` (datos separados de Warp.app) | `app/Cargo.toml`, `app/src/bin/oss.rs`, `crates/warp_core/src/paths.rs`, `crates/warp_core/src/channel/state.rs`, `script/macos/run` |

Ya venían apagados en la versión open source de Warp: telemetría, reporte de fallos y actualizaciones automáticas.

### Qué funciona y qué no

- **Funciona:** terminal, bloques, editor, autocompletado, workflows, settings locales, MCP y la detección de agentes CLI con su estado (en progreso, esperando permiso, terminado, fallido).
- **No funciona** (depende de servidores cerrados de Warp): la IA propia de Warp, Warp Drive, sesiones compartidas, equipos y Oz.

Los análisis detallados están en [docs/moca/reporte-repo.md](docs/moca/reporte-repo.md) y [docs/moca/reporte-offline.md](docs/moca/reporte-offline.md).

## Compilar y abrir (macOS)

Requisitos: Xcode, Rust (la versión la fija `rust-toolchain.toml`), Homebrew.

```bash
xcodebuild -downloadComponent MetalToolchain   # una vez, ~700 MB
brew install protobuf pkgconf cargo-binstall
./script/install_cargo_bundle
./script/run                                   # compila y abre "Moca Warp.app"
```

La app queda en `target/debug/bundle/osx/Moca Warp.app`. No hace falta el `./script/bootstrap` completo de Warp (instala Docker, gcloud, etc.).

## Versiones y firma

La app se firma con el certificado **Apple Development: Jose David Moreira** (equipo `DSXB7C9XQA`), fijado en `script/macos/run`; se puede cambiar con `MOCA_SIGNING_CERT`. El equipo también define la carpeta compartida de datos (`DSXB7C9XQA.gt.moca.warp`, en `crates/warp_core/src/macos.rs` y `paths.rs`).

Para sacar una versión nueva:

```bash
./script/moca/release 0.2.0             # compila release, firma, instala en /Applications, commit + tag v0.2.0
./script/moca/release 0.2.0 --publicar  # además crea un GitHub Release con el .zip
```

La versión se guarda en `app/Cargo.toml` y se ve en Finder > Obtener información.

## Ramas

- `moca`: nuestra versión (rama por defecto).
- `master`: copia del Warp original, para traer sus actualizaciones.

Para actualizar desde Warp:

```bash
git fetch upstream
git checkout master && git merge upstream/master
git checkout moca && git merge master
grep -rn "moca: sin cuentas" app/src crates   # revisar que nuestros cambios sigan en pie
```

## Licencia

Igual que Warp: **AGPL v3** para la app ([LICENSE-AGPL](LICENSE-AGPL)) y **MIT** para `warpui` y `warpui_core` ([LICENSE-MIT](LICENSE-MIT)). Este fork modifica el código original de Denver Technologies, Inc.; los cambios están listados arriba. Si se distribuye, debe seguir siendo código abierto bajo AGPL.
