# cu-skylock-tracker

A Copper task that follows one target through a detector's output with a
constant-velocity Kalman filter.

| | |
|---|---|
| Task | `cu_skylock_tracker::KalmanTracker` |
| Input | `cu_skylock_payloads::Detections` |
| Output | `cu_skylock_payloads::TrackState`: status, position and velocity with their variances, and a lead point |

The crate doesn't depend on a detector: anything that outputs `Detections` can feed it.

## What it does

- **Searching:** the frame's most confident detection starts a candidate, and a candidate
  that takes `confirm_hits` detections in a row becomes the target.
- **Tracking:** the target takes the most confident detection whose squared Mahalanobis
  distance from the predicted centre is at most `gate_chi2`.
- **Coasting:** no detection fell in the gate, so the estimate is the prediction.
- **Lost:** the target went longer than `max_coast_ms` without a detection. The next
  detection starts a new candidate.

The filter's state is the box centre and its velocity, `[x, y, vx, vy]` in px and px/s.
Time comes only from the messages' time of validity: each step's `dt` is the time since the
last message taken, and a message without a single time, or not later than the last one
taken, is dropped and counted in `late_drops`. The task copies its input's time of validity
onto its output.

`process()` doesn't allocate, lock or log. The whole tracker state freezes and thaws
(`Freezable`), so Copper's keyframes capture it.

## Config

All keys are optional.

| Key | Default | Meaning |
|---|---|---|
| `confirm_hits` | 3 | Detections in consecutive frames that make a candidate the target |
| `gate_chi2` | 9.21 | Largest squared Mahalanobis distance the target accepts (99 % for 2 degrees of freedom) |
| `max_coast_ms` | 1000 | How long the target may go without a detection before it is lost; must be longer than the frame interval |
| `lead_ms` | 0 | How far ahead of the frame the lead point is |
| `meas_std_px` | 5 | Standard deviation of a detection's centre, in px |
| `accel_psd` | 20000 | Power spectral density of the target's white-noise acceleration, in px²/s³ |
| `init_vel_std` | 300 | Standard deviation of a new candidate's velocity, in px/s |

The defaults come from an estimate at 30 fps, not from tuning on a real target.

## Limits

One target at a time, picked by confidence; there is no target selection and no
re-identification. The tracker takes a frame's time of validity as the time the frame
shows, so frames that arrive late are tracked as if they were live.

## Depending on it

The crates in this repository are pinned to one commit of Copper. A crate that uses them
must spell its own Copper dependencies with that same `git` and `rev`; the
[repository README](../../README.md#depending-on-the-crates) has the lines to copy.

## Licence

Apache-2.0.
