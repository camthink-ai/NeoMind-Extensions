//! The `/v1/systemone` sidecar server — the extension's decision endpoint.
//!
//! The platform's `LayaSidecar` HTTP client (already shipped in every
//! build) speaks this protocol, so hosting it here moves the whole LAYA
//! runtime out of the compiled binary: install the extension → decision
//! layer, on ANY build. Binds 127.0.0.1 only (loopback — this is a local
//! capability, not a network service).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;

use super::laya::{LayaEngine, WireQuestion};

/// POST body: `{state, questions: {qid: {type, instructions, criteria}}}`.
#[derive(Deserialize)]
pub struct SystemOneRequest {
    pub state: String,
    pub questions: BTreeMap<String, WireQuestion>,
}

/// Bind the sidecar listener. Split from [`serve_on`] so the caller can
/// distinguish "bind failed" (port taken — retry later, report honestly)
/// from "serving", and only then claim the engine is up.
pub fn bind(port: u16) -> Result<tiny_http::Server, String> {
    tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| format!("bind 127.0.0.1:{port}: {e}"))
}

/// Serve forever on the calling thread. Every request is answered; protocol
/// errors return 400 with a JSON error body (the client surfaces the text).
pub fn serve(engine: Arc<LayaEngine>, port: u16) -> Result<(), String> {
    serve_on(bind(port)?, engine)
}

/// The serve loop over an ALREADY-BOUND listener (see [`bind`]).
pub fn serve_on(server: tiny_http::Server, engine: Arc<LayaEngine>) -> Result<(), String> {
    for mut request in server.incoming_requests() {
        let (status, body) = match (request.method(), request.url()) {
            (tiny_http::Method::Get, "/health") => (200, "{}".to_string()),
            (tiny_http::Method::Post, "/v1/systemone") => {
                let mut raw = String::new();
                if let Err(e) = request.as_reader().read_to_string(&mut raw) {
                    (400, format!("{{\"error\":\"read body: {e}\"}}"))
                } else {
                    match serde_json::from_str::<SystemOneRequest>(&raw) {
                        Ok(req) => match engine.decide(&req.state, &req.questions) {
                            Ok(answers) => {
                                let mut obj = serde_json::Map::new();
                                obj.insert(
                                    "model".into(),
                                    serde_json::Value::String("laya-native-ext".into()),
                                );
                                obj.insert(
                                    "answers".into(),
                                    serde_json::to_value(&answers).unwrap_or_default(),
                                );
                                (200, serde_json::Value::Object(obj).to_string())
                            }
                            Err(e) => (500, format!("{{\"error\":\"{e}\"}}")),
                        },
                        Err(e) => (400, format!("{{\"error\":\"bad request: {e}\"}}")),
                    }
                }
            }
            _ => (404, "{\"error\":\"not found\"}".to_string()),
        };
        let response = tiny_http::Response::from_string(body).with_status_code(status);
        let _ = request.respond(response);
    }
    Ok(())
}
