// SPDX-License-Identifier: Apache-2.0

//! A fake `llama-server` for hub supervision tests — **not** a real model.
//!
//! It speaks just enough of the llama.cpp HTTP API for the hub's llama
//! supervisor and `/suggest` degradation to be tested without the heavy real
//! binary: `GET /health` returns `{"status":"ok"}`, and `POST /completion`
//! returns a canned French rephrase (streamed as SSE when `"stream":true`, or
//! token log-probabilities otherwise). It ignores `-m`/`-c` and binds the
//! `--port` the hub assigns.
//!
//! Two env hooks let a test drive its lifecycle:
//! - `FLUENCE_FAKE_LLAMA_PIDFILE` — where to write its pid, so a test can kill
//!   it to exercise crash → graceful degradation (D-2.6);
//! - `FLUENCE_FAKE_LLAMA_DIE_IF_EXISTS` — if that path exists at startup, exit
//!   immediately, so a test can make a restart fail and keep the engine down.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn main() {
    // A test can make respawns fail (keeping the engine down) by creating this
    // marker before killing the running instance.
    if let Some(marker) = std::env::var_os("FLUENCE_FAKE_LLAMA_DIE_IF_EXISTS")
        && std::path::Path::new(&marker).exists()
    {
        std::process::exit(1);
    }
    let port = parse_port().expect("--port <n> is required");
    if let Some(path) = std::env::var_os("FLUENCE_FAKE_LLAMA_PIDFILE") {
        let _ = std::fs::write(path, std::process::id().to_string());
    }

    let listener = TcpListener::bind(("127.0.0.1", port)).expect("fake-llama binds its port");
    for stream in listener.incoming().flatten() {
        thread::spawn(move || handle(stream));
    }
}

/// Finds the value of the `--port` argument.
fn parse_port() -> Option<u16> {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == "--port" {
            return args.next()?.parse().ok();
        }
    }
    None
}

/// Serves one request, then closes the connection.
fn handle(mut stream: TcpStream) {
    let Ok(peer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(peer);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => return,
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0_u8; content_length];
    if content_length > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }
    let body = String::from_utf8_lossy(&body);

    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    if path.starts_with("/health") {
        respond(
            &mut stream,
            "200 OK",
            "application/json",
            b"{\"status\":\"ok\"}",
        );
    } else if path.starts_with("/completion") {
        if body.contains("\"stream\":true") {
            let sse = concat!(
                "data: {\"content\":\"Je voudrais\"}\n\n",
                "data: {\"content\":\" de l'eau\"}\n\n",
                "data: {\"content\":\"\",\"stop\":true}\n\n",
            );
            respond(&mut stream, "200 OK", "text/event-stream", sse.as_bytes());
        } else {
            let json = concat!(
                "{\"completion_probabilities\":[{\"token\":\"e\",\"top_logprobs\":[",
                "{\"token\":\"e\",\"logprob\":-0.2},{\"token\":\"a\",\"logprob\":-1.0}]}]}",
            );
            respond(&mut stream, "200 OK", "application/json", json.as_bytes());
        }
    } else {
        respond(&mut stream, "404 Not Found", "text/plain", b"");
    }
}

/// Writes a complete HTTP/1.1 response and closes.
fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}
