use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Samples {
    pub name: String,
    pub unit: &'static str,
    pub samples: usize,
    pub median: f64,
    pub p99: f64,
    pub p999: f64,
    pub maximum: f64,
    pub over_5ms: usize,
    pub total_micros: u128,
}

impl Samples {
    pub fn micros(name: impl Into<String>, mut values: Vec<u64>) -> Self {
        values.sort_unstable();
        let percentile = |per_mille: usize| -> f64 {
            if values.is_empty() {
                return 0.0;
            }
            let index = (values.len() * per_mille)
                .div_ceil(1000)
                .saturating_sub(1)
                .min(values.len() - 1);
            values[index] as f64
        };
        Self {
            name: name.into(),
            unit: "microseconds",
            samples: values.len(),
            median: percentile(500),
            p99: percentile(990),
            p999: percentile(999),
            maximum: values.last().copied().unwrap_or(0) as f64,
            over_5ms: values.iter().filter(|&&value| value > 5000).count(),
            total_micros: values.iter().map(|&value| u128::from(value)).sum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nearest_rank_quantiles_and_tail_count_are_explicit() {
        let samples = Samples::micros("test", (1..=10_000).collect());
        assert_eq!(samples.median, 5000.0);
        assert_eq!(samples.p99, 9900.0);
        assert_eq!(samples.p999, 9990.0);
        assert_eq!(samples.over_5ms, 5000);
    }
}
