use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cb_db::models::{OverageBudget, Plan, User, Vps, VpsState, VpsUsagePeriod};
use cb_infra::ProviderRegistry;
use sqlx::PgPool;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Metrics collected for a single VPS.
pub struct VpsMetrics {
    pub storage_used_bytes: i64,
    pub cpu_used_ms: Option<i64>,
    pub memory_used_mb_seconds: Option<i64>,
}

/// Trait for collecting metrics from running VPS instances.
#[async_trait]
pub trait MetricsCollector: Send + Sync + 'static {
    async fn collect(&self, vps: &Vps) -> Result<VpsMetrics, BoxError>;
}

/// Stub collector that returns existing DB values (no-op).
pub struct StubCollector;

#[async_trait]
impl MetricsCollector for StubCollector {
    async fn collect(&self, vps: &Vps) -> Result<VpsMetrics, BoxError> {
        Ok(VpsMetrics {
            storage_used_bytes: vps.storage_used_bytes,
            cpu_used_ms: vps.cpu_used_ms,
            memory_used_mb_seconds: vps.memory_used_mb_seconds,
        })
    }
}

/// Spawn the background metrics monitor task.
pub fn spawn_monitor(
    pool: PgPool,
    collector: Arc<dyn MetricsCollector>,
    providers: ProviderRegistry,
    interval_secs: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            if let Err(e) = poll_metrics(&pool, &*collector).await {
                tracing::error!(error = %e, "metrics poll failed");
            }
            if let Err(e) = enforce_limits(&pool, &providers).await {
                tracing::error!(error = %e, "enforcement check failed");
            }
        }
    });
}

async fn poll_metrics(pool: &PgPool, collector: &dyn MetricsCollector) -> Result<(), BoxError> {
    let running = Vps::list_by_state(pool, VpsState::Running).await?;
    for vps in &running {
        let metering = cb_infra::metered_resources_for(&vps.provider);

        // Skip CPU/memory collection for bandwidth-only providers
        if !metering.cpu && !metering.memory {
            continue;
        }

        match collector.collect(vps).await {
            Ok(metrics) => {
                // Compute deltas (only positive — handles VPS restarts where new < old)
                let cpu_delta = if metering.cpu {
                    match (metrics.cpu_used_ms, vps.cpu_used_ms) {
                        (Some(new), Some(old)) if new > old => new - old,
                        _ => 0,
                    }
                } else {
                    0
                };
                let mem_delta = if metering.memory {
                    match (metrics.memory_used_mb_seconds, vps.memory_used_mb_seconds) {
                        (Some(new), Some(old)) if new > old => new - old,
                        _ => 0,
                    }
                } else {
                    0
                };

                // Upsert deltas into the period table
                if (cpu_delta > 0 || mem_delta > 0)
                    && let Err(e) =
                        VpsUsagePeriod::add_cpu_memory(pool, vps.id, cpu_delta, mem_delta).await
                {
                    tracing::error!(vps_id = %vps.id, error = %e, "failed to write period metrics");
                }

                // Update VPS row with absolute values + storage gauge
                if let Err(e) = Vps::update_usage(
                    pool,
                    vps.id,
                    metrics.storage_used_bytes,
                    metrics.cpu_used_ms,
                    metrics.memory_used_mb_seconds,
                )
                .await
                {
                    tracing::error!(vps_id = %vps.id, error = %e, "failed to write metrics");
                }
            }
            Err(e) => {
                tracing::error!(vps_id = %vps.id, error = %e, "failed to collect metrics");
            }
        }
    }
    Ok(())
}

/// Enforce usage limits by stopping VPSes when plan + overage budget are exhausted.
///
/// Currently only acts on Hetzner VPSes (fixed-allocation providers where stopping
/// the server saves money). Fly VPSes are gated per-request by the proxy.
async fn enforce_limits(pool: &PgPool, providers: &ProviderRegistry) -> Result<(), BoxError> {
    let running = Vps::list_by_state(pool, VpsState::Running).await?;

    // Collect distinct users who have running Hetzner VPSes
    let hetzner_users: HashSet<uuid::Uuid> = running
        .iter()
        .filter(|v| {
            v.provider.parse::<cb_infra::ProviderName>().ok()
                == Some(cb_infra::ProviderName::Hetzner)
        })
        .map(|v| v.user_id)
        .collect();

    for user_id in &hetzner_users {
        let user = match User::get_by_id(pool, *user_id).await {
            Ok(u) => u,
            Err(e) => {
                tracing::error!(user_id = %user_id, error = %e, "enforcement: failed to load user");
                continue;
            }
        };

        let plan_id = match user.plan_id {
            Some(id) => id,
            None => continue, // no plan = can't compute limits
        };

        let plan = match Plan::get_by_id(pool, plan_id).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(user_id = %user_id, error = %e, "enforcement: failed to load plan");
                continue;
            }
        };

        let usage = match VpsUsagePeriod::get_user_aggregate(pool, *user_id).await {
            Ok(u) => u,
            Err(e) => {
                tracing::error!(user_id = %user_id, error = %e, "enforcement: failed to load aggregate usage");
                continue;
            }
        };

        // Check if within plan limits
        let within_plan = usage.bandwidth_bytes <= plan.max_bandwidth_bytes
            && usage.cpu_used_ms <= plan.max_cpu_ms
            && usage.memory_used_mb_seconds <= plan.max_memory_mb_seconds;

        if within_plan {
            continue;
        }

        // Over plan limits — check overage budget
        let overage_cost = plan.overage_cost_cents(&usage);
        let budget = match OverageBudget::get_current(pool, *user_id).await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(user_id = %user_id, error = %e, "enforcement: failed to load overage budget");
                continue;
            }
        };

        if overage_cost <= budget.budget_cents {
            continue; // within overage allowance
        }

        // Budget exhausted — stop all of this user's running Hetzner VPSes
        let hetzner_provider = match providers.get(cb_infra::ProviderName::Hetzner) {
            Some(p) => p,
            None => {
                tracing::warn!("enforcement: Hetzner provider not available, skipping stop");
                continue;
            }
        };

        for vps in running.iter().filter(|v| {
            v.user_id == *user_id
                && v.state == VpsState::Running
                && v.provider.parse::<cb_infra::ProviderName>().ok()
                    == Some(cb_infra::ProviderName::Hetzner)
        }) {
            let vm_id = match &vps.provider_vm_id {
                Some(id) => cb_infra::types::VpsId(id.clone()),
                None => continue,
            };

            tracing::warn!(
                user_id = %user_id,
                vps_id = %vps.id,
                overage_cost_cents = overage_cost,
                budget_cents = budget.budget_cents,
                "enforcement: stopping Hetzner VPS (overage budget exhausted)"
            );

            if let Err(e) = hetzner_provider.stop_vps(&vm_id).await {
                tracing::error!(vps_id = %vps.id, error = %e, "enforcement: failed to stop VPS");
                continue;
            }

            if let Err(e) = Vps::set_state(pool, vps.id, VpsState::Stopped).await {
                tracing::error!(vps_id = %vps.id, error = %e, "enforcement: failed to update VPS state");
            }
        }
    }

    // Stub for future non-Hetzner enforcement
    for vps in running.iter().filter(|v| {
        v.provider.parse::<cb_infra::ProviderName>().ok() != Some(cb_infra::ProviderName::Hetzner)
            && v.provider.parse::<cb_infra::ProviderName>().ok()
                != Some(cb_infra::ProviderName::Fly)
    }) {
        tracing::debug!(
            vps_id = %vps.id,
            provider = %vps.provider,
            "enforcement: would apply to this provider in the future"
        );
    }

    Ok(())
}
