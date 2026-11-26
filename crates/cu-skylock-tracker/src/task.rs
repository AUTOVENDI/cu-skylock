//! The tracker task: `Detections` in, `TrackState` out, for one target. The tracking is
//! [`crate::tracker`]'s; this task reads the config, hands each frame's detections over with
//! the frame's `tov`, copies that `tov` onto the output and logs what changed.
//!
//! Config, all optional:
//! - `confirm_hits` (default 3): detections in consecutive frames that make a candidate the
//!   target.
//! - `gate_chi2` (default 9.21, 99 % for 2 degrees of freedom): the largest squared
//!   Mahalanobis distance between a detection's centre and the predicted one that the target
//!   takes.
//! - `max_coast_ms` (default 1000, at least 1): how long the target may go without a detection
//!   before it is lost. It also bounds the time between a candidate's detections, so it must
//!   be longer than the frame interval: with a shorter one no candidate is ever confirmed.
//! - `lead_ms` (default 0): how far ahead of the frame the lead point is.
//! - `meas_std_px` (default 5): the standard deviation of a detection's centre, in px.
//! - `accel_psd` (default 20000): the power spectral density of the target's white-noise
//!   acceleration, in px²/s³. Over `T` seconds the velocity's variance grows by
//!   `accel_psd · T`, in (px/s)².
//! - `init_vel_std` (default 300): the standard deviation of a new candidate's velocity, in
//!   px/s.
//!
//! A frame without detections still moves the track on, as a coast; a cycle without a frame
//! outputs nothing. A frame whose `tov` isn't a single time (`Tov::Time`), or isn't later than
//! the last frame taken, is dropped and outputs nothing. The `tov` is taken as the time the
//! frame shows, so frames that arrive late, such as a camera's start-up burst of old frames,
//! are tracked as if they were live (BREAKLOG.md).
//!
//! `process()` doesn't allocate, lock or log. `postprocess()` logs each confirmation, recovery
//! and loss, and the 1st, 2nd, 4th, 8th... dropped message; `stop()` logs a summary.

use crate::tracker::{TrackEvent, Tracker, TrackerConfig};
use cu_skylock_payloads::{Detections, TrackState};
use cu29::bincode::de::Decoder;
use cu29::bincode::enc::Encoder;
use cu29::bincode::error::{DecodeError, EncodeError};
use cu29::bincode::{Decode, Encode};
use cu29::prelude::*;
use skylock_timing::DurationHistogram;

/// Follows one target through the detector's output; see the module documentation.
#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct KalmanTracker {
    #[reflect(ignore)]
    tracker: Tracker,
    /// What the last `process()` changed, and the frame's `tov`, for `postprocess()`.
    #[reflect(ignore)]
    event: TrackEvent,
    #[reflect(ignore)]
    event_tov: Option<CuTime>,
    /// Time per frame in `process()`, in 1 µs buckets up to 10 ms.
    #[reflect(ignore)]
    timing: DurationHistogram,
}

/// Freezes the whole tracker state: estimate, covariance, last `tov`, status and counters.
impl Freezable for KalmanTracker {
    fn freeze<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        Encode::encode(self.tracker.state(), encoder)
    }

    fn thaw<D: Decoder>(&mut self, decoder: &mut D) -> Result<(), DecodeError> {
        self.tracker.set_state(Decode::decode(decoder)?);
        Ok(())
    }
}

impl CuTask for KalmanTracker {
    type Resources<'r> = ();
    type Input<'m> = input_msg!(Detections);
    type Output<'m> = output_msg!(TrackState);

    fn new(config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self>
    where
        Self: Sized,
    {
        let tracker_config = match config {
            Some(config) => read_config(config)?,
            None => read_config(&ComponentConfig::new())?,
        };
        Ok(Self {
            tracker: Tracker::new(tracker_config),
            event: TrackEvent::None,
            event_tov: None,
            timing: DurationHistogram::new(1_000, 10_000),
        })
    }

    fn process(
        &mut self,
        ctx: &CuContext,
        input: &Self::Input<'_>,
        output: &mut Self::Output<'_>,
    ) -> CuResult<()> {
        let Some(detections) = input.payload() else {
            output.clear_payload();
            return Ok(());
        };
        let started = ctx.now();
        let tov = match input.tov {
            Tov::Time(tov) => Some(tov),
            Tov::None | Tov::Range(_) => None,
        };
        let mut state = TrackState::default();
        self.event = self.tracker.step(tov, detections.as_slice(), &mut state);
        self.event_tov = tov;
        if self.event == TrackEvent::Dropped {
            output.clear_payload();
        } else {
            output.tov = input.tov;
            output.set_payload(state);
        }
        self.timing.record((ctx.now() - started).as_nanos());
        Ok(())
    }

    fn postprocess(&mut self, _ctx: &CuContext) -> CuResult<()> {
        let tov_ms = self.event_tov.map_or(0, |tov| tov.as_millis());
        match core::mem::take(&mut self.event) {
            TrackEvent::None => {}
            TrackEvent::Dropped => {
                let drops = self.tracker.state().late_drops;
                if drops.is_power_of_two() {
                    warning!(
                        "tracker: dropped a frame whose tov wasn't a single time or wasn't later than the last frame taken ({} so far)",
                        drops
                    );
                }
            }
            TrackEvent::Confirmed => info!("tracker: target confirmed at {} ms", tov_ms),
            TrackEvent::Recovered { gap } => info!(
                "tracker: target recovered at {} ms, {} ms after its previous detection",
                tov_ms,
                gap.as_millis()
            ),
            TrackEvent::Lost { gap } => info!(
                "tracker: target lost at {} ms, {} ms after its last detection",
                tov_ms,
                gap.as_millis()
            ),
        }
        Ok(())
    }

    fn stop(&mut self, _ctx: &CuContext) -> CuResult<()> {
        let state = self.tracker.state();
        let stats = state.stats;
        let [searching, tracking, coasting, lost] = stats.frames;
        info!(
            "tracker: frames: {} searching, {} tracking, {} coasting, {} lost",
            searching, tracking, coasting, lost
        );
        info!(
            "tracker: {} targets confirmed, {} recovered after coasting (longest gap {} ms), {} lost, {} frames dropped",
            stats.confirmed,
            stats.recovered,
            stats.longest_recovered_gap.as_millis(),
            stats.lost,
            state.late_drops
        );
        if let Some(t) = self.timing.summary() {
            info!(
                "tracker: time per frame p50 {} us, p95 {} us, p99 {} us, max {} us",
                t.p50_us, t.p95_us, t.p99_us, t.max_us
            );
        }
        Ok(())
    }
}

fn read_config(config: &ComponentConfig) -> CuResult<TrackerConfig> {
    let confirm_hits = read_u32(config, "confirm_hits", 3)?;
    let gate_chi2 = config.get::<f64>("gate_chi2")?.unwrap_or(9.21);
    let max_coast_ms = read_u32(config, "max_coast_ms", 1000)?;
    let lead_ms = read_u32(config, "lead_ms", 0)?;
    let meas_std_px = config.get::<f64>("meas_std_px")?.unwrap_or(5.0);
    let accel_psd = config.get::<f64>("accel_psd")?.unwrap_or(20_000.0);
    let init_vel_std = config.get::<f64>("init_vel_std")?.unwrap_or(300.0);
    if confirm_hits == 0 || max_coast_ms == 0 {
        return Err(CuError::from(
            "KalmanTracker: confirm_hits and max_coast_ms must be at least 1",
        ));
    }
    let positive = |value: f64| value > 0.0 && value.is_finite();
    let non_negative = |value: f64| value >= 0.0 && value.is_finite();
    if !positive(gate_chi2) || !positive(meas_std_px) {
        return Err(CuError::from(
            "KalmanTracker: gate_chi2 and meas_std_px must be above 0",
        ));
    }
    if !non_negative(accel_psd) || !non_negative(init_vel_std) {
        return Err(CuError::from(
            "KalmanTracker: accel_psd and init_vel_std must be 0 or above",
        ));
    }
    Ok(TrackerConfig {
        confirm_hits,
        gate_chi2,
        max_coast: CuDuration::from_millis(u64::from(max_coast_ms)),
        lead: CuDuration::from_millis(u64::from(lead_ms)),
        meas_var: meas_std_px * meas_std_px,
        accel_psd,
        init_vel_var: init_vel_std * init_vel_std,
    })
}

/// Reads a whole number from 0 to `u32::MAX`. Copper's `get::<u32>` converts with `as`, so a
/// negative value would wrap to a huge one (BREAKLOG.md); this reads it wide and checks it.
fn read_u32(config: &ComponentConfig, key: &str, default: u32) -> CuResult<u32> {
    match config.get::<i64>(key)? {
        None => Ok(default),
        Some(value) => u32::try_from(value).map_err(|_| {
            CuError::from(format!(
                "KalmanTracker: {key} must be a whole number from 0 to {}, not {value}",
                u32::MAX
            ))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cu_skylock_payloads::{Detection, TrackStatus};
    use cu29::bincode::config::standard;
    use cu29::bincode::de::DecoderImpl;
    use cu29::bincode::de::read::SliceReader;
    use cu29::bincode::encode_to_vec;

    const FRAME_NS: u64 = 33_333_333;

    fn task() -> KalmanTracker {
        let mut config = ComponentConfig::new();
        config.set("lead_ms", 100u32);
        KalmanTracker::new(Some(&config), ()).unwrap()
    }

    fn frame(index: u64, seen: bool) -> CuMsg<Detections> {
        let mut detections = Detections::default();
        if seen {
            let cx = 100.0 + 4.0 * index as f32;
            let wobble = if index.is_multiple_of(2) { 1.5 } else { -1.5 };
            let CuArrayVec(items) = &mut detections.items;
            items.push(Detection {
                x_min: cx - 30.0 + wobble,
                y_min: 100.0,
                x_max: cx + 30.0 + wobble,
                y_max: 260.0 - wobble,
                confidence: 0.7,
                class_id: 0,
            });
        }
        let mut msg = CuMsg::new(Some(detections));
        msg.tov = Tov::Time(CuTime::from_nanos(index * FRAME_NS));
        msg
    }

    fn run(task: &mut KalmanTracker, frames: impl Iterator<Item = (u64, bool)>) -> Vec<TrackState> {
        let ctx = CuContext::new_with_clock();
        frames
            .map(|(index, seen)| {
                let mut output = CuMsg::<TrackState>::new(None);
                task.process(&ctx, &frame(index, seen), &mut output)
                    .unwrap();
                assert_eq!(output.tov, Tov::Time(CuTime::from_nanos(index * FRAME_NS)));
                *output.payload().unwrap()
            })
            .collect()
    }

    #[test]
    fn freeze_then_thaw_restores_the_state_exactly() {
        let mut original = task();
        let script = |range: core::ops::Range<u64>| range.map(|i| (i, i % 7 != 3));
        run(&mut original, script(0..40));
        assert_eq!(original.tracker.state().status, TrackStatus::Tracking);
        let bytes = encode_to_vec(BincodeAdapter(&original), standard()).unwrap();

        let mut restored = task();
        let mut decoder = DecoderImpl::new(SliceReader::new(&bytes), standard(), ());
        restored.thaw(&mut decoder).unwrap();
        assert_eq!(restored.tracker.state(), original.tracker.state());

        // Both go on identically, bit for bit, through coasting and recovery.
        let next = || {
            script(40..60)
                .chain((60..70).map(|i| (i, false)))
                .chain(script(70..80))
        };
        assert_eq!(run(&mut restored, next()), run(&mut original, next()));
    }

    #[test]
    fn a_cycle_without_a_frame_outputs_nothing() {
        let mut tracker = task();
        let ctx = CuContext::new_with_clock();
        let mut output = CuMsg::<TrackState>::new(Some(TrackState::default()));
        tracker
            .process(&ctx, &CuMsg::<Detections>::new(None), &mut output)
            .unwrap();
        assert!(output.payload().is_none());
        assert_eq!(tracker.tracker.state().last_tov, None);
    }

    #[test]
    fn a_late_frame_outputs_nothing() {
        let mut tracker = task();
        let states = run(&mut tracker, [(5, true), (6, true)].into_iter());
        assert_eq!(states[1].hits, 2);
        let ctx = CuContext::new_with_clock();
        // The slot still holds the previous cycle's estimate, which must not leak out.
        let mut output = CuMsg::<TrackState>::new(Some(states[1]));
        tracker.process(&ctx, &frame(6, true), &mut output).unwrap();
        assert!(output.payload().is_none());
        assert_eq!(tracker.tracker.state().late_drops, 1);
    }

    #[test]
    fn bad_config_is_refused() {
        for (key, value) in [
            ("confirm_hits", 0i64),
            ("max_coast_ms", 0),
            ("lead_ms", -50),
        ] {
            let mut config = ComponentConfig::new();
            config.set(key, value);
            assert!(KalmanTracker::new(Some(&config), ()).is_err(), "{key}");
        }
        let bad = [
            ("gate_chi2", 0.0),
            ("meas_std_px", -1.0),
            ("accel_psd", f64::NAN),
            ("init_vel_std", -5.0),
        ];
        for (key, value) in bad {
            let mut config = ComponentConfig::new();
            config.set(key, value);
            assert!(KalmanTracker::new(Some(&config), ()).is_err(), "{key}");
        }
        assert!(KalmanTracker::new(None, ()).is_ok());
    }
}
