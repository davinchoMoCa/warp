# moca/ios — App de iPadOS

**v0.1 (hoy):** terminal remota nativa. App SwiftUI con [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) (MIT) que se conecta a `moca/pty-proxy` en la XPS por el tailnet. Cada vez que abre, es un shell zsh de la XPS.

**Después:** el motor de Moca Warp (Rust) compilado para iOS reemplaza a SwiftTerm, con las dos terminales (remota e integrada). Plan en [`docs/moca/ios.md`](../../docs/moca/ios.md).

## Compilar e instalar

```bash
brew install xcodegen            # una vez
moca/ios/build.sh                # compila, instala y abre en el iPad emparejado
```

- Firma con el equipo de desarrollador (automática; registra el iPad en la cuenta la primera vez).
- Lee la dirección del proxy y el token de `~/.config/moca/remote-tty.env` y los mete en `MocaWarp/Config.generated.plist`, que **no va al repo** (`.gitignore`). El proyecto `.xcodeproj` también se genera (`project.yml`).
- El iPad necesita la app de Tailscale conectada para llegar a la XPS.
