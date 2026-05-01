//! Local HTTP bridge for the SSH-remote workflow.
//!
//! Runs on the laptop. The remote Claude Code hook posts the raw hook JSON
//! to `POST /` over an `ssh -R 7777:127.0.0.1:7777` tunnel; this binary
//! forwards each body to the same `process_hook_json` path used by `vibekeys hook`.
//!
//! Binds to loopback by default. Anyone with shell access on the laptop can
//! send fake hooks; do not expose this on a non-loopback interface.

use crate::process_hook_json;
use log::{info, warn};
use std::io::Read;
use tiny_http::{Method, Response, Server};

pub async fn run(bind: &str) -> anyhow::Result<()> {
    let server = Server::http(bind).map_err(|e| anyhow::anyhow!("bind {bind}: {e}"))?;
    info!("vibekeys serve listening on {bind}");

    let handle = tokio::runtime::Handle::current();

    // tiny_http is blocking; run its accept loop on a dedicated blocking thread
    // and dispatch each request body into the async runtime.
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        for mut request in server.incoming_requests() {
            // Health check.
            if request.method() == &Method::Get {
                let _ = request.respond(Response::from_string("ok"));
                continue;
            }
            if request.method() != &Method::Post {
                let _ = request.respond(Response::from_string("method not allowed").with_status_code(405));
                continue;
            }

            let mut body = Vec::with_capacity(1024);
            if let Err(e) = request.as_reader().read_to_end(&mut body) {
                warn!("read body: {e}");
                let _ = request.respond(Response::from_string("bad body").with_status_code(400));
                continue;
            }

            // Reply immediately so the remote hook never blocks Claude Code.
            let _ = request.respond(Response::empty(204));

            // Forward to BLE on the async runtime; ignore errors per-request
            // (the device may be off, out of range, etc.).
            handle.spawn(async move {
                if let Err(e) = process_hook_json(&body).await {
                    warn!("hook processing failed: {e}");
                }
            });
        }
        Ok(())
    })
    .await??;

    Ok(())
}
