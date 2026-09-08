use crate::AppState;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::MissedTickBehavior;

pub(crate) const SECRET_CACHE_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(5 * 60);
pub(crate) const SESSION_RETIREMENT_BATCH_SIZE: usize = 4_096;

pub(crate) async fn run(app_state: Arc<AppState>) {
    let first_tick = tokio::time::Instant::now() + SECRET_CACHE_MAINTENANCE_INTERVAL;
    let mut interval = tokio::time::interval_at(first_tick, SECRET_CACHE_MAINTENANCE_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        interval.tick().await;
        purge_expired_secret_state(&app_state, Instant::now()).await;
    }
}

async fn purge_expired_secret_state(app_state: &AppState, now: Instant) {
    // Never overlap the two cache locks. Pending key bytes are wiped in place;
    // their destructors and deallocation run after releasing the write lock.
    // Session values stay in stable Arc allocations and are both zeroized and
    // deallocated after releasing their mutex.
    let retired_pending = {
        let mut pending = app_state.ephemeral_keys.write().await;
        pending.retire_expired_at(now)
    };
    let pending_count = retired_pending.removed_count();
    drop(retired_pending);

    // Snapshot the ordered expired suffix once. Repeatedly rescanning it for
    // each batch would become quadratic when expired leased entries remain.
    let candidates = {
        let sessions = app_state.session_states.lock().await;
        sessions.expired_unleased_keys_at(now)
    };

    let mut session_count = 0;
    let mut batches = candidates.chunks(SESSION_RETIREMENT_BATCH_SIZE).peekable();
    while let Some(batch) = batches.next() {
        let retired_sessions = {
            let mut sessions = app_state.session_states.lock().await;
            sessions.retire_expired_candidates_at(batch, now)
        };
        session_count += retired_sessions.removed_count();
        drop(retired_sessions);

        // Give waiting requests a scheduling point between bounded lock holds.
        if batches.peek().is_some() {
            tokio::task::yield_now().await;
        }
    }

    if pending_count != 0 || session_count != 0 {
        tracing::debug!(
            pending_attestations = pending_count,
            encryption_sessions = session_count,
            "Purged expired secret state"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_policy_is_five_minutes_with_bounded_session_batches() {
        assert_eq!(
            SECRET_CACHE_MAINTENANCE_INTERVAL,
            Duration::from_secs(5 * 60)
        );
        assert_eq!(SESSION_RETIREMENT_BATCH_SIZE, 4_096);
    }
}
