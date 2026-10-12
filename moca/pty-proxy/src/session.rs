use std::io::{self, Read, Write};
use std::net::SocketAddr;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::Message;
use url::form_urlencoded;

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const PTY_BUF: usize = 8192;

pub async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    expected_token: String,
    shell: String,
) {
    if let Err(error) = run(stream, peer, expected_token, shell).await {
        eprintln!("[{peer}] sesión terminada con error: {error:#}");
    }
}

async fn run(
    stream: TcpStream,
    peer: SocketAddr,
    expected_token: String,
    shell: String,
) -> Result<()> {
    let (params_tx, params_rx) = std::sync::mpsc::channel::<PtyParams>();

    let ws = match accept_hdr_async(stream, move |req: &Request, resp: Response| {
        match authorize(req, &expected_token, peer, &params_tx) {
            Some(error) => Err(error),
            None => Ok(resp),
        }
    })
    .await
    {
        Ok(ws) => ws,
        // el rechazo ya se respondió (401/404) y se registró dentro del handshake
        Err(tokio_tungstenite::tungstenite::Error::Http(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    let params = params_rx.recv().context("el handshake terminó sin validar la sesión")?;
    eprintln!("[{peer}] conexión aceptada: terminal {}x{}", params.cols, params.rows);

    let home = std::env::var("HOME").context("la variable HOME no está definida")?;

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: params.rows,
            cols: params.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("no se pudo crear la PTY")?;

    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l");
    cmd.cwd(&home);
    for key in ["HOME", "USER", "LOGNAME", "SHELL", "PATH", "LANG", "LC_ALL"] {
        if let Ok(value) = std::env::var(key) {
            cmd.env(key, value);
        }
    }
    cmd.env("TERM", "xterm-256color");

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .context("no se pudo lanzar el shell")?;
    // sin soltar el slave, el EOF de la PTY nunca llegaría tras la muerte del shell
    drop(pair.slave);

    let master = pair.master;
    let mut reader = master
        .try_clone_reader()
        .context("no se pudo abrir el lector de la PTY")?;
    let mut writer = master
        .take_writer()
        .context("no se pudo abrir el escritor de la PTY")?;
    let mut killer = child.clone_killer();

    let (pty_in_tx, mut pty_in_rx) = mpsc::channel::<PtyInput>(64);
    let (pty_out_tx, mut pty_out_rx) = mpsc::channel::<Vec<u8>>(256);
    let (child_exit_tx, mut child_exit_rx) = mpsc::channel::<()>(1);

    // la PTY se lee y escribe de forma bloqueante: en threads aparte
    std::thread::spawn(move || {
        let mut buf = [0u8; PTY_BUF];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if pty_out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break; // el websocket ya no está
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });

    std::thread::spawn(move || {
        while let Some(input) = pty_in_rx.blocking_recv() {
            match input {
                PtyInput::Write(bytes) => {
                    if writer.write_all(&bytes).and_then(|_| writer.flush()).is_err() {
                        break; // la PTY se cerró
                    }
                }
                PtyInput::Resize(size) => {
                    let _ = master.resize(size);
                }
            }
        }
        // al caer aquí se sueltan writer y master: la PTY se cierra y el shell recibe SIGHUP
    });

    std::thread::spawn(move || {
        let _ = child.wait();
        let _ = child_exit_tx.blocking_send(());
    });

    let (mut ws_sink, mut ws_stream) = ws.split();
    let mut shell_ended = false;

    loop {
        tokio::select! {
            output = pty_out_rx.recv() => {
                match output {
                    Some(bytes) => {
                        if ws_sink.send(Message::Binary(bytes.into())).await.is_err() {
                            break; // el cliente se desconectó
                        }
                    }
                    None => {
                        shell_ended = true;
                        break;
                    }
                }
            }
            _ = child_exit_rx.recv() => {
                // el shell terminó aunque algún proceso retenga la tty
                shell_ended = true;
                break;
            }
            message = ws_stream.next() => {
                match message {
                    Some(Ok(Message::Binary(data))) => {
                        if pty_in_tx.send(PtyInput::Write(data.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Some(size) = parse_resize(text.as_str()) {
                            eprintln!(
                                "[{peer}] redimensiona la terminal a {}x{}",
                                size.cols, size.rows
                            );
                            let _ = pty_in_tx.send(PtyInput::Resize(size)).await;
                        }
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                }
            }
        }
    }

    if shell_ended {
        let _ = ws_sink.send(Message::Close(None)).await;
        let _ = ws_sink.close().await;
    }
    drop(pty_in_tx); // el hilo de escritura suelta la PTY y manda SIGHUP al shell
    let _ = killer.kill(); // refuerzo por si el shell sobrevive al SIGHUP
    eprintln!("[{peer}] fin de sesión");
    Ok(())
}

struct PtyParams {
    rows: u16,
    cols: u16,
}

enum PtyInput {
    Write(Vec<u8>),
    Resize(PtySize),
}

#[derive(Deserialize)]
struct ResizeMessage {
    width: u32,
    height: u32,
    #[serde(default)]
    width_px: u32,
    #[serde(default)]
    height_px: u32,
}

/// Valida ruta, token y parámetros de tamaño; devuelve la respuesta HTTP de rechazo.
fn authorize(
    req: &Request,
    expected_token: &str,
    peer: SocketAddr,
    params_tx: &std::sync::mpsc::Sender<PtyParams>,
) -> Option<ErrorResponse> {
    let uri = req.uri();
    if uri.path() != "/create" {
        eprintln!("[{peer}] conexión rechazada: ruta {} no permitida", uri.path());
        return Some(error_response(404));
    }

    let query = uri.query().unwrap_or_default();
    let mut rows = None;
    let mut cols = None;
    let mut token = None;
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "num_rows" => rows = value.parse().ok(),
            "num_cols" => cols = value.parse().ok(),
            "token" => token = Some(value.into_owned()),
            _ => {}
        }
    }

    if !token.as_deref().is_some_and(|token| constant_time_eq(token, expected_token)) {
        eprintln!("[{peer}] conexión rechazada: token inválido o ausente");
        return Some(error_response(401));
    }

    let _ = params_tx.send(PtyParams {
        rows: rows.unwrap_or(DEFAULT_ROWS),
        cols: cols.unwrap_or(DEFAULT_COLS),
    });
    None
}

fn error_response(status: u16) -> ErrorResponse {
    Response::builder()
        .status(status)
        .body(Some(String::new()))
        .expect("respuesta de error válida")
}

fn parse_resize(text: &str) -> Option<PtySize> {
    let message: ResizeMessage = serde_json::from_str(text).ok()?;
    Some(PtySize {
        rows: clamp_size(message.height),
        cols: clamp_size(message.width),
        pixel_width: clamp_size(message.width_px),
        pixel_height: clamp_size(message.height_px),
    })
}

fn clamp_size(value: u32) -> u16 {
    value.min(u16::MAX as u32) as u16
}

/// Comparación en tiempo constante para no filtrar información del token por timing.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}
