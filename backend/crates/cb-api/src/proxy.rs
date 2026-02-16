use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::{PROXY_AUTHENTICATE, PROXY_AUTHORIZATION};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

use cb_db::models::{Agent, OverageBudget, Plan, User, Vps, VpsUsagePeriod};

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ProxyResponse = Response<Full<Bytes>>;

pub fn spawn_proxy(listen_addr: SocketAddr, db: PgPool) {
    tokio::spawn(async move {
        if let Err(e) = run_proxy(listen_addr, db).await {
            tracing::error!(error = %e, "proxy listener failed");
        }
    });
}

async fn run_proxy(listen_addr: SocketAddr, db: PgPool) -> Result<(), BoxError> {
    let listener = TcpListener::bind(listen_addr).await?;
    tracing::info!(addr = %listen_addr, "starting forward proxy");

    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    loop {
        let (stream, peer) = listener.accept().await?;
        let db = db.clone();
        let http_client = http_client.clone();

        tokio::spawn(async move {
            let db = db.clone();
            let http_client = http_client.clone();

            let service = service_fn(move |req: Request<Incoming>| {
                let db = db.clone();
                let http_client = http_client.clone();
                async move {
                    Ok::<_, Infallible>(match handle_request(req, db, http_client).await {
                        Ok(resp) => resp,
                        Err(e) => {
                            tracing::error!(error = %e, "proxy handler error");
                            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
                        }
                    })
                }
            });

            let builder = auto::Builder::new(TokioExecutor::new());
            let conn = builder.serve_connection_with_upgrades(TokioIo::new(stream), service);

            if let Err(e) = conn.await {
                tracing::debug!(peer = %peer, error = %e, "proxy connection error");
            }
        });
    }
}

async fn handle_request(
    req: Request<Incoming>,
    db: PgPool,
    http_client: reqwest::Client,
) -> Result<ProxyResponse, BoxError> {
    // Authenticate
    let agent = match authenticate(&req, &db).await {
        Ok(agent) => agent,
        Err(resp) => return Ok(resp),
    };

    // Resolve VPS
    let vps_id = match agent.vps_id {
        Some(id) => id,
        None => return Ok(error_response(StatusCode::FORBIDDEN, "agent has no VPS")),
    };

    // Provider-aware usage enforcement:
    // - Hetzner: skip proxy-side check; the monitor handles enforcement by stopping VPSes
    // - Fly (and others): per-request gating with overage budget support
    let vps = Vps::get_by_id(&db, vps_id)
        .await
        .map_err(|e| -> BoxError { e.into() })?;

    let is_hetzner = vps.provider.parse::<cb_infra::ProviderName>().ok()
        == Some(cb_infra::ProviderName::Hetzner);

    if !is_hetzner && let Err(resp) = check_usage(&vps, &db).await {
        return Ok(resp);
    }

    if req.method() == Method::CONNECT {
        handle_connect(req, db, vps_id).await
    } else {
        handle_plain_http(req, db, vps_id, http_client).await
    }
}

// ── Authentication ───────────────────────────────────────────────────

async fn authenticate(req: &Request<Incoming>, db: &PgPool) -> Result<Agent, ProxyResponse> {
    let header = req
        .headers()
        .get(PROXY_AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(proxy_auth_required)?;

    let encoded = header
        .strip_prefix("Basic ")
        .ok_or_else(proxy_auth_required)?;

    let decoded = BASE64.decode(encoded).map_err(|_| proxy_auth_required())?;
    let credentials = String::from_utf8(decoded).map_err(|_| proxy_auth_required())?;

    let (agent_id_str, token) = credentials
        .split_once(':')
        .ok_or_else(proxy_auth_required)?;

    let agent_id = agent_id_str
        .parse::<Uuid>()
        .map_err(|_| proxy_auth_required())?;

    Agent::get_by_id_and_token(db, agent_id, token)
        .await
        .map_err(|_| proxy_auth_required())
}

fn proxy_auth_required() -> ProxyResponse {
    Response::builder()
        .status(StatusCode::PROXY_AUTHENTICATION_REQUIRED)
        .header(PROXY_AUTHENTICATE, "Basic realm=\"slopbox\"")
        .body(Full::new(Bytes::from("Proxy authentication required")))
        .unwrap()
}

// ── Usage check ─────────────────────────────────────────────────────

/// Check aggregate user-level usage against plan limits + overage budget.
///
/// Used for Fly VPSes (and any future elastic providers). Hetzner VPSes
/// skip this — the monitor handles enforcement by stopping servers.
async fn check_usage(vps: &Vps, db: &PgPool) -> Result<(), ProxyResponse> {
    let err = |_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error");

    let user = User::get_by_id(db, vps.user_id).await.map_err(err)?;

    let plan_id = user
        .plan_id
        .ok_or_else(|| error_response(StatusCode::FORBIDDEN, "no plan"))?;

    let plan = Plan::get_by_id(db, plan_id).await.map_err(err)?;

    // Aggregate usage across all of the user's VPSes for the current month
    let usage = VpsUsagePeriod::get_user_aggregate(db, user.id)
        .await
        .map_err(err)?;

    // Check if within plan limits
    let within_plan = usage.bandwidth_bytes <= plan.max_bandwidth_bytes
        && usage.cpu_used_ms <= plan.max_cpu_ms
        && usage.memory_used_mb_seconds <= plan.max_memory_mb_seconds;

    if within_plan {
        return Ok(());
    }

    // Over plan limits — check overage budget
    let overage_cost = plan.overage_cost_cents(&usage);
    let budget = OverageBudget::get_current(db, user.id).await.map_err(err)?;

    if overage_cost > budget.budget_cents {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "usage limit exceeded (overage budget exhausted)",
        ));
    }

    Ok(())
}

// ── CONNECT (HTTPS tunneling) ────────────────────────────────────────

async fn handle_connect(
    req: Request<Incoming>,
    db: PgPool,
    vps_id: Uuid,
) -> Result<ProxyResponse, BoxError> {
    let host = req.uri().authority().map(|a| a.as_str().to_owned());
    let host = match host {
        Some(h) => h,
        None => return Ok(error_response(StatusCode::BAD_REQUEST, "missing host")),
    };

    let target = match TcpStream::connect(&host).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(host = %host, error = %e, "CONNECT target unreachable");
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "target unreachable",
            ));
        }
    };

    // Spawn the tunnel task — it runs after we return the 200
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                if let Err(e) = tunnel(TokioIo::new(upgraded), target, db, vps_id).await {
                    tracing::debug!(error = %e, "tunnel error");
                }
            }
            Err(e) => tracing::debug!(error = %e, "upgrade error"),
        }
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Full::new(Bytes::new()))
        .unwrap())
}

async fn tunnel(
    client: TokioIo<hyper::upgrade::Upgraded>,
    target: TcpStream,
    db: PgPool,
    vps_id: Uuid,
) -> Result<(), BoxError> {
    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (mut target_read, mut target_write) = tokio::io::split(target);

    let bytes_in = Arc::new(AtomicI64::new(0));
    let bytes_out = Arc::new(AtomicI64::new(0));

    let bytes_out_clone = bytes_out.clone();
    let bytes_in_clone = bytes_in.clone();

    // client → target (bytes leaving the agent = egress = bytes_out)
    let mut client_to_target = tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        loop {
            let n = match client_read.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            bytes_out_clone.fetch_add(n as i64, Ordering::Relaxed);
            if target_write.write_all(&buf[..n]).await.is_err() {
                break;
            }
        }
    });

    // target → client (bytes entering the agent = ingress = bytes_in)
    let mut target_to_client = tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        loop {
            let n = match target_read.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            bytes_in_clone.fetch_add(n as i64, Ordering::Relaxed);
            if client_write.write_all(&buf[..n]).await.is_err() {
                break;
            }
        }
    });

    tokio::select! {
        _ = &mut client_to_target => { target_to_client.abort(); }
        _ = &mut target_to_client => { client_to_target.abort(); }
    }

    // Flush byte counts
    let total_in = bytes_in.load(Ordering::Relaxed);
    let total_out = bytes_out.load(Ordering::Relaxed);
    let total = total_in + total_out;
    if total > 0
        && let Err(e) = VpsUsagePeriod::add_bandwidth(&db, vps_id, total).await
    {
        tracing::error!(vps_id = %vps_id, error = %e, "failed to flush proxy byte counts");
    }

    Ok(())
}

// ── Plain HTTP forwarding ────────────────────────────────────────────

async fn handle_plain_http(
    req: Request<Incoming>,
    db: PgPool,
    vps_id: Uuid,
    http_client: reqwest::Client,
) -> Result<ProxyResponse, BoxError> {
    let method = req.method().clone();
    let uri = req.uri().to_string();

    // Collect request body
    let body_bytes = req.into_body().collect().await?.to_bytes();
    let bytes_out = body_bytes.len() as i64;

    // Forward request (strip Proxy-Authorization — reqwest doesn't carry it anyway)
    let reqwest_method =
        reqwest::Method::from_bytes(method.as_str().as_bytes()).map_err(|_| "invalid method")?;

    let resp = match http_client
        .request(reqwest_method, &uri)
        .body(body_bytes)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(uri = %uri, error = %e, "plain HTTP forward failed");
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "target unreachable",
            ));
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16())?;
    let resp_bytes = resp.bytes().await?;
    let bytes_in = resp_bytes.len() as i64;

    // Flush byte counts
    let total = bytes_in + bytes_out;
    if total > 0
        && let Err(e) = VpsUsagePeriod::add_bandwidth(&db, vps_id, total).await
    {
        tracing::error!(vps_id = %vps_id, error = %e, "failed to flush proxy byte counts");
    }

    Ok(Response::builder()
        .status(status)
        .body(Full::new(resp_bytes))
        .unwrap())
}

// ── Helpers ──────────────────────────────────────────────────────────

fn error_response(status: StatusCode, body: &str) -> ProxyResponse {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::from(body.to_owned())))
        .unwrap()
}
