mod session;

use std::path::Path;

use anyhow::{bail, Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let listen = match std::env::var("MOCA_PTY_LISTEN") {
        Ok(value) if !value.is_empty() => value,
        _ => bail!(
            "falta MOCA_PTY_LISTEN (p. ej. \"100.1.2.3:3030\"), la dirección donde escuchar.\n\
             no hay valor por defecto a propósito: el servidor entrega acceso a un shell"
        ),
    };
    let token = match std::env::var("MOCA_PTY_TOKEN") {
        Ok(value) if !value.is_empty() => value,
        _ => bail!("falta MOCA_PTY_TOKEN o está vacía: es obligatoria para autenticar las conexiones"),
    };
    let shell = resolve_shell()?;

    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("no se pudo escuchar en {listen}"))?;
    eprintln!("moca-pty-proxy escuchando en ws://{listen}/create (shell: {shell})");

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(session::handle_connection(
                    stream,
                    peer,
                    token.clone(),
                    shell.clone(),
                ));
            }
            Err(error) => eprintln!("error aceptando conexión: {error}"),
        }
    }
}

fn resolve_shell() -> Result<String> {
    let preferred = std::env::var("MOCA_PTY_SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    if Path::new(&preferred).exists() {
        return Ok(preferred);
    }
    if Path::new("/bin/bash").exists() {
        eprintln!("{preferred} no existe; usando /bin/bash");
        return Ok("/bin/bash".to_string());
    }
    bail!("no hay un shell usable (probé {preferred} y /bin/bash)");
}
