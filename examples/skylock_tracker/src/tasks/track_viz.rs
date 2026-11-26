//! Rerun view of the camera with the detector's boxes and the tracker's target.
//!
//! The detections are boxes labelled with their class and confidence. The target is drawn in
//! its status's colour: a box of the last detection's size around the estimated centre, an
//! arrow to where its velocity takes it in half a second, and a dot at the lead point,
//! labelled with the status, the hits and the speed. The label sits on the dot, inside the
//! box, because Rerun puts a box's label under its bottom edge, where the detection's label
//! already is. The target is drawn only in a cycle that carries a frame, and is cut to the
//! frame: Rerun fits the 2D view to everything drawn in it, so an arrow or a coasting
//! estimate past the edge would shrink the picture. A lead point outside the frame is drawn
//! on its edge.
//!
//! The viewer that `new()` spawns listens on 127.0.0.1 only and keeps at most `memory_limit`
//! (default `2GB`) of history before it drops the oldest. Frames go to it as NV12, half the
//! bytes of RGB, and the timeline is each frame's `tov`.

use cu_sensor_payloads::CuImage;
use cu_skylock_payloads::{COCO_CLASSES, Detections, MAX_DETECTIONS, TrackState, TrackStatus};
use cu29::prelude::*;
use rerun::{
    AnnotationContext, Arrows2D, Boxes2D, Clear, Color, Image, PixelFormat, Points2D, Radius,
    RecordingStream, RecordingStreamBuilder, SpawnOptions,
};
use std::fmt::Write;

const CAMERA_ENTITY: &str = "camera";
const IMAGE_ENTITY: &str = "camera/image";
const BOXES_ENTITY: &str = "camera/image/detections";
const TRACK_ENTITY: &str = "camera/image/track";
const VELOCITY_ENTITY: &str = "camera/image/track/velocity";
const LEAD_ENTITY: &str = "camera/image/track/lead";
/// The velocity arrow is the distance the target covers in this time, in s.
const ARROW_S: f32 = 0.5;
/// Line and dot sizes in UI points, so they stay thin on screen at any zoom.
const TRACK_LINE_RADIUS: f32 = 2.0;
const LEAD_RADIUS: f32 = 3.0;
/// Room for a class name and a confidence, or a status, a count and a speed, so labels don't
/// reallocate.
const LABEL_CAPACITY: usize = 48;

/// Catppuccin Mocha accents, one per status.
fn status_color(status: TrackStatus) -> Color {
    match status {
        TrackStatus::Searching => Color::from_rgb(180, 190, 254),
        TrackStatus::Tracking => Color::from_rgb(166, 227, 161),
        TrackStatus::Coasting => Color::from_rgb(249, 226, 175),
        TrackStatus::Lost => Color::from_rgb(243, 139, 168),
    }
}

fn status_name(status: TrackStatus) -> &'static str {
    match status {
        TrackStatus::Searching => "searching",
        TrackStatus::Tracking => "tracking",
        TrackStatus::Coasting => "coasting",
        TrackStatus::Lost => "lost",
    }
}

/// Shows the frames, the detections and the target in a Rerun viewer it spawns.
#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct RerunTrackViz {
    #[reflect(ignore)]
    rec: RecordingStream,
    /// Box labels, rewritten in place for each frame.
    #[reflect(ignore)]
    labels: [String; MAX_DETECTIONS],
    #[reflect(ignore)]
    track_label: String,
    /// Whether the view shows a target, so that it is cleared once when there is none.
    track_shown: bool,
}

impl Freezable for RerunTrackViz {}

impl CuSinkTask for RerunTrackViz {
    type Resources<'r> = ();
    type Input<'m> = input_msg!('m, CuImage<Vec<u8>>, Detections, TrackState);

    fn new(config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self>
    where
        Self: Sized,
    {
        let memory_limit = match config {
            Some(config) => config.get::<String>("memory_limit")?,
            None => None,
        }
        .unwrap_or_else(|| "2GB".to_string());
        let options = SpawnOptions {
            memory_limit,
            // The proxy's buffer only serves viewers that connect late.
            server_memory_limit: "64MB".to_string(),
            extra_args: vec!["--bind".to_string(), "127.0.0.1".to_string()],
            ..Default::default()
        };
        let rec = RecordingStreamBuilder::new("SKYLOCK tracker")
            .spawn_opts(&options)
            .map_err(|e| CuError::new_with_cause("Failed to spawn the Rerun viewer", e))?;
        let classes = COCO_CLASSES
            .iter()
            .enumerate()
            .map(|(id, name)| (id as u16, *name));
        rec.log_static(CAMERA_ENTITY, &AnnotationContext::new(classes))
            .map_err(|e| CuError::new_with_cause("Failed to log the class names", e))?;
        Ok(Self {
            rec,
            labels: std::array::from_fn(|_| String::with_capacity(LABEL_CAPACITY)),
            track_label: String::with_capacity(LABEL_CAPACITY),
            track_shown: false,
        })
    }

    fn process(&mut self, _ctx: &CuContext, input: &Self::Input<'_>) -> CuResult<()> {
        let (image_msg, detections_msg, track_msg) = *input;
        if let Tov::Time(tov) = image_msg.tov {
            self.rec
                .set_duration_secs("tov", tov.as_nanos() as f64 / 1e9);
        }
        let mut frame_size = None;
        if let Some(image) = image_msg.payload() {
            self.log_image(image)?;
            frame_size = Some([image.format.width as f32, image.format.height as f32]);
        }
        if let Some(detections) = detections_msg.payload() {
            self.log_detections(detections)?;
        }
        if let (Some(track), Some(frame_size)) = (track_msg.payload(), frame_size) {
            self.log_track(track, frame_size)?;
        }
        Ok(())
    }
}

impl RerunTrackViz {
    fn log_image(&self, image: &CuImage<Vec<u8>>) -> CuResult<()> {
        let format = &image.format;
        if &format.pixel_format != b"NV12" || format.stride != format.width {
            return Err(CuError::from(
                "RerunTrackViz shows only NV12 frames without row padding",
            ));
        }
        let size = [format.width, format.height];
        let bytes = (format.width * format.height * 3 / 2) as usize;
        image.buffer_handle.with_inner(|data| {
            let Some(nv12) = data.get(..bytes) else {
                return Err(CuError::from(
                    "RerunTrackViz: the frame is shorter than NV12",
                ));
            };
            let frame = Image::from_pixel_format(size, PixelFormat::NV12, nv12);
            self.rec
                .log(IMAGE_ENTITY, &frame)
                .map_err(|e| CuError::new_with_cause("Failed to log the image", e))
        })
    }

    fn log_detections(&mut self, detections: &Detections) -> CuResult<()> {
        for (label, detection) in self.labels.iter_mut().zip(detections.iter()) {
            label.clear();
            let name = COCO_CLASSES
                .get(usize::from(detection.class_id))
                .copied()
                .unwrap_or("?");
            let _ = write!(label, "{name} {:.2}", detection.confidence);
        }
        let boxes = Boxes2D::from_mins_and_sizes(
            detections.iter().map(|d| [d.x_min, d.y_min]),
            detections.iter().map(|d| {
                let (w, h) = d.size();
                [w, h]
            }),
        )
        .with_class_ids(detections.iter().map(|d| d.class_id))
        .with_labels(self.labels[..detections.len()].iter().map(String::as_str));
        self.rec
            .log(BOXES_ENTITY, &boxes)
            .map_err(|e| CuError::new_with_cause("Failed to log the boxes", e))
    }

    /// Draws the target, cut to a frame of `frame` (width, height) px. Each part is logged in
    /// every frame, empty when it falls outside, so none is left over from an earlier frame.
    fn log_track(&mut self, track: &TrackState, frame: [f32; 2]) -> CuResult<()> {
        let failed = |e| CuError::new_with_cause("Failed to log the track", e);
        if track.status == TrackStatus::Searching && track.hits == 0 {
            if self.track_shown {
                self.rec
                    .log(TRACK_ENTITY, &Clear::recursive())
                    .map_err(failed)?;
                self.track_shown = false;
            }
            return Ok(());
        }
        let color = status_color(track.status);
        let center = [track.x, track.y];
        let target = clip_box(center, [track.width, track.height], frame);
        let velocity = clip_arrow(center, [track.vx * ARROW_S, track.vy * ARROW_S], frame);
        let lead = onto_frame([track.lead_x, track.lead_y], frame);
        self.track_label.clear();
        let _ = write!(
            self.track_label,
            "{} {} · {:.0} px/s",
            status_name(track.status),
            track.hits,
            track.vx.hypot(track.vy)
        );
        let target =
            Boxes2D::from_mins_and_sizes(target.map(|(min, _)| min), target.map(|(_, size)| size))
                .with_colors([color])
                .with_radii([Radius::new_ui_points(TRACK_LINE_RADIUS)]);
        let velocity = Arrows2D::from_vectors(velocity)
            .with_origins(velocity.map(|_| center))
            .with_colors([color]);
        let lead = Points2D::new(lead)
            .with_colors([color])
            .with_radii([Radius::new_ui_points(LEAD_RADIUS)])
            .with_labels(lead.map(|_| self.track_label.as_str()));
        self.rec.log(TRACK_ENTITY, &target).map_err(failed)?;
        self.rec.log(VELOCITY_ENTITY, &velocity).map_err(failed)?;
        self.rec.log(LEAD_ENTITY, &lead).map_err(failed)?;
        self.track_shown = true;
        Ok(())
    }
}

fn is_inside(point: [f32; 2], frame: [f32; 2]) -> bool {
    (0.0..=frame[0]).contains(&point[0]) && (0.0..=frame[1]).contains(&point[1])
}

/// The part of the box of `size` around `center` that lies in the frame, as its top-left
/// corner and size; `None` when no part does.
fn clip_box(center: [f32; 2], size: [f32; 2], frame: [f32; 2]) -> Option<([f32; 2], [f32; 2])> {
    if !center.iter().chain(&size).all(|v| v.is_finite()) {
        return None;
    }
    let min = [
        (center[0] - size[0] / 2.0).max(0.0),
        (center[1] - size[1] / 2.0).max(0.0),
    ];
    let max = [
        (center[0] + size[0] / 2.0).min(frame[0]),
        (center[1] + size[1] / 2.0).min(frame[1]),
    ];
    (max[0] > min[0] && max[1] > min[1]).then_some((min, [max[0] - min[0], max[1] - min[1]]))
}

/// The arrow from `origin` along `vector`, cut where it leaves the frame; `None` when
/// `origin` is outside it.
fn clip_arrow(origin: [f32; 2], vector: [f32; 2], frame: [f32; 2]) -> Option<[f32; 2]> {
    if !is_inside(origin, frame) || !vector.iter().all(|v| v.is_finite()) {
        return None;
    }
    let mut scale = 1.0_f32;
    for ((&start, &step), &end) in origin.iter().zip(&vector).zip(&frame) {
        if step > 0.0 {
            scale = scale.min((end - start) / step);
        } else if step < 0.0 {
            scale = scale.min(-start / step);
        }
    }
    Some([vector[0] * scale, vector[1] * scale])
}

/// `point` moved onto the frame's nearest edge if it lies outside; `None` if it isn't finite.
fn onto_frame(point: [f32; 2], frame: [f32; 2]) -> Option<[f32; 2]> {
    point
        .iter()
        .all(|v| v.is_finite())
        .then(|| [point[0].clamp(0.0, frame[0]), point[1].clamp(0.0, frame[1])])
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: [f32; 2] = [640.0, 360.0];

    #[test]
    fn the_target_is_cut_to_the_frame() {
        // Inside: unchanged.
        assert_eq!(
            clip_box([320.0, 180.0], [80.0, 200.0], FRAME),
            Some(([280.0, 80.0], [80.0, 200.0]))
        );
        // Over the right edge: cut there.
        assert_eq!(
            clip_box([620.0, 180.0], [80.0, 100.0], FRAME),
            Some(([580.0, 130.0], [60.0, 100.0]))
        );
        // Entirely outside: nothing.
        assert_eq!(clip_box([700.0, 180.0], [80.0, 100.0], FRAME), None);
        assert_eq!(clip_box([f32::NAN, 180.0], [80.0, 100.0], FRAME), None);

        // An arrow ends at the edge it crosses first.
        assert_eq!(
            clip_arrow([600.0, 100.0], [80.0, 40.0], FRAME),
            Some([40.0, 20.0])
        );
        assert_eq!(
            clip_arrow([10.0, 20.0], [-20.0, -10.0], FRAME),
            Some([-10.0, -5.0])
        );
        assert_eq!(
            clip_arrow([100.0, 100.0], [30.0, 0.0], FRAME),
            Some([30.0, 0.0])
        );
        assert_eq!(clip_arrow([700.0, 100.0], [-10.0, 0.0], FRAME), None);

        assert_eq!(onto_frame([700.0, -5.0], FRAME), Some([640.0, 0.0]));
        assert_eq!(onto_frame([f32::INFINITY, 5.0], FRAME), None);
    }
}
