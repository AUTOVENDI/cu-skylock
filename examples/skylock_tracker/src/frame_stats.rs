//! Frame-delivery statistics for the camera path.
//!
//! The camera source stamps `tov` when it pops a frame from its ring, so `tov` alone can't
//! show how long a frame waited. The buffer's PTS (GStreamer running time) can: `tov - PTS`
//! is a constant for the run plus that wait, so the lag above the run's smallest value is
//! the delay a frame picked up between decoding and the Copper loop. PTS steps also show
//! frames the ring dropped. Gaps over a threshold are kept with their time, to line them up
//! with other events. All storage is allocated up front, so `record` never allocates.

const NS_PER_MS: i64 = 1_000_000;
/// The lag histogram has 1 ms buckets and covers this many ms on each side of the first
/// frame's lag.
const LAG_SPAN_MS: i64 = 2_000;
const LAG_BUCKETS: usize = 2 * LAG_SPAN_MS as usize;
/// How many long gaps one period keeps; later ones are only counted.
pub const MAX_STALLS: usize = 16;

/// A gap between two frames at or above the stall threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stall {
    /// `tov` of the frame that ended the gap, in ns.
    pub tov_ns: u64,
    pub gap_ns: u64,
}

/// Counters for one reporting period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeriodStats {
    pub frames: u64,
    /// Longest wait between two frames' `tov`, in ns.
    pub max_gap_ns: u64,
    /// Smallest lag in the period relative to the first frame's lag, in ns. Across periods
    /// it shows drift between Copper's clock and GStreamer's, apart from ring waits.
    pub min_lag_ns: Option<i64>,
    stalls: [Stall; MAX_STALLS],
    stall_count: usize,
    /// Gaps over the threshold beyond the first `MAX_STALLS`.
    pub stalls_missed: u64,
}

impl PeriodStats {
    /// The period's first `MAX_STALLS` gaps over the stall threshold, in order.
    pub fn stalls(&self) -> &[Stall] {
        &self.stalls[..self.stall_count]
    }
}

/// Lag above the run's smallest lag, in ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LagSummary {
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
    pub max_ms: u64,
    /// Frames whose lag fell outside the histogram, for example after a PTS reset.
    pub out_of_range: u64,
}

/// Totals for the whole run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSummary {
    pub frames: u64,
    pub frames_without_pts: u64,
    /// Times the PTS went backwards, as it can when a stream restarts.
    pub pts_resets: u64,
    /// Last PTS minus first PTS, in ns, when the PTS never went backwards.
    pub pts_span_ns: Option<u64>,
    /// Smallest and largest step between consecutive PTS values, in ns.
    pub min_pts_step_ns: Option<u64>,
    pub max_pts_step_ns: u64,
    /// Longest wait between two frames' `tov`, in ns.
    pub max_gap_ns: u64,
    pub lag: Option<LagSummary>,
}

pub struct FrameStats {
    frames: u64,
    frames_without_pts: u64,
    pts_resets: u64,
    first_pts_ns: Option<u64>,
    last_pts_ns: Option<u64>,
    min_pts_step_ns: Option<u64>,
    max_pts_step_ns: u64,
    last_tov_ns: Option<u64>,
    max_gap_ns: u64,
    lag_reference_ns: Option<i64>,
    lag_histogram: Box<[u32]>,
    lag_out_of_range: u64,
    stall_threshold_ns: u64,
    period: PeriodStats,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameStats {
    pub fn new() -> Self {
        Self {
            frames: 0,
            frames_without_pts: 0,
            pts_resets: 0,
            first_pts_ns: None,
            last_pts_ns: None,
            min_pts_step_ns: None,
            max_pts_step_ns: 0,
            last_tov_ns: None,
            max_gap_ns: 0,
            lag_reference_ns: None,
            lag_histogram: vec![0; LAG_BUCKETS].into_boxed_slice(),
            lag_out_of_range: 0,
            stall_threshold_ns: 0,
            period: PeriodStats::default(),
        }
    }

    /// Keeps each gap of at least `threshold_ns` between frames in the period's stalls;
    /// 0 (the default) keeps none.
    pub fn set_stall_threshold_ns(&mut self, threshold_ns: u64) {
        self.stall_threshold_ns = threshold_ns;
    }

    /// Records one frame: its `tov` and, for a live buffer, its PTS.
    pub fn record(&mut self, tov_ns: u64, pts_ns: Option<u64>) {
        self.frames += 1;
        self.period.frames += 1;
        if let Some(last) = self.last_tov_ns {
            let gap = tov_ns.saturating_sub(last);
            self.max_gap_ns = self.max_gap_ns.max(gap);
            self.period.max_gap_ns = self.period.max_gap_ns.max(gap);
            if self.stall_threshold_ns > 0 && gap >= self.stall_threshold_ns {
                let period = &mut self.period;
                if period.stall_count < MAX_STALLS {
                    period.stalls[period.stall_count] = Stall {
                        tov_ns,
                        gap_ns: gap,
                    };
                    period.stall_count += 1;
                } else {
                    period.stalls_missed += 1;
                }
            }
        }
        self.last_tov_ns = Some(tov_ns);

        let Some(pts) = pts_ns else {
            self.frames_without_pts += 1;
            return;
        };
        match self.last_pts_ns {
            None => self.first_pts_ns = Some(pts),
            Some(last) if pts > last => {
                let step = pts - last;
                self.min_pts_step_ns = Some(self.min_pts_step_ns.map_or(step, |min| min.min(step)));
                self.max_pts_step_ns = self.max_pts_step_ns.max(step);
            }
            Some(_) => self.pts_resets += 1,
        }
        self.last_pts_ns = Some(pts);

        let lag = tov_ns as i64 - pts as i64;
        let relative = lag - *self.lag_reference_ns.get_or_insert(lag);
        self.period.min_lag_ns = Some(
            self.period
                .min_lag_ns
                .map_or(relative, |min| min.min(relative)),
        );
        let bucket = relative.div_euclid(NS_PER_MS) + LAG_SPAN_MS;
        match usize::try_from(bucket) {
            Ok(index) if index < LAG_BUCKETS => self.lag_histogram[index] += 1,
            _ => self.lag_out_of_range += 1,
        }
    }

    /// Returns the current period's counters and starts a new period.
    pub fn take_period(&mut self) -> PeriodStats {
        std::mem::take(&mut self.period)
    }

    /// Starts the run totals afresh, for example after a camera's start-up burst, keeping the
    /// current period's counters. Doesn't allocate.
    pub fn reset_run(&mut self) {
        self.frames = 0;
        self.frames_without_pts = 0;
        self.pts_resets = 0;
        self.first_pts_ns = None;
        self.last_pts_ns = None;
        self.min_pts_step_ns = None;
        self.max_pts_step_ns = 0;
        self.last_tov_ns = None;
        self.max_gap_ns = 0;
        self.lag_reference_ns = None;
        self.lag_histogram.fill(0);
        self.lag_out_of_range = 0;
    }

    pub fn summary(&self) -> RunSummary {
        let pts_span_ns = match (self.first_pts_ns, self.last_pts_ns) {
            (Some(first), Some(last)) if self.pts_resets == 0 => Some(last - first),
            _ => None,
        };
        RunSummary {
            frames: self.frames,
            frames_without_pts: self.frames_without_pts,
            pts_resets: self.pts_resets,
            pts_span_ns,
            min_pts_step_ns: self.min_pts_step_ns,
            max_pts_step_ns: self.max_pts_step_ns,
            max_gap_ns: self.max_gap_ns,
            lag: self.lag_summary(),
        }
    }

    fn lag_summary(&self) -> Option<LagSummary> {
        let counted: u64 = self.lag_histogram.iter().map(|&n| u64::from(n)).sum();
        let lowest = self.lag_histogram.iter().position(|&n| n > 0)?;
        let highest = self.lag_histogram.iter().rposition(|&n| n > 0)?;
        let percentile = |fraction: f64| {
            let rank = ((counted as f64 * fraction).ceil() as u64).max(1);
            let mut seen = 0;
            for (index, &n) in self.lag_histogram.iter().enumerate() {
                seen += u64::from(n);
                if seen >= rank {
                    return (index - lowest) as u64;
                }
            }
            (highest - lowest) as u64
        };
        Some(LagSummary {
            p50_ms: percentile(0.50),
            p95_ms: percentile(0.95),
            p99_ms: percentile(0.99),
            max_ms: (highest - lowest) as u64,
            out_of_range: self.lag_out_of_range,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn steady_stream_has_no_extra_lag() {
        let mut stats = FrameStats::new();
        for i in 0..100 {
            // 25 fps; each frame pops 30 ms after its PTS.
            stats.record(5_000 * MS + i * 40 * MS + 30 * MS, Some(i * 40 * MS));
        }
        let summary = stats.summary();
        assert_eq!(summary.frames, 100);
        assert_eq!(summary.pts_span_ns, Some(99 * 40 * MS));
        assert_eq!(summary.min_pts_step_ns, Some(40 * MS));
        assert_eq!(summary.max_gap_ns, 40 * MS);
        let lag = summary.lag.unwrap();
        assert_eq!((lag.p50_ms, lag.max_ms, lag.out_of_range), (0, 0, 0));
    }

    #[test]
    fn waiting_frames_and_dropped_frames_show_up() {
        let mut stats = FrameStats::new();
        // The first 10 frames pop at once; the next 10 wait 7 frame periods (280 ms) and
        // every other frame is dropped, as when a slow loop drains a full ring.
        for i in 0..10 {
            stats.record(1_000 * MS + i * 40 * MS, Some(i * 40 * MS));
        }
        for i in 10..20 {
            let pts = (10 + 2 * (i - 10)) * 40 * MS;
            stats.record(1_000 * MS + pts + 280 * MS, Some(pts));
        }
        let summary = stats.summary();
        assert_eq!(summary.min_pts_step_ns, Some(40 * MS));
        assert_eq!(summary.max_pts_step_ns, 80 * MS);
        let lag = summary.lag.unwrap();
        assert_eq!(lag.p50_ms, 0);
        assert_eq!(lag.p95_ms, 280);
        assert_eq!(lag.max_ms, 280);
    }

    #[test]
    fn periods_reset_and_pts_resets_are_counted() {
        let mut stats = FrameStats::new();
        stats.record(100 * MS, Some(10 * MS));
        stats.record(150 * MS, Some(60 * MS));
        assert_eq!(
            stats.take_period(),
            PeriodStats {
                frames: 2,
                max_gap_ns: 50 * MS,
                min_lag_ns: Some(0),
                ..PeriodStats::default()
            }
        );
        // The stream restarts: PTS goes back to zero after a 10 s outage.
        stats.record(10_150 * MS, Some(0));
        stats.record(10_190 * MS, None);
        assert_eq!(stats.take_period().max_gap_ns, 10_000 * MS);
        let summary = stats.summary();
        assert_eq!((summary.pts_resets, summary.frames_without_pts), (1, 1));
        assert_eq!(summary.pts_span_ns, None);
        assert_eq!(summary.lag.unwrap().out_of_range, 1);
    }

    #[test]
    fn reset_run_drops_a_start_up_burst() {
        let mut stats = FrameStats::new();
        // A burst of stale frames whose PTS goes back and forth, then a steady stream.
        for (i, pts) in [500, 300, 700, 400].into_iter().enumerate() {
            stats.record(1_000 * MS + i as u64 * 5 * MS, Some(pts * MS));
        }
        stats.reset_run();
        for i in 0..10 {
            stats.record(2_000 * MS + i * 33 * MS, Some(1_000 * MS + i * 33 * MS));
        }
        let summary = stats.summary();
        assert_eq!((summary.frames, summary.pts_resets), (10, 0));
        assert_eq!(summary.pts_span_ns, Some(9 * 33 * MS));
        assert_eq!(summary.lag.unwrap().max_ms, 0);
        // The period counters survive the reset.
        assert_eq!(stats.take_period().frames, 14);
    }

    #[test]
    fn long_gaps_are_kept_with_their_time() {
        let mut stats = FrameStats::new();
        stats.set_stall_threshold_ns(100 * MS);
        // 30 fps with a 250 ms gap, then 20 gaps of exactly 100 ms.
        let mut tov = 1_000 * MS;
        for _ in 0..5 {
            stats.record(tov, None);
            tov += 33 * MS;
        }
        tov += 217 * MS;
        stats.record(tov, None);
        for _ in 0..20 {
            tov += 100 * MS;
            stats.record(tov, None);
        }
        let period = stats.take_period();
        assert_eq!(
            period.stalls()[0],
            Stall {
                tov_ns: 1_382 * MS,
                gap_ns: 250 * MS
            }
        );
        assert_eq!(period.stalls().len(), MAX_STALLS);
        assert_eq!(period.stalls_missed, 21 - MAX_STALLS as u64);
        assert!(stats.take_period().stalls().is_empty());
    }
}
