use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::extract::ws::{Message, WebSocket};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::mpsc;
use uuid::Uuid;

use cb_db::models::{Agent, Vps, VpsState, VpsUsagePeriod};

use crate::auth::{UserId, authenticate_gateway_request};
use crate::error::ApiError;
use crate::state::AppState;

const GATEWAY_PORT: u16 = 18789;
const MAX_REQUEST_BODY: usize = 10 * 1024 * 1024; // 10 MB

// ── RPC method blocklist ────────────────────────────────────────────

fn is_blocked_method(method: &str) -> bool {
    method.starts_with("config.")
        || method.starts_with("exec.approvals.")
        || method == "exec.approval.resolve"
        || method == "update.run"
}

// ── HMAC nonce signing ──────────────────────────────────────────────

fn sign_nonce(nonce: &str, token: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes())
        .expect("HMAC accepts any key size");
    mac.update(nonce.as_bytes());
    let result = mac.finalize().into_bytes();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

// ── Gateway target resolution ───────────────────────────────────────

struct GatewayTarget {
    agent: Agent,
    vps: Vps,
    _user_id: UserId,
}

async fn resolve_gateway_target(
    headers: &HeaderMap,
    query: Option<&str>,
    state: &AppState,
    agent_id: Uuid,
) -> Result<GatewayTarget, ApiError> {
    let user_id = authenticate_gateway_request(headers, query, &state.config.jwt_secret)
        .ok_or(ApiError::Unauthorized)?;

    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id.0 {
        return Err(ApiError::NotFound);
    }

    let vps_id = agent.vps_id.ok_or(ApiError::NotFound)?;

    let vps = Vps::get_by_id(&state.db, vps_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if vps.state != VpsState::Running {
        return Err(ApiError::Conflict("VPS is not running".into()));
    }

    if vps.address.is_none() {
        return Err(ApiError::Internal("VPS has no address".into()));
    }

    Ok(GatewayTarget {
        agent,
        vps,
        _user_id: user_id,
    })
}

// ── HTTP proxy ──────────────────────────────────────────────────────

async fn proxy_http(
    State(state): State<AppState>,
    Path((agent_id, path)): Path<(Uuid, String)>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let target = resolve_gateway_target(&headers, None, &state, agent_id).await?;
    let address = target.vps.address.as_deref().unwrap();

    // Block POST /tools/invoke
    if method == Method::POST && path == "tools/invoke" {
        return Err(ApiError::BadRequest(
            "tools/invoke is blocked through the gateway proxy".into(),
        ));
    }

    // Read request body with size limit
    let body_bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return Err(ApiError::BadRequest(
                "request body too large (max 10MB)".into(),
            ));
        }
    };

    let req_size = body_bytes.len() as i64;

    let upstream_url = format!("http://{address}:{GATEWAY_PORT}/{path}");

    // Build upstream request — strip browser cookies, inject auth
    let client = reqwest::Client::new();
    let mut upstream_req = client.request(method, &upstream_url);

    // Forward safe headers (content-type, accept, etc.)
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        // Skip hop-by-hop headers and browser cookies
        if matches!(
            name_str,
            "host" | "cookie" | "authorization" | "connection" | "transfer-encoding"
        ) {
            continue;
        }
        if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
            upstream_req = upstream_req.header(name_str, v);
        }
    }

    upstream_req = upstream_req.header(
        "Authorization",
        format!("Bearer {}", target.agent.gateway_token),
    );

    upstream_req = upstream_req.body(body_bytes);

    let upstream_resp = upstream_req
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("upstream request failed: {e}")))?;

    let status = StatusCode::from_u16(upstream_resp.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);

    let resp_headers = upstream_resp.headers().clone();

    let resp_bytes = upstream_resp
        .bytes()
        .await
        .map_err(|e| ApiError::Internal(format!("failed to read upstream response: {e}")))?;

    let resp_size = resp_bytes.len() as i64;

    // Track bandwidth
    let total_bytes = req_size + resp_size;
    if total_bytes > 0 {
        let _ = VpsUsagePeriod::add_bandwidth(&state.db, target.vps.id, total_bytes).await;
    }

    // Build response
    let mut response = Response::builder().status(status);

    for (name, value) in resp_headers.iter() {
        let name_str = name.as_str();
        // Skip hop-by-hop headers
        if matches!(name_str, "transfer-encoding" | "connection") {
            continue;
        }
        if let Ok(v) = HeaderValue::from_bytes(value.as_bytes()) {
            response = response.header(name, v);
        }
    }

    Ok(response
        .body(Body::from(resp_bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

// ── WebSocket proxy ─────────────────────────────────────────────────

async fn proxy_ws(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
    axum::extract::Query(query_params): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    // Reconstruct query string for JWT extraction
    let query_string = if query_params.is_empty() {
        None
    } else {
        Some(
            query_params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&"),
        )
    };
    let target = resolve_gateway_target(
        &headers,
        query_string.as_deref(),
        &state,
        agent_id,
    )
    .await?;
    let address = target.vps.address.clone().unwrap();
    let gateway_token = target.agent.gateway_token.clone();
    let vps_id = target.vps.id;
    let db = state.db.clone();

    Ok(ws.on_upgrade(move |client_ws| {
        ws_relay(client_ws, address, gateway_token, vps_id, db)
    }))
}

async fn ws_relay(
    client_ws: WebSocket,
    address: String,
    gateway_token: String,
    vps_id: Uuid,
    db: sqlx::PgPool,
) {
    let upstream_url = format!("ws://{address}:{GATEWAY_PORT}/ws");

    let upstream_conn = tokio_tungstenite::connect_async(&upstream_url).await;

    let (upstream_ws, _) = match upstream_conn {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!(error = %e, "failed to connect to upstream WebSocket");
            return;
        }
    };

    let (mut upstream_write, mut upstream_read) = upstream_ws.split();
    let (mut client_write, mut client_read) = client_ws.split();

    let bandwidth = Arc::new(AtomicI64::new(0));

    // Channel for writing to client (shared by upstream reader + error responses)
    let (client_tx, mut client_rx) = mpsc::channel::<Message>(64);

    // Handshake state: we need to intercept the first few messages
    let handshake_done = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Task 1: client writer — drains from mpsc channel
    let write_task = tokio::spawn(async move {
        while let Some(msg) = client_rx.recv().await {
            if client_write.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Task 2: upstream reader → client
    let bw_up = bandwidth.clone();
    let client_tx_up = client_tx.clone();
    let upstream_reader = tokio::spawn(async move {
        while let Some(msg_result) = upstream_read.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(_) => break,
            };

            let data_len = match &msg {
                tokio_tungstenite::tungstenite::Message::Text(t) => t.len(),
                tokio_tungstenite::tungstenite::Message::Binary(b) => b.len(),
                tokio_tungstenite::tungstenite::Message::Close(_) => {
                    let _ = client_tx_up.send(Message::Close(None)).await;
                    break;
                }
                tokio_tungstenite::tungstenite::Message::Ping(p) => {
                    let _ = client_tx_up
                        .send(Message::Ping(p.to_vec().into()))
                        .await;
                    continue;
                }
                tokio_tungstenite::tungstenite::Message::Pong(p) => {
                    let _ = client_tx_up
                        .send(Message::Pong(p.to_vec().into()))
                        .await;
                    continue;
                }
                _ => continue,
            };

            bw_up.fetch_add(data_len as i64, Ordering::Relaxed);

            // Convert tungstenite message to axum ws message
            let axum_msg = match msg {
                tokio_tungstenite::tungstenite::Message::Text(t) => {
                    Message::Text(t.to_string().into())
                }
                tokio_tungstenite::tungstenite::Message::Binary(b) => {
                    Message::Binary(b.to_vec().into())
                }
                _ => continue,
            };

            if client_tx_up.send(axum_msg).await.is_err() {
                break;
            }
        }
    });

    // Task 3: client reader → upstream (with filtering + handshake interception)
    let bw_client = bandwidth.clone();
    let hs_done = handshake_done.clone();
    let client_reader = tokio::spawn(async move {
        while let Some(msg_result) = client_read.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(_) => break,
            };

            match msg {
                Message::Text(text) => {
                    let text_str: &str = &text;
                    bw_client.fetch_add(text_str.len() as i64, Ordering::Relaxed);

                    // Parse as JSON for filtering / handshake interception
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(text_str) {
                        let method = json
                            .get("method")
                            .and_then(|m| m.as_str())
                            .unwrap_or("");

                        // Handshake interception: replace auth token + recompute nonce
                        if !hs_done.load(Ordering::Relaxed) && method == "connect" {
                            if let Some(params) = json.get_mut("params") {
                                // Inject the real gateway token
                                if let Some(auth) = params.get_mut("auth")
                                    && let Some(a) = auth.as_object_mut()
                                {
                                    a.insert(
                                        "token".into(),
                                        serde_json::Value::String(
                                            gateway_token.clone(),
                                        ),
                                    );
                                }

                                // Recompute signedNonce if present
                                if let Some(nonce) =
                                    params.get("nonce").and_then(|n| n.as_str())
                                {
                                    let signed = sign_nonce(nonce, &gateway_token);
                                    if let Some(p) = params.as_object_mut() {
                                        p.insert(
                                            "signedNonce".into(),
                                            serde_json::Value::String(signed),
                                        );
                                    }
                                }
                            }
                            hs_done.store(true, Ordering::Relaxed);

                            let modified = serde_json::to_string(&json).unwrap_or_default();
                            let tung_msg =
                                tokio_tungstenite::tungstenite::Message::Text(modified.into());
                            if upstream_write.send(tung_msg).await.is_err() {
                                break;
                            }
                            continue;
                        }

                        // RPC method filtering
                        if is_blocked_method(method) {
                            // Send error response back to client
                            let id = json.get("id").cloned().unwrap_or(serde_json::Value::Null);
                            let error_resp = serde_json::json!({
                                "id": id,
                                "error": {
                                    "code": -32601,
                                    "message": format!("method '{}' is blocked", method)
                                }
                            });
                            let error_str = serde_json::to_string(&error_resp).unwrap_or_default();
                            let _ = client_tx.send(Message::Text(error_str.into())).await;
                            continue;
                        }
                    }

                    // Forward unmodified
                    let tung_msg = tokio_tungstenite::tungstenite::Message::Text(
                        text.to_string().into(),
                    );
                    if upstream_write.send(tung_msg).await.is_err() {
                        break;
                    }
                }
                Message::Binary(data) => {
                    bw_client.fetch_add(data.len() as i64, Ordering::Relaxed);
                    let tung_msg = tokio_tungstenite::tungstenite::Message::Binary(
                        data.to_vec().into(),
                    );
                    if upstream_write.send(tung_msg).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => {
                    let _ = upstream_write
                        .send(tokio_tungstenite::tungstenite::Message::Close(None))
                        .await;
                    break;
                }
                Message::Ping(p) => {
                    let _ = upstream_write
                        .send(tokio_tungstenite::tungstenite::Message::Ping(
                            p.to_vec().into(),
                        ))
                        .await;
                }
                Message::Pong(p) => {
                    let _ = upstream_write
                        .send(tokio_tungstenite::tungstenite::Message::Pong(
                            p.to_vec().into(),
                        ))
                        .await;
                }
            }
        }
    });

    // Wait for either side to finish
    tokio::select! {
        _ = upstream_reader => {}
        _ = client_reader => {}
    }

    // Abort remaining tasks
    write_task.abort();

    // Flush bandwidth
    let total = bandwidth.load(Ordering::Relaxed);
    if total > 0 {
        let _ = VpsUsagePeriod::add_bandwidth(&db, vps_id, total).await;
    }
}

// ── Router ──────────────────────────────────────────────────────────

pub fn gateway_router() -> Router<AppState> {
    Router::new()
        .route("/agents/{agent_id}/gateway/ws", get(proxy_ws))
        .route("/agents/{agent_id}/gateway/{*path}", any(proxy_http))
}
