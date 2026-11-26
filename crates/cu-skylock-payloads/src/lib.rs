//! Copper payloads for object detections and a single-target track state: what a detector
//! such as `cu-skylock-detect` outputs, and what the tracker in `cu-skylock-tracker` takes
//! and outputs. Every payload has a fixed size.
//!
//! In a Copper config, name them `cu_skylock_payloads::Detections` and
//! `cu_skylock_payloads::TrackState`.

use bincode::{Decode, Encode};
use cu29::prelude::*;
use serde::{Deserialize, Serialize};

/// The most detections one frame carries; the detector keeps the most confident.
pub const MAX_DETECTIONS: usize = 32;

/// One detected object. The box is in the source image's pixel coordinates, x to the right
/// and y down from the top-left corner, so a tracker can work on it without knowing the
/// model's input size.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Encode, Decode, Serialize, Deserialize, Reflect,
)]
pub struct Detection {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
    /// The model's score for `class_id`, from 0 to 1.
    pub confidence: f32,
    pub class_id: u16,
}

impl Detection {
    /// The box centre, in pixels.
    pub fn center(&self) -> (f32, f32) {
        (
            (self.x_min + self.x_max) / 2.0,
            (self.y_min + self.y_max) / 2.0,
        )
    }

    /// The box width and height, in pixels.
    pub fn size(&self) -> (f32, f32) {
        (self.x_max - self.x_min, self.y_max - self.y_min)
    }
}

/// One frame's detections, most confident first.
#[derive(Debug, Clone, Default, Encode, Decode, Serialize, Deserialize, Reflect)]
// `CuArrayVec` decodes only with the `()` context, so the derive must use it too.
#[bincode(decode_context = "()")]
#[reflect(opaque, from_reflect = false)]
pub struct Detections {
    pub items: CuArrayVec<Detection, MAX_DETECTIONS>,
}

impl Detections {
    pub fn len(&self) -> usize {
        self.items.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Detection> {
        self.items.0.iter()
    }

    /// The detections, most confident first.
    pub fn as_slice(&self) -> &[Detection] {
        self.items.0.as_slice()
    }
}

/// Where the tracker stands with its one target.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Encode, Decode, Serialize, Deserialize, Reflect,
)]
pub enum TrackStatus {
    /// No confirmed target. While `hits` is above 0, a candidate is gathering detections.
    #[default]
    Searching,
    /// The target took a detection in this frame.
    Tracking,
    /// The target had no detection in its gate in this frame, so its position is predicted.
    Coasting,
    /// The target went longer than `max_coast_ms` without a detection and was dropped. The
    /// estimate is the last one before that, held until a new candidate starts.
    Lost,
}

/// The tracker's estimate for one frame: at the frame's `tov`, except in `Lost`, where it is
/// the estimate held from before the target was lost.
///
/// Positions are the box centre in the source image's pixels, x to the right and y down from
/// the top-left corner, and velocities are in px/s. Image-plane quantities have no SI unit,
/// so these are plain numbers, as in [`Detection`]. In `Searching` with no hits every field
/// but `late_drops` is 0.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Encode, Decode, Serialize, Deserialize, Reflect,
)]
pub struct TrackState {
    pub status: TrackStatus,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// The covariance's diagonal: x and y in px², vx and vy in (px/s)².
    pub var_x: f32,
    pub var_y: f32,
    pub var_vx: f32,
    pub var_vy: f32,
    /// The size of the last detection the track took, in px.
    pub width: f32,
    pub height: f32,
    /// That detection's confidence and class.
    pub confidence: f32,
    pub class_id: u16,
    /// Where the target will be `lead_ms` after this frame at its current velocity, in px.
    pub lead_x: f32,
    pub lead_y: f32,
    /// Detections the current track, or candidate, has taken.
    pub hits: u32,
    /// Frames since the track's last detection.
    pub misses: u32,
    /// Messages dropped since the start because their `tov` wasn't a single time
    /// (`Tov::Time`) or wasn't later than the last message the tracker took.
    pub late_drops: u32,
}

/// The 80 COCO class names, by class id, as the `yolov8*.safetensors` weights number them.
pub const COCO_CLASSES: [&str; 80] = [
    "person",
    "bicycle",
    "car",
    "motorcycle",
    "airplane",
    "bus",
    "train",
    "truck",
    "boat",
    "traffic light",
    "fire hydrant",
    "stop sign",
    "parking meter",
    "bench",
    "bird",
    "cat",
    "dog",
    "horse",
    "sheep",
    "cow",
    "elephant",
    "bear",
    "zebra",
    "giraffe",
    "backpack",
    "umbrella",
    "handbag",
    "tie",
    "suitcase",
    "frisbee",
    "skis",
    "snowboard",
    "sports ball",
    "kite",
    "baseball bat",
    "baseball glove",
    "skateboard",
    "surfboard",
    "tennis racket",
    "bottle",
    "wine glass",
    "cup",
    "fork",
    "knife",
    "spoon",
    "bowl",
    "banana",
    "apple",
    "sandwich",
    "orange",
    "broccoli",
    "carrot",
    "hot dog",
    "pizza",
    "donut",
    "cake",
    "chair",
    "couch",
    "potted plant",
    "bed",
    "dining table",
    "toilet",
    "tv",
    "laptop",
    "mouse",
    "remote",
    "keyboard",
    "cell phone",
    "microwave",
    "oven",
    "toaster",
    "sink",
    "refrigerator",
    "book",
    "clock",
    "vase",
    "scissors",
    "teddy bear",
    "hair drier",
    "toothbrush",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detections_round_trip_through_bincode() {
        let mut detections = Detections::default();
        let CuArrayVec(items) = &mut detections.items;
        items.push(Detection {
            x_min: 1.0,
            y_min: 2.0,
            x_max: 30.5,
            y_max: 40.25,
            confidence: 0.75,
            class_id: 0,
        });
        items.push(Detection {
            x_min: 100.0,
            y_min: 120.0,
            x_max: 130.0,
            y_max: 150.0,
            confidence: 0.5,
            class_id: 2,
        });
        let config = bincode::config::standard();
        let bytes = bincode::encode_to_vec(&detections, config).unwrap();
        let (decoded, used): (Detections, usize) =
            bincode::decode_from_slice(&bytes, config).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(decoded.items.0.as_slice(), detections.items.0.as_slice());
        assert_eq!(decoded.iter().next().unwrap().center(), (15.75, 21.125));
    }

    #[test]
    fn track_state_round_trips_through_bincode() {
        let state = TrackState {
            status: TrackStatus::Coasting,
            x: 320.5,
            y: 180.25,
            vx: -42.0,
            vy: 7.5,
            var_x: 4.0,
            var_y: 4.5,
            var_vx: 900.0,
            var_vy: 950.0,
            width: 80.0,
            height: 200.0,
            confidence: 0.8,
            class_id: 0,
            lead_x: 320.5,
            lead_y: 180.25,
            hits: 12,
            misses: 3,
            late_drops: 1,
        };
        let config = bincode::config::standard();
        let bytes = bincode::encode_to_vec(state, config).unwrap();
        let (decoded, used): (TrackState, usize) =
            bincode::decode_from_slice(&bytes, config).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(decoded, state);
    }
}
