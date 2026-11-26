//! YOLOv8 object detection as a Copper task.
//!
//! [`YoloDetect`] takes an NV12 `cu_sensor_payloads::CuImage<Vec<u8>>` and outputs
//! `cu_skylock_payloads::Detections`: up to 32 boxes in source-image pixels, each with a
//! confidence and a COCO class id. The model is YOLOv8 in Candle ([`yolo`]), with the weights
//! `lmz/candle-yolo-v8` publishes on Hugging Face; they are downloaded into the local Hugging
//! Face cache on the first run and are not part of this crate.
//!
//! Name the task in a Copper config as `cu_skylock_detect::YoloDetect`. Its config keys are
//! in the task's module documentation, and in this crate's README.
//!
//! The `cuda` feature runs inference on an NVIDIA GPU; without it the task runs on the CPU.

pub mod task;
pub mod yolo;

pub use task::YoloDetect;
