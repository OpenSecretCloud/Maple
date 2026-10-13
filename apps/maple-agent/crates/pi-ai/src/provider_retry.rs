//! Retries of a request whose response did not start, as Pi's `retryProviderRequest`
//! makes them after the OpenAI SDK's policy: which failures are sent again, and how long
//! to wait first.

use std::hash::{BuildHasher, Hasher, RandomState};
use std::time::Duration;

use crate::types::now_ms;

/// The longest wait a server may ask for when the request sets no limit.
const DEFAULT_MAX_RETRY_DELAY_MS: u64 = 60_000;

/// A request that failed before its response started.
#[derive(Debug)]
pub(crate) struct RequestFailure {
    /// `None` when no response came: the connection failed or timed out.
    pub status: Option<u16>,
    pub headers: Vec<(String, String)>,
    /// What the response fails with when the request is not sent again.
    pub message: String,
    /// The error as the OpenAI SDK states it, which a refused retry quotes.
    pub sdk_message: String,
}

impl RequestFailure {
    /// A failure without a response.
    pub fn without_response(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            status: None,
            headers: Vec::new(),
            sdk_message: message.clone(),
            message,
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Whether to send the request again: as the server's `x-should-retry` says, or else
    /// when no response came, or it was a timeout, a conflict, a rate limit or a server
    /// error.
    pub fn is_retryable(&self) -> bool {
        match self.header("x-should-retry") {
            Some("true") => return true,
            Some("false") => return false,
            _ => {}
        }
        match self.status {
            None => true,
            Some(status) => matches!(status, 408 | 409 | 429) || status >= 500,
        }
    }

    /// How long to wait before retry `retry_index`, counted from zero: what the server
    /// asks in `retry-after-ms` or `retry-after`, or else half a second doubling up to
    /// eight, less up to a quarter. When the server asks for longer than `max_delay_ms`,
    /// the request fails with this error instead.
    pub fn retry_delay(
        &self,
        retry_index: u32,
        max_delay_ms: Option<u64>,
    ) -> Result<Duration, String> {
        if let Some(delay) = self.header("retry-after-ms").and_then(parse_float)
            && delay.is_finite()
        {
            return self.server_delay(delay, max_delay_ms);
        }
        if let Some(value) = self.header("retry-after") {
            let delay = match parse_float(value) {
                Some(seconds) => Some(seconds * 1000.0),
                None => parse_http_date(value).map(|at| at - now_ms() as f64),
            };
            if let Some(delay) = delay.filter(|delay| delay.is_finite()) {
                return self.server_delay(delay, max_delay_ms);
            }
        }
        let exponential = (0.5 * 2f64.powi(retry_index.min(16) as i32)).min(8.0) * 1000.0;
        Ok(millis(exponential * (1.0 - random_fraction() * 0.25)))
    }

    fn server_delay(&self, delay_ms: f64, max_delay_ms: Option<u64>) -> Result<Duration, String> {
        let max = max_delay_ms.unwrap_or(DEFAULT_MAX_RETRY_DELAY_MS);
        if max > 0 && delay_ms > max as f64 {
            return Err(format!(
                "Server requested {}s retry delay (max: {}s). {}",
                (delay_ms / 1000.0).ceil(),
                (max as f64 / 1000.0).ceil(),
                self.sdk_message
            ));
        }
        Ok(millis(delay_ms))
    }
}

fn millis(ms: f64) -> Duration {
    Duration::try_from_secs_f64(ms.max(0.0) / 1000.0).unwrap_or(Duration::MAX)
}

/// A number in [0, 1).
fn random_fraction() -> f64 {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_i64(now_ms());
    (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// The number `text` starts with, as JavaScript's `parseFloat` reads it.
fn parse_float(text: &str) -> Option<f64> {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let sign = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    if text[sign..].starts_with("Infinity") {
        return Some(if bytes[0] == b'-' {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    let digits = |from: usize| {
        from + bytes[from..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let mut end = digits(sign);
    let mut has_digits = end > sign;
    if bytes.get(end) == Some(&b'.') {
        let fraction_end = digits(end + 1);
        has_digits |= fraction_end > end + 1;
        end = fraction_end;
    }
    if !has_digits {
        return None;
    }
    // An exponent counts only with digits after it.
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let exponent_sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent_end = digits(end + 1 + exponent_sign);
        if exponent_end > end + 1 + exponent_sign {
            end = exponent_end;
        }
    }
    text[..end].trim_end_matches('.').parse().ok()
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The time an HTTP date names, in milliseconds since the Unix epoch. Only the format
/// servers must send is read: `Sun, 06 Nov 1994 08:49:37 GMT`.
fn parse_http_date(text: &str) -> Option<f64> {
    let (_, rest) = text.trim().split_once(", ")?;
    let mut parts = rest.split(' ');
    let day: i64 = parts.next()?.parse().ok()?;
    let month_name = parts.next()?;
    let month = MONTHS.iter().position(|name| *name == month_name)? as i64 + 1;
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next()?.parse().ok()?;
    if parts.next()? != "GMT"
        || parts.next().is_some()
        || clock.next().is_some()
        || !(1..=31).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..61).contains(&second)
    {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(seconds as f64 * 1000.0)
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(status: Option<u16>, headers: &[(&str, &str)]) -> RequestFailure {
        RequestFailure {
            status,
            headers: headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            message: "429: {\"message\":\"Too many requests\",\"code\":429}".into(),
            sdk_message: "429 Too many requests".into(),
        }
    }

    #[test]
    fn retries_follow_the_sdks_policy() {
        assert!(failure(None, &[]).is_retryable());
        for status in [408, 409, 429, 500, 503] {
            assert!(failure(Some(status), &[]).is_retryable(), "{status}");
        }
        for status in [400, 401, 403, 404, 413, 422] {
            assert!(!failure(Some(status), &[]).is_retryable(), "{status}");
        }
        assert!(failure(Some(400), &[("X-Should-Retry", "true")]).is_retryable());
        assert!(!failure(Some(503), &[("x-should-retry", "false")]).is_retryable());
    }

    #[test]
    fn the_server_says_how_long_to_wait() {
        let delay = |headers: &[(&str, &str)]| failure(Some(429), headers).retry_delay(0, None);
        assert_eq!(
            delay(&[("retry-after-ms", "1500")]),
            Ok(Duration::from_millis(1500))
        );
        assert_eq!(delay(&[("Retry-After", "2")]), Ok(Duration::from_secs(2)));
        // `retry-after-ms` comes first; text that is no number or date is passed over.
        assert_eq!(
            delay(&[("retry-after", "9"), ("retry-after-ms", "250")]),
            Ok(Duration::from_millis(250))
        );
        let backoff = delay(&[("retry-after", "soon")]).unwrap();
        assert!(backoff <= Duration::from_millis(500) && backoff >= Duration::from_millis(375));
        // A date in the past is no wait.
        assert_eq!(
            delay(&[("retry-after", "Sun, 06 Nov 1994 08:49:37 GMT")]),
            Ok(Duration::ZERO)
        );
    }

    #[test]
    fn a_wait_longer_than_the_limit_fails_the_request() {
        let failure = failure(Some(429), &[("retry-after", "120")]);
        assert_eq!(
            failure.retry_delay(0, None),
            Err("Server requested 120s retry delay (max: 60s). 429 Too many requests".into())
        );
        assert_eq!(
            failure.retry_delay(0, Some(0)),
            Ok(Duration::from_secs(120))
        );
        assert!(failure.retry_delay(0, Some(200_000)).is_ok());
        assert!(failure.retry_delay(0, Some(90_500)).is_err_and(|error| {
            error.starts_with("Server requested 120s retry delay (max: 91s).")
        }));
    }

    #[test]
    fn backoff_doubles_up_to_eight_seconds() {
        let failure = failure(None, &[]);
        for (index, ceiling) in [(0, 500), (1, 1_000), (3, 4_000), (4, 8_000), (9, 8_000)] {
            let delay = failure.retry_delay(index, None).unwrap();
            let ceiling = Duration::from_millis(ceiling);
            assert!(
                delay <= ceiling && delay >= ceiling * 3 / 4,
                "{index}: {delay:?}"
            );
        }
    }

    #[test]
    fn numbers_and_dates_read_as_javascript_reads_them() {
        assert_eq!(parse_float(" 1.5s"), Some(1.5));
        assert_eq!(parse_float("2e3"), Some(2000.0));
        assert_eq!(parse_float("2e"), Some(2.0));
        assert_eq!(parse_float("5."), Some(5.0));
        assert_eq!(parse_float(".5"), Some(0.5));
        assert_eq!(parse_float("-Infinity"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_float("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        assert_eq!(parse_float("."), None);
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0.0));
        assert_eq!(
            parse_http_date("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(1_445_412_480_000.0)
        );
        assert_eq!(parse_http_date("Wed, 21 Oct 2015 07:28:00 UTC"), None);
        assert_eq!(parse_http_date("21 Oct 2015"), None);
    }
}
