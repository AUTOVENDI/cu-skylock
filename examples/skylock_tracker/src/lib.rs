//! The parts of the example app that its binary (`src/main.rs`) and its log reader
//! (`src/logreader.rs`) share: the camera URL handling, the frame statistics and the app's
//! own tasks. The detector, the tracker and their messages come from the `cu-skylock-detect`,
//! `cu-skylock-tracker` and `cu-skylock-payloads` crates.

pub mod frame_stats;
pub mod rtsp;
pub mod tasks;
