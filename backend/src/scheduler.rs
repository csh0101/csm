use chrono::Utc;
use tokio::time::{Duration as TokioDuration, interval};

use crate::{
    api::{IncrementalSummaryRequest, next_run_after, run_incremental_summary},
    models::{AnalysisCycle, SubscriptionStatus},
    state::SharedState,
    storage,
};

const SCHEDULER_TICK_SECONDS: u64 = 30;

pub async fn run(state: SharedState) {
    initialize_missing_next_runs(&state).await;

    let mut ticker = interval(TokioDuration::from_secs(SCHEDULER_TICK_SECONDS));
    loop {
        ticker.tick().await;

        let Some(subscription_id) = due_subscription_id(&state).await else {
            continue;
        };

        tracing::info!("running scheduled incremental summary for {subscription_id}");
        if let Err(error) = run_incremental_summary(
            state.clone(),
            IncrementalSummaryRequest {
                subscription_id: subscription_id.clone(),
                peer_access_token: None,
                since: None,
                language: None,
            },
        )
        .await
        {
            tracing::warn!("scheduled incremental summary failed for {subscription_id}: {error}");
        }
    }
}

async fn initialize_missing_next_runs(state: &SharedState) {
    let now = Utc::now();
    let mut inner = state.inner.write().await;
    let mut collaboration = inner.collaboration.clone();
    let mut changed = false;

    for subscription in &mut collaboration.subscriptions {
        if subscription.status != SubscriptionStatus::Active
            || subscription.analysis_cycle == AnalysisCycle::Manual
            || subscription.next_run_at.is_some()
        {
            continue;
        }

        subscription.next_run_at = next_run_after(now, &subscription.analysis_cycle);
        changed = true;
    }

    if !changed {
        return;
    }

    if let Err(error) =
        storage::save_collaboration_store(&state.config.collaboration_path, &collaboration)
    {
        tracing::warn!("failed to persist initialized scheduled next run times: {error}");
        return;
    }
    inner.collaboration = collaboration;
}

async fn due_subscription_id(state: &SharedState) -> Option<String> {
    let now = Utc::now();
    let inner = state.inner.read().await;
    inner
        .collaboration
        .subscriptions
        .iter()
        .filter(|subscription| {
            subscription.status == SubscriptionStatus::Active
                && subscription.analysis_cycle != AnalysisCycle::Manual
                && subscription
                    .next_run_at
                    .is_some_and(|next_run_at| next_run_at <= now)
        })
        .min_by_key(|subscription| subscription.next_run_at)
        .map(|subscription| subscription.subscription_id.clone())
}
