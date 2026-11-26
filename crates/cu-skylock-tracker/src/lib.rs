//! A single-target tracker as a Copper task.
//!
//! [`KalmanTracker`] takes `cu_skylock_payloads::Detections` and outputs
//! `cu_skylock_payloads::TrackState`: the target's status (searching, tracking, coasting or
//! lost), its position and velocity in the image with their variances, and a lead point. The
//! tracking itself is in [`tracker`]: a constant-velocity Kalman filter on the box centre,
//! gating by Mahalanobis distance, and coasting through missed detections. It takes time only
//! from the messages' time of validity, and its whole state freezes and thaws for Copper's
//! keyframes.
//!
//! Name the task in a Copper config as `cu_skylock_tracker::KalmanTracker`. Its config keys
//! are in the task's module documentation, and in this crate's README.
//!
//! This crate doesn't depend on a detector: anything that outputs `Detections` can feed it.

pub mod task;
pub mod tracker;

pub use task::KalmanTracker;
