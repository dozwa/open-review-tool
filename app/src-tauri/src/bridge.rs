//! Entwicklungsbrücke (Feature `bridge`): dieselben Befehle wie in der App, angeboten über HTTP auf
//! `127.0.0.1`. Damit lässt sich die Oberfläche im Browser gegen den echten Kern bedienen, etwa für
//! Durchläufe mit Chrome (`npx vite`, dann `http://127.0.0.1:5173/?bridge=<port>`). Nie im Release-Build.
//!
//! Start: `ORT_BRIDGE_PORT=5181 ORT_DATA_DIR=/tmp/ort-a ORT_KEY_STORE=file cargo run -p open-review-tool
//! --features bridge`. Je Person eine Brücke mit eigenem Port und Datenverzeichnis.
//!
//! Anfrage: `POST /` mit `{"cmd": …, "args": {…}}`. Antwort: `{"ok": …}`, `{"err": …}` oder für Bytes
//! `{"raw": "<base64>"}`. Dateidialoge gibt es hier nicht; die Oberfläche bekommt mit `__bridge_out`
//! ein Verzeichnis für gespeicherte Dateien.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::MockRuntime;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

/// Nur die Vorschau von Vite darf die Brücke ansprechen.
const ORIGIN: &str = "http://127.0.0.1:5173";

pub fn run(port: u16) {
    let data_dir = PathBuf::from(std::env::var_os("ORT_DATA_DIR").expect("Die Brücke braucht ORT_DATA_DIR"));
    assert!(
        std::env::var("ORT_KEY_STORE").is_ok_and(|v| v == "file"),
        "Die Brücke braucht ORT_KEY_STORE=file, damit der Schlüsselbund unberührt bleibt"
    );
    let out = data_dir.join("out");
    std::fs::create_dir_all(&out).expect("Ausgabeverzeichnis anlegen");
    let state = crate::state(data_dir.clone()).expect("Datenverzeichnis anlegen");
    let app = mock_builder()
        .manage(state)
        .invoke_handler(crate::handler())
        .build(mock_context(noop_assets()))
        .expect("Brücke konnte nicht starten");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default()).build().expect("Fenster der Brücke");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("Port der Brücke belegt");
    eprintln!("Brücke auf http://127.0.0.1:{port}, Daten in {}", data_dir.display());
    for stream in listener.incoming().flatten() {
        if let Err(e) = serve(&webview, &out, stream) {
            eprintln!("Brücke: {e}");
        }
    }
}

fn serve(webview: &WebviewWindow<MockRuntime>, out: &std::path::Path, mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;

    let answer = if line.starts_with("POST") {
        let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let cmd = request["cmd"].as_str().unwrap_or_default().to_string();
        let args = request.get("args").cloned().unwrap_or(json!({}));
        if cmd == "__bridge_out" {
            json!({ "ok": out.display().to_string() })
        } else {
            invoke(webview, cmd, args)
        }
    } else {
        Value::Null
    };
    let text = answer.to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: {ORIGIN}\r\n\
         Access-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: content-type\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
}

fn invoke(webview: &WebviewWindow<MockRuntime>, cmd: String, args: Value) -> Value {
    let request = InvokeRequest {
        cmd,
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "tauri://localhost".parse().expect("feste Adresse"),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    match get_ipc_response(webview, request) {
        Ok(InvokeResponseBody::Raw(bytes)) => json!({ "raw": B64.encode(bytes) }),
        Ok(InvokeResponseBody::Json(text)) => {
            json!({ "ok": serde_json::from_str::<Value>(&text).unwrap_or(Value::Null) })
        }
        Err(e) => json!({ "err": e }),
    }
}
