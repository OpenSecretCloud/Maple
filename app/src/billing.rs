//! Sidebar plan card data derived from the Maple billing status. The HTTP
//! client lives in the `maple-billing` crate.

// This module is the desktop frontend's boundary. A headless build (no
// `desktop` feature) uses only a few entry points, so the rest is unused
// there by design.
#![cfg_attr(not(feature = "desktop"), allow(dead_code))]

pub use maple_billing::{BillingClient, BillingError, BillingStatus};

/// Billing API base URL. `MAPLE_BILLING_API_URL` overrides the default.
pub fn configured_billing_api_url() -> String {
    std::env::var("MAPLE_BILLING_API_URL")
        .ok()
        .map(|url| url.trim().trim_end_matches('/').to_string())
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| maple_billing::DEFAULT_BILLING_API_URL.to_string())
}

/// Plan quota shown in the sidebar footer: plan name, percent of the
/// quota used in the current period, and the date the period resets.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanUsage {
    pub plan_label: String,
    pub percent_used: u8,
    pub resets_label: String,
}

impl PlanUsage {
    /// Build the card data the same way the Maple web sidebar does. Returns
    /// `None` when the status has no usage meter.
    pub fn from_status(
        status: &BillingStatus,
        now: chrono::DateTime<chrono::Local>,
    ) -> Option<Self> {
        let total = status.total_tokens.filter(|total| *total > 0)?;
        let used = status.used_tokens?.max(0);
        let percent = ((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0);
        Some(Self {
            plan_label: plan_name_label(status.product_name.as_deref()),
            percent_used: percent.round() as u8,
            resets_label: reset_label(status.usage_reset_date.as_deref(), now),
        })
    }
}

fn plan_name_label(raw: Option<&str>) -> String {
    let cleaned = raw
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Pro");
    let has_plan_suffix = cleaned
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case("plan"));
    if has_plan_suffix {
        cleaned.to_string()
    } else {
        format!("{cleaned} Plan")
    }
}

/// Text after "Resets" in the card, matching `formatResetDate` in the web app.
fn reset_label(iso: Option<&str>, now: chrono::DateTime<chrono::Local>) -> String {
    let Some(reset) = iso
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&chrono::Local))
    else {
        return "Monthly".to_string();
    };
    let remaining = reset - now;
    if remaining < chrono::Duration::zero() {
        return "Monthly".to_string();
    }
    let days = (reset.date_naive() - now.date_naive()).num_days();
    match days {
        0 => match remaining.num_hours() {
            0 => "in less than 1 hour".to_string(),
            1 => "in 1 hour".to_string(),
            hours => format!("in {hours} hours"),
        },
        1 => "Tomorrow".to_string(),
        2..=7 => format!("in {days} days"),
        _ => reset.format("%b %-d").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> chrono::DateTime<chrono::Local> {
        chrono::Local
            .with_ymd_and_hms(2026, 8, 26, 12, 0, 0)
            .unwrap()
    }

    fn status(product: &str, used: i64, total: i64, reset: &str) -> BillingStatus {
        BillingStatus {
            product_name: Some(product.to_string()),
            total_tokens: Some(total),
            used_tokens: Some(used),
            usage_reset_date: Some(reset.to_string()),
        }
    }

    #[test]
    fn builds_card_from_status() {
        let reset = (now() + chrono::Duration::days(11)).to_rfc3339();
        let plan = PlanUsage::from_status(&status("Max", 17, 100, &reset), now()).unwrap();
        assert_eq!(plan.plan_label, "Max Plan");
        assert_eq!(plan.percent_used, 17);
        assert_eq!(plan.resets_label, "Sep 6");
    }

    #[test]
    fn keeps_existing_plan_suffix_and_clamps() {
        let plan =
            PlanUsage::from_status(&status("Pro Plan", 500, 100, "2026-08-28T12:00:00Z"), now())
                .unwrap();
        assert_eq!(plan.plan_label, "Pro Plan");
        assert_eq!(plan.percent_used, 100);
        assert_eq!(plan.resets_label, "in 2 days");
    }

    #[test]
    fn relative_reset_labels() {
        let label =
            |reset: chrono::DateTime<chrono::Local>| reset_label(Some(&reset.to_rfc3339()), now());
        assert_eq!(
            label(now() + chrono::Duration::minutes(30)),
            "in less than 1 hour"
        );
        assert_eq!(label(now() + chrono::Duration::hours(1)), "in 1 hour");
        assert_eq!(label(now() + chrono::Duration::hours(3)), "in 3 hours");
        assert_eq!(label(now() + chrono::Duration::days(1)), "Tomorrow");
        assert_eq!(label(now() + chrono::Duration::days(5)), "in 5 days");
        assert_eq!(label(now() + chrono::Duration::days(20)), "Sep 15");
        assert_eq!(label(now() - chrono::Duration::days(1)), "Monthly");
        assert_eq!(reset_label(None, now()), "Monthly");
        assert_eq!(reset_label(Some("garbage"), now()), "Monthly");
    }

    #[test]
    fn no_meter_without_totals() {
        let mut s = status("Max", 1, 0, "2026-09-06T00:00:00Z");
        assert!(PlanUsage::from_status(&s, now()).is_none());
        s.total_tokens = Some(10);
        s.used_tokens = None;
        assert!(PlanUsage::from_status(&s, now()).is_none());
    }
}
