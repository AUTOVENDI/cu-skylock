//! The app's own tasks, as `copperconfig.ron` names them, and the camera source's type. The
//! detector and the tracker are named there by their crates.

mod gst_to_image;
mod track_viz;

pub use gst_to_image::GstToCuImage;
pub use track_viz::RerunTrackViz;

/// How many frames the camera source holds. What happens when the ring is full, and which
/// frame a pass takes, is the source's `frame_mode` in `copperconfig.ron`.
/// `cu_gstreamer::CuDefaultGStreamer` uses 8 as well.
pub const CAMERA_RING_DEPTH: usize = 8;

/// The camera source: GStreamer running the RTSP pipeline from `copperconfig.ron`.
pub type RtspCamera = cu_gstreamer::CuGStreamer<CAMERA_RING_DEPTH>;
