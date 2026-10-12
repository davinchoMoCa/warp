# moca/

Código propio de Moca Warp que no existe en Warp. Warp nunca toca esta carpeta, así que traer sus actualizaciones no genera conflictos aquí.

| Carpeta | Qué es |
|---|---|
| `ios/` | App de iPadOS (proyecto Xcode en Swift) que envuelve la librería Rust |
| `pty-proxy/` | Servidor websocket → PTY para la terminal remota del iPad (corre en la XPS) |
| `ios-shell/` | Shell y herramientas embebidas para la terminal integrada del iPad |

Plan y decisiones: [`docs/moca/ios.md`](../docs/moca/ios.md).
