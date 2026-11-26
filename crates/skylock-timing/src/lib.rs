//! A fixed-size histogram of durations, for the percentiles of a task's work per frame.
//! Storage is allocated up front, so `record` never allocates.

/// Counts durations from 0 to `buckets * bucket_ns` in buckets of `bucket_ns`. Longer ones
/// count as overflow and still set the maximum.
pub struct DurationHistogram {
    bucket_ns: u64,
    counts: Box<[u32]>,
    overflow: u64,
    samples: u64,
    max_ns: u64,
}

/// Percentiles in µs. Each is the upper edge of the bucket that holds it, rounded up to a
/// whole µs, or the maximum if it falls past the last bucket. The maximum is rounded down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationSummary {
    pub samples: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

impl DurationHistogram {
    pub fn new(bucket_ns: u64, buckets: usize) -> Self {
        assert!(bucket_ns > 0 && buckets > 0);
        Self {
            bucket_ns,
            counts: vec![0; buckets].into_boxed_slice(),
            overflow: 0,
            samples: 0,
            max_ns: 0,
        }
    }

    pub fn record(&mut self, ns: u64) {
        self.samples += 1;
        self.max_ns = self.max_ns.max(ns);
        match usize::try_from(ns / self.bucket_ns) {
            Ok(index) if index < self.counts.len() => self.counts[index] += 1,
            _ => self.overflow += 1,
        }
    }

    pub fn summary(&self) -> Option<DurationSummary> {
        if self.samples == 0 {
            return None;
        }
        let percentile = |fraction: f64| {
            let rank = ((self.samples as f64 * fraction).ceil() as u64).max(1);
            let mut seen = 0;
            for (index, &count) in self.counts.iter().enumerate() {
                seen += u64::from(count);
                if seen >= rank {
                    return ((index as u64 + 1) * self.bucket_ns).div_ceil(1_000);
                }
            }
            self.max_ns / 1_000
        };
        Some(DurationSummary {
            samples: self.samples,
            p50_us: percentile(0.50),
            p95_us: percentile(0.95),
            p99_us: percentile(0.99),
            max_us: self.max_ns / 1_000,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const US: u64 = 1_000;
    const MS: u64 = 1_000_000;

    #[test]
    fn percentiles_come_from_the_buckets() {
        let mut timing = DurationHistogram::new(100 * US, 100);
        // 90 frames at 4.05 ms, 9 at 7.25 ms and one at 9.95 ms.
        (0..90).for_each(|_| timing.record(4_050 * US));
        (0..9).for_each(|_| timing.record(7_250 * US));
        timing.record(9_950 * US);
        let summary = timing.summary().unwrap();
        assert_eq!(
            summary,
            DurationSummary {
                samples: 100,
                p50_us: 4_100,
                p95_us: 7_300,
                p99_us: 7_300,
                max_us: 9_950,
            }
        );
    }

    #[test]
    fn sub_microsecond_buckets_round_up() {
        let mut timing = DurationHistogram::new(100, 100);
        (0..98).for_each(|_| timing.record(150));
        timing.record(1_850);
        timing.record(2_400);
        let summary = timing.summary().unwrap();
        // 0.2 µs, 1.9 µs and 2.5 µs bucket edges.
        assert_eq!((summary.p50_us, summary.p99_us, summary.max_us), (1, 2, 2));
        assert_eq!(DurationHistogram::new(100, 100).summary(), None);
    }

    #[test]
    fn overflow_reports_the_maximum() {
        let mut timing = DurationHistogram::new(100 * US, 10);
        timing.record(500 * US);
        timing.record(250 * MS);
        let summary = timing.summary().unwrap();
        assert_eq!((summary.p50_us, summary.p95_us), (600, 250_000));
        assert!(DurationHistogram::new(US, 1).summary().is_none());
    }
}
