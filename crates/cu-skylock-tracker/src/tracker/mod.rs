//! SKYLOCK's single-target tracker: one frame's detections in, one track estimate out, with
//! the constant-velocity filter of [`kalman`] on the box centre.
//!
//! The states, as [`TrackStatus`] reports them:
//! - `Searching`: the frame's most confident detection starts a candidate. Each later frame
//!   must put a detection in the candidate's gate within `max_coast` of the previous one, or
//!   the candidate is dropped and that frame's most confident detection starts a new one. A
//!   candidate that has taken `confirm_hits` detections becomes the target.
//! - `Tracking`: the target took a detection in this frame: the most confident one whose
//!   squared Mahalanobis distance from the predicted centre is at most `gate_chi2`.
//! - `Coasting`: no detection fell in the gate, so the estimate is the prediction.
//! - `Lost`: the target went longer than `max_coast` without a detection, whatever the frame
//!   that showed it holds. The estimate is held from the frame before, and the next detection
//!   starts a candidate.
//!
//! Time comes only from the messages' time of validity: each step's `dt` is the time since the
//! last message taken, and a message without a time, or not later than the last message taken,
//! is dropped and counted. The filter maths works in seconds and pixels as `f64`; times and
//! durations here are Copper's `CuTime` and `CuDuration`. Nothing here allocates.

pub mod kalman;

use bincode::{Decode, Encode};
use cu_skylock_payloads::{Detection, TrackState, TrackStatus};
use cu29::prelude::{CuDuration, CuTime};
use kalman::{ConstantVelocity, Innovation};

/// Tracker settings. `KalmanTracker`'s config sets them; see its documentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackerConfig {
    /// Detections, in consecutive frames, that make a candidate the target.
    pub confirm_hits: u32,
    /// The gate on the squared Mahalanobis distance.
    pub gate_chi2: f64,
    /// The longest time without a detection before the target is lost. It also bounds the
    /// time between a candidate's detections, so it must be longer than the frame interval.
    pub max_coast: CuDuration,
    /// How far ahead of the frame the lead point is.
    pub lead: CuDuration,
    /// The variance of a detection's centre on each axis, in px².
    pub meas_var: f64,
    /// The power spectral density of the target's white-noise acceleration, in px²/s³.
    pub accel_psd: f64,
    /// The variance of a new candidate's velocity on each axis, in (px/s)².
    pub init_vel_var: f64,
}

/// What a step changed, for the log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrackEvent {
    /// Nothing worth logging.
    #[default]
    None,
    /// The message was dropped: it had no time, or its time wasn't later than the last
    /// message taken.
    Dropped,
    /// A candidate became the target.
    Confirmed,
    /// The target took a detection again, `gap` after its previous one.
    Recovered { gap: CuDuration },
    /// The target was lost, `gap` after its last detection.
    Lost { gap: CuDuration },
}

/// Counts over the run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Encode, Decode)]
pub struct TrackerStats {
    /// Messages taken, by the status they left: searching, tracking, coasting, lost.
    pub frames: [u64; 4],
    /// Candidates that became the target.
    pub confirmed: u32,
    /// Coasts that ended with the target taking a detection again.
    pub recovered: u32,
    /// Targets lost.
    pub lost: u32,
    /// The longest gap between two detections of a target that was recovered.
    pub longest_recovered_gap: CuDuration,
}

/// Everything that carries from one message to the next, which the task freezes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Encode, Decode)]
pub struct TrackerState {
    /// The estimate of the target or candidate.
    pub filter: ConstantVelocity,
    /// Where the tracker stands, as the last message left it.
    pub status: TrackStatus,
    /// The time of the last message taken.
    pub last_tov: Option<CuTime>,
    /// The time of the last detection the track took.
    pub last_hit: CuTime,
    /// Detections the current track, or candidate, has taken.
    pub hits: u32,
    /// Frames since the track's last detection.
    pub misses: u32,
    /// Messages dropped since the start.
    pub late_drops: u32,
    /// The size of the last detection taken, in px.
    pub size: [f32; 2],
    /// That detection's confidence.
    pub confidence: f32,
    /// That detection's class.
    pub class_id: u16,
    /// Counts over the run.
    pub stats: TrackerStats,
}

/// One target's tracker: [`Tracker::step`] takes each frame's detections in turn.
pub struct Tracker {
    config: TrackerConfig,
    state: TrackerState,
}

impl Tracker {
    /// A tracker with no candidate yet.
    pub fn new(config: TrackerConfig) -> Self {
        Self {
            config,
            state: TrackerState::default(),
        }
    }

    /// Everything that carries from one message to the next.
    pub fn state(&self) -> &TrackerState {
        &self.state
    }

    /// Replaces that state, as a keyframe restore does.
    pub fn set_state(&mut self, state: TrackerState) {
        self.state = state;
    }

    /// Takes one frame's detections, valid at `tov`, and writes the estimate into `out`. A
    /// dropped message leaves `out` alone.
    pub fn step(
        &mut self,
        tov: Option<CuTime>,
        detections: &[Detection],
        out: &mut TrackState,
    ) -> TrackEvent {
        let last_tov = self.state.last_tov;
        let Some(tov) = tov.filter(|&tov| last_tov.is_none_or(|last| tov > last)) else {
            self.state.late_drops = self.state.late_drops.saturating_add(1);
            return TrackEvent::Dropped;
        };
        let dt = last_tov.map_or(0.0, |last| seconds(tov - last));
        self.state.last_tov = Some(tov);

        let event = match self.state.status {
            TrackStatus::Tracking | TrackStatus::Coasting => self.follow(tov, dt, detections),
            TrackStatus::Searching if self.state.hits > 0 => self.confirm(tov, dt, detections),
            TrackStatus::Searching | TrackStatus::Lost => self.search(tov, detections),
        };
        let stats = &mut self.state.stats;
        stats.frames[self.state.status as usize] += 1;
        match event {
            TrackEvent::Confirmed => stats.confirmed += 1,
            TrackEvent::Recovered { gap } => {
                stats.recovered += 1;
                stats.longest_recovered_gap = stats.longest_recovered_gap.max(gap);
            }
            TrackEvent::Lost { .. } => stats.lost += 1,
            TrackEvent::None | TrackEvent::Dropped => {}
        }
        self.write(out);
        event
    }

    /// A confirmed target: lost if its last detection is too old, else predict, then take
    /// the best detection in the gate or coast.
    fn follow(&mut self, tov: CuTime, dt: f64, detections: &[Detection]) -> TrackEvent {
        let gap = tov - self.state.last_hit;
        if gap > self.config.max_coast {
            self.state.misses = self.state.misses.saturating_add(1);
            self.state.status = TrackStatus::Lost;
            return TrackEvent::Lost { gap };
        }
        self.state.filter.predict(dt, self.config.accel_psd);
        if let Some((detection, innovation)) = self.best_in_gate(detections) {
            let was_coasting = self.state.status == TrackStatus::Coasting;
            self.take(tov, &detection, &innovation);
            self.state.status = TrackStatus::Tracking;
            return if was_coasting {
                TrackEvent::Recovered { gap }
            } else {
                TrackEvent::None
            };
        }
        self.state.misses = self.state.misses.saturating_add(1);
        self.state.status = TrackStatus::Coasting;
        TrackEvent::None
    }

    /// A candidate: it must take a detection in every frame until it is confirmed.
    fn confirm(&mut self, tov: CuTime, dt: f64, detections: &[Detection]) -> TrackEvent {
        if tov - self.state.last_hit <= self.config.max_coast {
            self.state.filter.predict(dt, self.config.accel_psd);
            if let Some((detection, innovation)) = self.best_in_gate(detections) {
                self.take(tov, &detection, &innovation);
                return self.confirm_if_ready();
            }
        }
        self.search(tov, detections)
    }

    /// No candidate: the most confident detection starts one.
    fn search(&mut self, tov: CuTime, detections: &[Detection]) -> TrackEvent {
        let Some(detection) = most_confident(detections) else {
            if self.state.status == TrackStatus::Lost {
                self.state.misses = self.state.misses.saturating_add(1);
            } else {
                self.clear_target();
            }
            return TrackEvent::None;
        };
        let (cx, cy) = detection.center();
        self.state.filter = ConstantVelocity::new(
            [f64::from(cx), f64::from(cy)],
            self.config.meas_var,
            self.config.init_vel_var,
        );
        self.state.status = TrackStatus::Searching;
        self.state.hits = 1;
        self.state.misses = 0;
        self.state.last_hit = tov;
        self.keep_box(detection);
        self.confirm_if_ready()
    }

    fn confirm_if_ready(&mut self) -> TrackEvent {
        if self.state.hits >= self.config.confirm_hits {
            self.state.status = TrackStatus::Tracking;
            TrackEvent::Confirmed
        } else {
            TrackEvent::None
        }
    }

    /// The most confident detection whose centre falls in the gate, with its innovation. On
    /// equal confidence, the nearer one.
    fn best_in_gate(&self, detections: &[Detection]) -> Option<(Detection, Innovation)> {
        let mut best: Option<(Detection, Innovation)> = None;
        for detection in detections.iter().filter(|d| is_usable(d)) {
            let (cx, cy) = detection.center();
            let Some(innovation) = self
                .state
                .filter
                .innovation([f64::from(cx), f64::from(cy)], self.config.meas_var)
            else {
                continue;
            };
            if innovation.distance2 > self.config.gate_chi2 {
                continue;
            }
            let better = best.as_ref().is_none_or(|(kept, kept_innovation)| {
                detection.confidence > kept.confidence
                    || (detection.confidence == kept.confidence
                        && innovation.distance2 < kept_innovation.distance2)
            });
            if better {
                best = Some((*detection, innovation));
            }
        }
        best
    }

    fn take(&mut self, tov: CuTime, detection: &Detection, innovation: &Innovation) {
        self.state.filter.update(innovation, self.config.meas_var);
        self.state.hits = self.state.hits.saturating_add(1);
        self.state.misses = 0;
        self.state.last_hit = tov;
        self.keep_box(detection);
    }

    fn keep_box(&mut self, detection: &Detection) {
        let (width, height) = detection.size();
        self.state.size = [width, height];
        self.state.confidence = detection.confidence;
        self.state.class_id = detection.class_id;
    }

    /// Back to searching with no candidate.
    fn clear_target(&mut self) {
        let state = &mut self.state;
        state.filter = ConstantVelocity::default();
        state.status = TrackStatus::Searching;
        state.hits = 0;
        state.misses = 0;
        state.size = [0.0; 2];
        state.confidence = 0.0;
        state.class_id = 0;
    }

    fn write(&self, out: &mut TrackState) {
        let s = &self.state;
        if s.status == TrackStatus::Searching && s.hits == 0 {
            *out = TrackState {
                late_drops: s.late_drops,
                ..TrackState::default()
            };
            return;
        }
        let [x, y, vx, vy] = s.filter.x;
        let p = &s.filter.p;
        let lead_s = seconds(self.config.lead);
        *out = TrackState {
            status: s.status,
            x: x as f32,
            y: y as f32,
            vx: vx as f32,
            vy: vy as f32,
            var_x: p[0][0] as f32,
            var_y: p[1][1] as f32,
            var_vx: p[2][2] as f32,
            var_vy: p[3][3] as f32,
            width: s.size[0],
            height: s.size[1],
            confidence: s.confidence,
            class_id: s.class_id,
            lead_x: (x + vx * lead_s) as f32,
            lead_y: (y + vy * lead_s) as f32,
            hits: s.hits,
            misses: s.misses,
            late_drops: s.late_drops,
        };
    }
}

/// A duration in seconds, for the filter maths.
fn seconds(duration: CuDuration) -> f64 {
    duration.as_nanos() as f64 / 1e9
}

fn is_usable(detection: &Detection) -> bool {
    let (cx, cy) = detection.center();
    cx.is_finite() && cy.is_finite() && detection.confidence.is_finite()
}

/// The most confident usable detection; the first one on equal confidence.
fn most_confident(detections: &[Detection]) -> Option<&Detection> {
    detections
        .iter()
        .filter(|d| is_usable(d))
        .fold(None, |best: Option<&Detection>, detection| match best {
            Some(kept) if kept.confidence >= detection.confidence => Some(kept),
            _ => Some(detection),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_NS: u64 = 33_333_333;

    fn frame_time(frame: u64) -> CuTime {
        CuTime::from_nanos(frame * FRAME_NS)
    }

    fn frames(count: u64) -> CuDuration {
        CuDuration::from_nanos(count * FRAME_NS)
    }

    /// The scenes below move at constant velocity, so the process noise is low. Detections
    /// carry 2 px of noise against the 3 px the filter assumes.
    fn config() -> TrackerConfig {
        TrackerConfig {
            confirm_hits: 3,
            gate_chi2: 9.21,
            max_coast: CuDuration::from_millis(1000),
            lead: CuDuration::from_millis(100),
            meas_var: 9.0,
            accel_psd: 500.0,
            init_vel_var: 300.0 * 300.0,
        }
    }

    fn detection_at(cx: f32, cy: f32, confidence: f32) -> Detection {
        Detection {
            x_min: cx - 40.0,
            y_min: cy - 100.0,
            x_max: cx + 40.0,
            y_max: cy + 100.0,
            confidence,
            class_id: 0,
        }
    }

    /// Repeatable noise: xorshift64 into a sum of four uniforms, about N(0, σ²).
    struct Noise(u64);

    impl Noise {
        fn next_uniform(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }

        fn sample(&mut self, sigma: f64) -> f64 {
            // Four uniforms have variance 4/12; scale that to σ².
            let sum: f64 = (0..4).map(|_| self.next_uniform() - 0.5).sum();
            sum * sigma * 3f64.sqrt()
        }
    }

    /// A target moving at constant velocity, seen with noise in every frame.
    struct Scene {
        tracker: Tracker,
        start: (f64, f64),
        velocity: (f64, f64),
        noise: Noise,
        frame: u64,
    }

    impl Scene {
        fn new() -> Self {
            Self {
                tracker: Tracker::new(config()),
                start: (100.0, 200.0),
                velocity: (120.0, -45.0),
                noise: Noise(0x9e37_79b9_7f4a_7c15),
                frame: 0,
            }
        }

        fn truth(&self, frame: u64) -> (f64, f64) {
            let t = (frame * FRAME_NS) as f64 / 1e9;
            (
                self.start.0 + self.velocity.0 * t,
                self.start.1 + self.velocity.1 * t,
            )
        }

        /// One frame, with the target seen or not.
        fn step(&mut self, seen: bool) -> (TrackState, TrackEvent) {
            let (x, y) = self.truth(self.frame);
            let detection = detection_at(
                (x + self.noise.sample(2.0)) as f32,
                (y + self.noise.sample(2.0)) as f32,
                0.8,
            );
            let detections: &[Detection] = if seen { &[detection] } else { &[] };
            let mut out = TrackState::default();
            let event = self
                .tracker
                .step(Some(frame_time(self.frame)), detections, &mut out);
            self.frame += 1;
            (out, event)
        }
    }

    #[test]
    fn a_noisy_constant_velocity_track_converges() {
        let mut scene = Scene::new();
        let mut events = Vec::new();
        let mut out = TrackState::default();
        for _ in 0..90 {
            let (state, event) = scene.step(true);
            events.push(event);
            out = state;
        }
        // Confirmed on the third frame, and tracking ever since.
        assert_eq!(events[2], TrackEvent::Confirmed);
        assert_eq!(events.iter().filter(|&&e| e != TrackEvent::None).count(), 1);
        assert_eq!(out.status, TrackStatus::Tracking);
        assert_eq!((out.hits, out.misses), (90, 0));
        // σ started at 3 px on position and 300 px/s on velocity (at 0, against 120 and -45).
        // In steady state this configuration gives 1.5 px and 10 px/s; after 3 frames it is
        // still 2.5 px and 40 px/s. The estimate must be within 3 σ of the truth.
        let (x, y) = scene.truth(89);
        let checks = [
            ("x", out.x, x, out.var_x, 2.0),
            ("y", out.y, y, out.var_y, 2.0),
            ("vx", out.vx, 120.0, out.var_vx, 15.0),
            ("vy", out.vy, -45.0, out.var_vy, 15.0),
        ];
        for (name, estimate, truth, variance, max_sigma) in checks {
            let sigma = f64::from(variance).sqrt();
            let error = (f64::from(estimate) - truth).abs();
            assert!(sigma < max_sigma, "{name}: σ {sigma}");
            assert!(
                error < 3.0 * sigma,
                "{name}: {estimate} against {truth}, σ {sigma}"
            );
        }
        // The lead point is 0.1 s ahead along the velocity.
        assert!((out.lead_x - (out.x + 0.1 * out.vx)).abs() < 1e-3);
        assert!((out.lead_y - (out.y + 0.1 * out.vy)).abs() < 1e-3);
        let stats = scene.tracker.state().stats;
        assert_eq!(stats.frames, [2, 88, 0, 0]);
        assert_eq!(stats.confirmed, 1);
    }

    #[test]
    fn missed_detections_coast_then_lose_the_target() {
        let mut scene = Scene::new();
        for _ in 0..30 {
            scene.step(true);
        }
        let last_hit = scene.frame - 1;
        let (seen_x, _) = scene.truth(last_hit);
        // 30 frames is 1.0 s: still coasting.
        let mut coasted = Vec::new();
        for _ in 0..30 {
            coasted.push(scene.step(false).0);
        }
        assert!(coasted.iter().all(|s| s.status == TrackStatus::Coasting));
        assert_eq!(coasted.last().unwrap().misses, 30);
        // The prediction kept moving with the target, and its uncertainty grew.
        let (x, _) = scene.truth(scene.frame - 1);
        let last = coasted.last().unwrap();
        let sigma = f64::from(last.var_x).sqrt();
        assert!(
            (f64::from(last.x) - x).abs() < 3.0 * sigma,
            "x {} against {x}",
            last.x
        );
        assert!(f64::from(last.x) > seen_x + 60.0);
        assert!(last.var_x > coasted[0].var_x);

        // 31 frames is past max_coast: lost, holding the last prediction.
        let (lost, event) = scene.step(false);
        assert_eq!(lost.status, TrackStatus::Lost);
        assert_eq!(event, TrackEvent::Lost { gap: frames(31) });
        assert_eq!((lost.x, lost.y, lost.misses), (last.x, last.y, 31));
        let (held, event) = scene.step(false);
        assert_eq!(event, TrackEvent::None);
        assert_eq!((held.status, held.x, held.y), (lost.status, lost.x, lost.y));
        assert_eq!(held.misses, 32);
        // The next detection starts a candidate.
        let (candidate, _) = scene.step(true);
        assert_eq!(
            (candidate.status, candidate.hits),
            (TrackStatus::Searching, 1)
        );
        assert_eq!(scene.tracker.state().stats.lost, 1);
    }

    #[test]
    fn a_detection_after_max_coast_does_not_save_the_target() {
        let mut scene = Scene::new();
        for _ in 0..30 {
            scene.step(true);
        }
        let before = scene.step(true).0;
        // No message for 2 s, as when the stream freezes, then a detection right where the
        // target would be.
        scene.frame += 60;
        let (x, y) = scene.truth(scene.frame);
        let mut out = TrackState::default();
        let event = scene.tracker.step(
            Some(frame_time(scene.frame)),
            &[detection_at(x as f32, y as f32, 0.9)],
            &mut out,
        );
        assert_eq!(event, TrackEvent::Lost { gap: frames(61) });
        assert_eq!(
            (out.status, out.x, out.y),
            (TrackStatus::Lost, before.x, before.y)
        );
        // The next frame's detection starts a candidate, wherever it is.
        let event = scene.tracker.step(
            Some(frame_time(scene.frame + 1)),
            &[detection_at(40.0, 40.0, 0.9)],
            &mut out,
        );
        assert_eq!(event, TrackEvent::None);
        assert_eq!(
            (out.status, out.hits, out.x),
            (TrackStatus::Searching, 1, 40.0)
        );
    }

    #[test]
    fn the_track_holds_through_a_short_occlusion() {
        let mut scene = Scene::new();
        for _ in 0..30 {
            scene.step(true);
        }
        for _ in 0..12 {
            assert_eq!(scene.step(false).0.status, TrackStatus::Coasting);
        }
        let (state, event) = scene.step(true);
        assert_eq!(event, TrackEvent::Recovered { gap: frames(13) });
        assert_eq!(
            (state.status, state.hits, state.misses),
            (TrackStatus::Tracking, 31, 0)
        );
        let stats = scene.tracker.state().stats;
        assert_eq!(
            (stats.recovered, stats.longest_recovered_gap),
            (1, frames(13))
        );
    }

    #[test]
    fn a_candidate_needs_its_hits_in_consecutive_frames() {
        let mut tracker = Tracker::new(config());
        let mut out = TrackState::default();
        let mut step = |frame: u64, detections: &[Detection], out: &mut TrackState| {
            tracker.step(Some(frame_time(frame)), detections, out)
        };
        step(0, &[detection_at(300.0, 150.0, 0.9)], &mut out);
        step(1, &[detection_at(302.0, 150.0, 0.9)], &mut out);
        assert_eq!((out.status, out.hits), (TrackStatus::Searching, 2));
        // A frame without it drops the candidate.
        step(2, &[], &mut out);
        assert_eq!(out, TrackState::default());
        // A detection far from the candidate starts a new one instead.
        step(3, &[detection_at(300.0, 150.0, 0.9)], &mut out);
        step(4, &[detection_at(500.0, 150.0, 0.9)], &mut out);
        assert_eq!((out.status, out.hits), (TrackStatus::Searching, 1));
        assert_eq!(out.x, 500.0);
        step(5, &[detection_at(501.0, 150.0, 0.9)], &mut out);
        let event = step(6, &[detection_at(502.0, 151.0, 0.9)], &mut out);
        assert_eq!(
            (event, out.status),
            (TrackEvent::Confirmed, TrackStatus::Tracking)
        );
    }

    #[test]
    fn the_gate_keeps_the_target_against_a_more_confident_outlier() {
        let mut scene = Scene::new();
        for _ in 0..20 {
            scene.step(true);
        }
        let (x, y) = scene.truth(scene.frame);
        let detections = [
            detection_at(x as f32 + 200.0, y as f32, 0.95),
            detection_at(x as f32 + 1.0, y as f32 - 1.0, 0.40),
        ];
        let mut out = TrackState::default();
        scene
            .tracker
            .step(Some(frame_time(scene.frame)), &detections, &mut out);
        assert_eq!(out.status, TrackStatus::Tracking);
        assert_eq!(out.confidence, 0.40);
        assert!((f64::from(out.x) - x).abs() < 5.0);
    }

    #[test]
    fn a_late_or_untimed_message_is_dropped_and_counted() {
        let mut tracker = Tracker::new(config());
        let mut out = TrackState::default();
        let seen = [detection_at(320.0, 180.0, 0.9)];
        assert_eq!(
            tracker.step(Some(frame_time(100)), &seen, &mut out),
            TrackEvent::None
        );
        let before = *tracker.state();
        let untouched = TrackState {
            hits: 99,
            ..TrackState::default()
        };
        for tov in [Some(frame_time(100)), Some(frame_time(50)), None] {
            let mut out = untouched;
            assert_eq!(tracker.step(tov, &seen, &mut out), TrackEvent::Dropped);
            assert_eq!(out, untouched);
        }
        let after = *tracker.state();
        assert_eq!(after.late_drops, 3);
        assert_eq!(
            TrackerState {
                late_drops: before.late_drops,
                ..after
            },
            before
        );
        // The next message on time is taken, one frame after the last one taken.
        tracker.step(Some(frame_time(101)), &seen, &mut out);
        assert_eq!((out.hits, out.late_drops), (2, 3));
        assert_eq!(tracker.state().last_tov, Some(frame_time(101)));
    }
}
