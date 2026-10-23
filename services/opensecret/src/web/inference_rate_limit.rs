//! Best-effort, process-local admission for authenticated inference requests.
//! One account shares a rolling window across all mounted inference surfaces.
use crate::{models::users::User, ApiError};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

const WINDOW: Duration = Duration::from_secs(60);
const MAX_ACCOUNTS: usize = 10_000;
const DEFAULT_RPM: usize = 100;
const DEFAULT_COOLDOWN_SECONDS: usize = 60;

struct AccountWindow {
    admitted: VecDeque<Instant>,
    blocked_until: Option<Instant>,
    retain_until: Instant,
}

pub(crate) struct InferenceRateLimiter {
    rpm: usize,
    cooldown: Duration,
    max_accounts: usize,
    accounts: Mutex<HashMap<Uuid, AccountWindow>>,
}

impl Default for InferenceRateLimiter {
    fn default() -> Self {
        Self::new(
            DEFAULT_RPM,
            Duration::from_secs(DEFAULT_COOLDOWN_SECONDS as u64),
        )
    }
}

impl InferenceRateLimiter {
    fn new(rpm: usize, cooldown: Duration) -> Self {
        Self {
            rpm,
            cooldown,
            max_accounts: MAX_ACCOUNTS,
            accounts: Mutex::default(),
        }
    }

    pub(crate) fn from_env() -> Result<Self, String> {
        let rpm = setting("INFERENCE_RATE_LIMIT_RPM", DEFAULT_RPM, 1_000)?;
        let cooldown = setting(
            "INFERENCE_RATE_LIMIT_COOLDOWN_SECONDS",
            DEFAULT_COOLDOWN_SECONDS,
            3_600,
        )?;
        Ok(Self::new(rpm, Duration::from_secs(cooldown as u64)))
    }

    pub(crate) fn check(&self, account: Uuid) -> Result<(), ApiError> {
        let mut accounts = self
            .accounts
            .lock()
            .map_err(|_| ApiError::InternalServerError)?;
        // Sample time under the lock so concurrent admissions stay ordered.
        self.admit(&mut accounts, account, Instant::now())
    }

    fn admit(
        &self,
        accounts: &mut HashMap<Uuid, AccountWindow>,
        account: Uuid,
        now: Instant,
    ) -> Result<(), ApiError> {
        if !accounts.contains_key(&account) && accounts.len() >= self.max_accounts {
            // Reclaim only expired state. Evicting a live bucket would let new
            // accounts reset another account's limit or active cooldown.
            accounts.retain(|_, window| window.retain_until > now);
            if accounts.len() >= self.max_accounts {
                return Err(ApiError::ServiceUnavailable);
            }
        }
        let window = accounts.entry(account).or_insert_with(|| AccountWindow {
            admitted: VecDeque::new(),
            blocked_until: None,
            retain_until: now,
        });
        if let Some(until) = window.blocked_until {
            if until > now {
                return Err(rate_limited(until.duration_since(now)));
            }
            window.blocked_until = None;
        }
        while window
            .admitted
            .front()
            .is_some_and(|time| now.duration_since(*time) >= WINDOW)
        {
            window.admitted.pop_front();
        }
        if window.admitted.len() >= self.rpm {
            let until = now + self.cooldown;
            window.blocked_until = Some(until);
            window.retain_until = window.retain_until.max(until);
            return Err(rate_limited(self.cooldown));
        }
        window.admitted.push_back(now);
        window.retain_until = now + WINDOW;
        Ok(())
    }
}

fn setting(name: &str, default: usize, maximum: usize) -> Result<usize, String> {
    match std::env::var(name) {
        Ok(value) => parse_setting(&value, maximum)
            .ok_or_else(|| format!("{name} must be an integer between 1 and {maximum}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(_) => Err(format!("{name} must be valid Unicode")),
    }
}

fn parse_setting(value: &str, maximum: usize) -> Option<usize> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| (1..=maximum).contains(value))
}

fn rate_limited(remaining: Duration) -> ApiError {
    // Integer Retry-After must never tell the caller to retry before expiry.
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0);
    ApiError::InferenceRateLimited {
        retry_after: Duration::from_secs(seconds.max(1)),
    }
}

/// Mounted only on inference POSTs, inside authentication and transport/body
/// validation, before handlers can persist Responses or contact a provider.
pub(crate) async fn enforce(
    State(limiter): State<Arc<InferenceRateLimiter>>,
    Extension(user): Extension<User>,
    request: Request,
    next: Next,
) -> Response {
    match limiter.check(user.uuid) {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_at(
        limiter: &InferenceRateLimiter,
        account: Uuid,
        now: Instant,
    ) -> Result<(), ApiError> {
        limiter.admit(&mut limiter.accounts.lock().unwrap(), account, now)
    }

    fn retry_seconds(result: Result<(), ApiError>) -> u64 {
        match result {
            Err(ApiError::InferenceRateLimited { retry_after }) => retry_after.as_secs(),
            other => panic!("expected rate limit, got {other:?}"),
        }
    }

    #[test]
    fn default_allows_100_then_cooldown_is_fixed_and_rounded_up() {
        let limiter = InferenceRateLimiter::default();
        let account = Uuid::new_v4();
        let now = Instant::now();
        for _ in 0..100 {
            check_at(&limiter, account, now).unwrap();
        }
        assert_eq!(retry_seconds(check_at(&limiter, account, now)), 60);
        assert_eq!(
            retry_seconds(check_at(
                &limiter,
                account,
                now + Duration::from_millis(500)
            )),
            60
        );
        assert_eq!(
            retry_seconds(check_at(&limiter, account, now + Duration::from_secs(59))),
            1
        );
        // Rejected requests did not extend the original deadline.
        check_at(&limiter, account, now + WINDOW).unwrap();
    }

    #[test]
    fn rolling_window_does_not_reset_at_a_calendar_boundary() {
        let limiter = InferenceRateLimiter::new(2, WINDOW);
        let account = Uuid::new_v4();
        let now = Instant::now();
        check_at(&limiter, account, now).unwrap();
        check_at(&limiter, account, now + Duration::from_secs(59)).unwrap();
        // Exactly 60 seconds removes just the first admission.
        check_at(&limiter, account, now + WINDOW).unwrap();
        assert_eq!(retry_seconds(check_at(&limiter, account, now + WINDOW)), 60);
    }

    #[test]
    fn accounts_and_enclave_instances_are_independent() {
        let limiter = InferenceRateLimiter::new(1, WINDOW);
        let account = Uuid::new_v4();
        limiter.check(account).unwrap();
        assert_eq!(retry_seconds(limiter.check(account)), 60);
        limiter.check(Uuid::new_v4()).unwrap();
        InferenceRateLimiter::default().check(account).unwrap();
    }

    #[test]
    fn capacity_reclaims_expired_state_without_evicting_live_cooldowns() {
        let mut limiter = InferenceRateLimiter::new(1, WINDOW);
        limiter.max_accounts = 1;
        let account = Uuid::new_v4();
        let another = Uuid::new_v4();
        let now = Instant::now();
        check_at(&limiter, account, now).unwrap();
        assert_eq!(
            retry_seconds(check_at(&limiter, account, now + Duration::from_secs(30))),
            60
        );
        assert!(matches!(
            check_at(&limiter, another, now + WINDOW),
            Err(ApiError::ServiceUnavailable)
        ));
        assert_eq!(retry_seconds(check_at(&limiter, account, now + WINDOW)), 30);
        check_at(&limiter, another, now + Duration::from_secs(90)).unwrap();
        assert_eq!(limiter.accounts.lock().unwrap().len(), 1);
    }

    #[test]
    fn concurrent_requests_cannot_over_admit() {
        let limiter = Arc::new(InferenceRateLimiter::default());
        let account = Uuid::new_v4();
        let barrier = Arc::new(std::sync::Barrier::new(16));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let limiter = limiter.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    (0..10).filter(|_| limiter.check(account).is_ok()).count()
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .sum::<usize>(),
            100
        );
        assert_eq!(
            limiter.accounts.lock().unwrap()[&account].admitted.len(),
            100
        );
    }

    #[test]
    fn invalid_configuration_cannot_disable_or_unbound_the_limiter() {
        for invalid in ["", "0", "-1", "1001", "garbage", "999999999999999999999999"] {
            assert_eq!(parse_setting(invalid, 1000), None);
        }
        assert_eq!(parse_setting("100", 1000), Some(100));
        assert_eq!(parse_setting("1000", 1000), Some(1000));
    }
}
