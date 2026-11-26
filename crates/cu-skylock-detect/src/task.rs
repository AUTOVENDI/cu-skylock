//! YOLOv8 object detection: an NV12 `CuImage` in, `Detections` out.
//!
//! Config:
//! - `width`, `height` (required): the frames' size.
//! - `variant`: `yolov8n` (default), `yolov8s`, `yolov8m`, `yolov8l` or `yolov8x`. The weights
//!   come from Hugging Face (`lmz/candle-yolo-v8`) into its local cache on the first run.
//! - `conf_threshold` (default 0.25), `iou_threshold` (default 0.7, as Ultralytics).
//! - `classes`: the COCO class ids to keep, such as `[0]` for people; all when left out.
//!
//! A `cuda` build fails at start if it can't open GPU 0, rather than running on the CPU.
//! When the task stops, it logs the percentiles of its time per frame.

use crate::yolo::detect::{BoxMapping, DecodeParams, DecodeScratch, anchor_count, decode_tensor};
use crate::yolo::preprocess::{model_input_dims, nv12_to_planar_rgb};
use crate::yolo::{Multiples, YoloV8};
use candle_core::{DType, Device, Tensor};
use candle_nn::{Module, VarBuilder};
use cu_sensor_payloads::CuImage;
use cu_skylock_payloads::{COCO_CLASSES, Detections};
use cu29::prelude::*;
use skylock_timing::DurationHistogram;
use std::time::Instant;

#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct YoloDetect {
    #[reflect(ignore)]
    model: YoloV8,
    #[reflect(ignore)]
    device: Device,
    conf_threshold: f32,
    iou_threshold: f32,
    #[reflect(ignore)]
    classes: Vec<u16>,
    width: u32,
    height: u32,
    model_width: usize,
    model_height: usize,
    #[reflect(ignore)]
    mapping: BoxMapping,
    /// The model input, refilled for each frame.
    #[reflect(ignore)]
    input: Vec<f32>,
    #[reflect(ignore)]
    scratch: DecodeScratch,
    /// Time per frame in `process()`, in 100 µs buckets up to 1 s.
    #[reflect(ignore)]
    timing: DurationHistogram,
}

// No state carries from one frame to the next.
impl Freezable for YoloDetect {}

impl CuTask for YoloDetect {
    type Resources<'r> = ();
    type Input<'m> = input_msg!(CuImage<Vec<u8>>);
    type Output<'m> = output_msg!(Detections);

    fn new(config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self>
    where
        Self: Sized,
    {
        let config = config.ok_or_else(|| CuError::from("YoloDetect requires configuration"))?;
        let width = config
            .get::<u32>("width")?
            .ok_or_else(|| CuError::from("YoloDetect requires width"))?;
        let height = config
            .get::<u32>("height")?
            .ok_or_else(|| CuError::from("YoloDetect requires height"))?;
        let variant = config
            .get::<String>("variant")?
            .unwrap_or_else(|| "yolov8n".to_string());
        let conf_threshold = config.get::<f64>("conf_threshold")?.unwrap_or(0.25) as f32;
        let iou_threshold = config.get::<f64>("iou_threshold")?.unwrap_or(0.7) as f32;
        let classes = config.get_value::<Vec<u16>>("classes")?.unwrap_or_default();
        if let Some(&bad) = classes
            .iter()
            .find(|&&c| usize::from(c) >= COCO_CLASSES.len())
        {
            return Err(CuError::from(format!(
                "YoloDetect: class {bad} isn't one of the {} COCO classes",
                COCO_CLASSES.len()
            )));
        }

        let multiples = match variant.as_str() {
            "yolov8n" => Multiples::n(),
            "yolov8s" => Multiples::s(),
            "yolov8m" => Multiples::m(),
            "yolov8l" => Multiples::l(),
            "yolov8x" => Multiples::x(),
            _ => {
                return Err(CuError::from(format!(
                    "Unknown variant: {variant}. Use yolov8[n|s|m|l|x]"
                )));
            }
        };

        #[cfg(feature = "cuda")]
        let device = Device::new_cuda(0).map_err(|e| {
            CuError::new_with_cause("This cuda build of YoloDetect can't open GPU 0", e)
        })?;
        #[cfg(not(feature = "cuda"))]
        let device = Device::Cpu;

        let weights = download_weights(&format!("{variant}.safetensors"))?;
        // SAFETY: the safetensors file is memory-mapped and only read by VarBuilder.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)
                .map_err(|e| CuError::new_with_cause("Failed to load model weights", e))?
        };
        let model = YoloV8::load(vb, multiples, COCO_CLASSES.len())
            .map_err(|e| CuError::new_with_cause("Failed to build the YOLOv8 model", e))?;

        let (model_width, model_height) = model_input_dims(width as usize, height as usize);
        info!(
            "yolo_detect: {} on the {}, {}x{} frames in as {}x{}",
            variant.as_str(),
            device_name(&device),
            width,
            height,
            model_width,
            model_height
        );
        Ok(Self {
            model,
            device,
            conf_threshold,
            iou_threshold,
            classes,
            width,
            height,
            model_width,
            model_height,
            mapping: BoxMapping::new((width, height), (model_width, model_height)),
            input: vec![0.0; 3 * model_width * model_height],
            scratch: DecodeScratch::new(anchor_count(model_width, model_height)),
            timing: DurationHistogram::new(100_000, 10_000),
        })
    }

    /// Runs the model once on a blank frame, so that loading kernels doesn't land on the
    /// first camera frame, and checks the output's shape.
    fn start(&mut self, _ctx: &CuContext) -> CuResult<()> {
        let started = Instant::now();
        let shape = (1, 3, self.model_height, self.model_width);
        let pred = Tensor::zeros(shape, DType::F32, &self.device)
            .and_then(|blank| self.model.forward(&blank))
            .and_then(|pred| pred.to_device(&Device::Cpu))
            .map_err(|e| CuError::new_with_cause("YoloDetect warm-up failed", e))?;
        let expected = (1, 4 + COCO_CLASSES.len(), self.scratch.anchors());
        if pred.dims3().ok() != Some(expected) {
            return Err(CuError::from(format!(
                "YoloDetect: the model output is {:?}, not {expected:?}",
                pred.dims()
            )));
        }
        info!(
            "yolo_detect: warm-up inference took {} ms",
            started.elapsed().as_millis() as u64
        );
        Ok(())
    }

    fn process(
        &mut self,
        ctx: &CuContext,
        input: &Self::Input<'_>,
        output: &mut Self::Output<'_>,
    ) -> CuResult<()> {
        let Some(image) = input.payload() else {
            output.clear_payload();
            return Ok(());
        };
        let started = ctx.now();
        let format = &image.format;
        if format.width != self.width
            || format.height != self.height
            || &format.pixel_format != b"NV12"
        {
            return Err(CuError::from(
                "YoloDetect: the frame isn't NV12 at the configured width and height",
            ));
        }
        let geometry = (
            self.width as usize,
            self.height as usize,
            format.stride as usize,
        );
        let model_size = (self.model_width, self.model_height);
        let filled = image.buffer_handle.with_inner(|data| {
            if data.len() < geometry.2 * geometry.1 * 3 / 2 {
                return false;
            }
            nv12_to_planar_rgb(data, geometry, &mut self.input, model_size);
            true
        });
        if !filled {
            return Err(CuError::from("YoloDetect: the frame is shorter than NV12"));
        }

        let shape = (1, 3, self.model_height, self.model_width);
        let pred = Tensor::from_slice(&self.input, shape, &self.device)
            .and_then(|tensor| self.model.forward(&tensor))
            .map_err(|e| CuError::new_with_cause("Model inference failed", e))?;
        let params = DecodeParams {
            conf_threshold: self.conf_threshold,
            iou_threshold: self.iou_threshold,
            classes: &self.classes,
            mapping: self.mapping,
        };
        let mut detections = Detections::default();
        decode_tensor(&pred, &params, &mut self.scratch, &mut detections)?;
        output.tov = input.tov;
        output.set_payload(detections);
        self.timing.record((ctx.now() - started).as_nanos());
        Ok(())
    }

    fn stop(&mut self, _ctx: &CuContext) -> CuResult<()> {
        if let Some(t) = self.timing.summary() {
            info!(
                "yolo_detect: {} frames, time per frame p50 {} us, p95 {} us, p99 {} us, max {} us",
                t.samples, t.p50_us, t.p95_us, t.p99_us, t.max_us
            );
        }
        Ok(())
    }
}

fn device_name(device: &Device) -> &'static str {
    if device.is_cuda() { "GPU" } else { "CPU" }
}

/// The path of `filename` from `lmz/candle-yolo-v8`, downloaded into the Hugging Face cache
/// if it isn't there yet. The weights stay out of the repo.
fn download_weights(filename: &str) -> CuResult<std::path::PathBuf> {
    let client = hf_hub::HFClientSync::new()
        .map_err(|e| CuError::new_with_cause("Failed to create HuggingFace API", e))?;
    client
        .model("lmz", "candle-yolo-v8")
        .download_file()
        .filename(filename.to_string())
        .send()
        .map_err(|e| CuError::new_with_cause("Failed to download model weights", e))
}
