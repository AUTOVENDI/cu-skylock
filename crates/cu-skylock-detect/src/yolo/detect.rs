//! Postprocessing for YOLOv8 detection: the best class of each anchor, a confidence
//! threshold, non-maximum suppression per class, and boxes mapped back onto the source image.
//!
//! The model's output has shape `(4 + classes, anchors)`. Rows 0 to 3 hold each anchor's box
//! as centre x, centre y, width and height in model-input pixels; the other rows hold the
//! class scores, already through a sigmoid. Decoding works in buffers that
//! [`DecodeScratch::new`] sizes once, so a frame doesn't allocate.

use candle_core::{Device, Storage, Tensor};
use cu_skylock_payloads::{Detection, Detections};
use cu29::prelude::*;

/// The most candidates that go into NMS; beyond it, the least confident are dropped first.
pub const MAX_NMS_CANDIDATES: usize = 1024;

/// How model-input coordinates map onto the source image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxMapping {
    /// Source pixels per model-input pixel, along x.
    pub scale_x: f32,
    /// Source pixels per model-input pixel, along y.
    pub scale_y: f32,
    /// Source image width in pixels; boxes are clipped to it.
    pub width: f32,
    /// Source image height in pixels; boxes are clipped to it.
    pub height: f32,
}

impl BoxMapping {
    /// The mapping for a source image of `source` (width, height) resized to `model`.
    pub fn new(source: (u32, u32), model: (usize, usize)) -> Self {
        Self {
            scale_x: source.0 as f32 / model.0 as f32,
            scale_y: source.1 as f32 / model.1 as f32,
            width: source.0 as f32,
            height: source.1 as f32,
        }
    }

    /// Maps a box given as centre and size in model-input pixels to its corners in source
    /// pixels, clipped to the image: `(x_min, y_min, x_max, y_max)`.
    pub fn to_source(&self, cx: f32, cy: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
        let x_min = ((cx - w / 2.0) * self.scale_x).clamp(0.0, self.width);
        let y_min = ((cy - h / 2.0) * self.scale_y).clamp(0.0, self.height);
        let x_max = ((cx + w / 2.0) * self.scale_x).clamp(0.0, self.width);
        let y_max = ((cy + h / 2.0) * self.scale_y).clamp(0.0, self.height);
        (x_min, y_min, x_max, y_max)
    }
}

/// Decoding settings.
#[derive(Debug, Clone, Copy)]
pub struct DecodeParams<'a> {
    pub conf_threshold: f32,
    /// Boxes of the same class that overlap a more confident one by more than this IoU are
    /// dropped.
    pub iou_threshold: f32,
    /// The class ids to keep; every class when empty.
    pub classes: &'a [u16],
    pub mapping: BoxMapping,
}

#[derive(Debug, Clone, Copy, Default)]
struct Candidate {
    detection: Detection,
    suppressed: bool,
}

/// Buffers for decoding one model output with a fixed number of anchors.
pub struct DecodeScratch {
    anchors: usize,
    best_score: Vec<f32>,
    best_class: Vec<u16>,
    candidates: Vec<Candidate>,
}

impl DecodeScratch {
    pub fn new(anchors: usize) -> Self {
        Self {
            anchors,
            best_score: vec![0.0; anchors],
            best_class: vec![0; anchors],
            candidates: Vec::with_capacity(anchors),
        }
    }

    pub fn anchors(&self) -> usize {
        self.anchors
    }
}

/// The number of anchors YOLOv8 produces for a model input of `width` x `height`: one per
/// cell of its stride-8, -16 and -32 grids.
pub fn anchor_count(width: usize, height: usize) -> usize {
    [8, 16, 32]
        .iter()
        .map(|stride| (width / stride) * (height / stride))
        .sum()
}

/// Decodes a model output of shape `(1, rows, anchors)` or `(rows, anchors)`, on any device,
/// into `out`. A GPU output is copied to the CPU first, which allocates inside Candle.
pub fn decode_tensor(
    pred: &Tensor,
    params: &DecodeParams<'_>,
    scratch: &mut DecodeScratch,
    out: &mut Detections,
) -> CuResult<()> {
    let candle = |e: candle_core::Error| CuError::new_with_cause("Cannot read the model output", e);
    let pred = if pred.rank() == 3 {
        pred.squeeze(0).map_err(candle)?
    } else {
        pred.clone()
    };
    let pred = if pred.device().is_cpu() {
        pred
    } else {
        pred.to_device(&Device::Cpu).map_err(candle)?
    };
    let pred = pred.contiguous().map_err(candle)?;
    let (rows, _) = pred.dims2().map_err(candle)?;
    let (storage, layout) = pred.storage_and_layout();
    let Storage::Cpu(cpu) = &*storage else {
        return Err(CuError::from("The model output isn't on the CPU"));
    };
    let data = cpu.as_slice::<f32>().map_err(candle)?;
    let (start, end) = layout
        .contiguous_offsets()
        .ok_or_else(|| CuError::from("The model output isn't contiguous"))?;
    decode(&data[start..end], rows, params, scratch, out)
}

/// Decodes one model output, a row-major `(rows, anchors)` slice, into `out`: at most
/// `MAX_DETECTIONS` boxes, most confident first.
pub fn decode(
    pred: &[f32],
    rows: usize,
    params: &DecodeParams<'_>,
    scratch: &mut DecodeScratch,
    out: &mut Detections,
) -> CuResult<()> {
    let n = scratch.anchors;
    if rows < 5 || pred.len() != rows * n {
        return Err(CuError::from(format!(
            "Unexpected model output: {} values for {rows} rows of {n} anchors",
            pred.len()
        )));
    }
    let classes = rows - 4;

    // The best allowed class of each anchor, one class row at a time.
    scratch.best_score.fill(0.0);
    scratch.best_class.fill(0);
    let mut scan = |class: u16| {
        let index = 4 + class as usize;
        if index >= rows {
            return;
        }
        let row = &pred[index * n..(index + 1) * n];
        for ((best, best_class), &score) in scratch
            .best_score
            .iter_mut()
            .zip(scratch.best_class.iter_mut())
            .zip(row)
        {
            if score > *best {
                *best = score;
                *best_class = class;
            }
        }
    };
    if params.classes.is_empty() {
        (0..classes as u16).for_each(&mut scan);
    } else {
        params.classes.iter().copied().for_each(&mut scan);
    }

    let (cx, rest) = pred.split_at(n);
    let (cy, rest) = rest.split_at(n);
    let (w, rest) = rest.split_at(n);
    let h = &rest[..n];
    let boxes = cx.iter().zip(cy).zip(w).zip(h);
    let scores = scratch.best_score.iter().zip(&scratch.best_class);
    let candidates = &mut scratch.candidates;
    candidates.clear();
    for ((((&cx, &cy), &w), &h), (&confidence, &class_id)) in boxes.zip(scores) {
        if confidence < params.conf_threshold {
            continue;
        }
        let (x_min, y_min, x_max, y_max) = params.mapping.to_source(cx, cy, w, h);
        if x_max <= x_min || y_max <= y_min {
            continue;
        }
        candidates.push(Candidate {
            detection: Detection {
                x_min,
                y_min,
                x_max,
                y_max,
                confidence,
                class_id,
            },
            suppressed: false,
        });
    }

    let by_confidence =
        |a: &Candidate, b: &Candidate| b.detection.confidence.total_cmp(&a.detection.confidence);
    if candidates.len() > MAX_NMS_CANDIDATES {
        candidates.select_nth_unstable_by(MAX_NMS_CANDIDATES - 1, by_confidence);
        candidates.truncate(MAX_NMS_CANDIDATES);
    }
    candidates.sort_unstable_by(by_confidence);

    let CuArrayVec(items) = &mut out.items;
    items.clear();
    for i in 0..candidates.len() {
        if candidates[i].suppressed {
            continue;
        }
        let kept = candidates[i].detection;
        if items.try_push(kept).is_err() {
            break;
        }
        for other in &mut candidates[i + 1..] {
            if !other.suppressed
                && other.detection.class_id == kept.class_id
                && iou(&kept, &other.detection) > params.iou_threshold
            {
                other.suppressed = true;
            }
        }
    }
    Ok(())
}

/// Intersection over union of two boxes.
fn iou(a: &Detection, b: &Detection) -> f32 {
    let w = (a.x_max.min(b.x_max) - a.x_min.max(b.x_min)).max(0.0);
    let h = (a.y_max.min(b.y_max) - a.y_min.max(b.y_min)).max(0.0);
    let intersection = w * h;
    let (aw, ah) = a.size();
    let (bw, bh) = b.size();
    let union = aw * ah + bw * bh - intersection;
    if union > 0.0 {
        intersection / union
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cu_skylock_payloads::MAX_DETECTIONS;

    /// A model output of `classes` classes over `anchors` anchors, all scores 0.
    struct Output {
        anchors: usize,
        data: Vec<f32>,
    }

    impl Output {
        fn new(classes: usize, anchors: usize) -> Self {
            Self {
                anchors,
                data: vec![0.0; (4 + classes) * anchors],
            }
        }

        fn set(
            &mut self,
            anchor: usize,
            (cx, cy, w, h): (f32, f32, f32, f32),
            class: usize,
            score: f32,
        ) {
            let n = self.anchors;
            for (row, value) in [cx, cy, w, h].into_iter().enumerate() {
                self.data[row * n + anchor] = value;
            }
            self.data[(4 + class) * n + anchor] = score;
        }

        fn tensor(&self) -> Tensor {
            let rows = self.data.len() / self.anchors;
            Tensor::from_vec(self.data.clone(), (1, rows, self.anchors), &Device::Cpu).unwrap()
        }
    }

    fn params(classes: &[u16]) -> DecodeParams<'_> {
        DecodeParams {
            conf_threshold: 0.25,
            iou_threshold: 0.7,
            classes,
            mapping: BoxMapping::new((640, 352), (640, 352)),
        }
    }

    fn run(output: &Output, params: &DecodeParams<'_>) -> Detections {
        let mut scratch = DecodeScratch::new(output.anchors);
        let mut out = Detections::default();
        decode_tensor(&output.tensor(), params, &mut scratch, &mut out).unwrap();
        out
    }

    #[test]
    fn a_synthetic_output_tensor_decodes_with_nms_per_class() {
        let mut output = Output::new(3, 6);
        output.set(0, (100.0, 100.0, 40.0, 40.0), 0, 0.90);
        // Overlaps anchor 0 (IoU 0.86) with the same class: suppressed.
        output.set(1, (102.0, 101.0, 40.0, 40.0), 0, 0.80);
        // Overlaps anchor 0 too, but as another class: kept.
        output.set(2, (101.0, 100.0, 40.0, 40.0), 1, 0.85);
        // Below the confidence threshold.
        output.set(3, (300.0, 200.0, 20.0, 20.0), 2, 0.20);
        // Far from the others.
        output.set(4, (500.0, 50.0, 30.0, 60.0), 0, 0.50);
        let out = run(&output, &params(&[]));
        let got: Vec<(u16, f32)> = out.iter().map(|d| (d.class_id, d.confidence)).collect();
        assert_eq!(got, vec![(0, 0.90), (1, 0.85), (0, 0.50)]);
        let first = out.iter().next().unwrap();
        assert_eq!(
            (first.x_min, first.y_min, first.x_max, first.y_max),
            (80.0, 80.0, 120.0, 120.0)
        );
    }

    #[test]
    fn the_class_filter_keeps_only_listed_classes() {
        let mut output = Output::new(3, 4);
        output.set(0, (100.0, 100.0, 40.0, 40.0), 0, 0.90);
        output.set(1, (300.0, 100.0, 40.0, 40.0), 1, 0.80);
        // Scores for two classes on one anchor: with the filter, the lower allowed one wins.
        output.set(2, (500.0, 100.0, 40.0, 40.0), 0, 0.95);
        output.data[(4 + 1) * 4 + 2] = 0.60;
        let out = run(&output, &params(&[1]));
        let got: Vec<(u16, f32)> = out.iter().map(|d| (d.class_id, d.confidence)).collect();
        assert_eq!(got, vec![(1, 0.80), (1, 0.60)]);
    }

    #[test]
    fn boxes_map_back_onto_the_source_frame() {
        // A 640x360 frame goes into the model as 640x352 (preprocess::model_input_dims).
        let mapping = BoxMapping::new((640, 360), (640, 352));
        let (x_min, y_min, x_max, y_max) = mapping.to_source(320.0, 176.0, 64.0, 32.0);
        assert_eq!((x_min, x_max), (288.0, 352.0));
        let scale = 360.0 / 352.0;
        assert!((y_min - 160.0 * scale).abs() < 1e-4);
        assert!((y_max - 192.0 * scale).abs() < 1e-4);
        // The model's bottom edge lands on the frame's, and boxes past the edges are clipped.
        let (_, _, _, bottom) = mapping.to_source(320.0, 342.0, 10.0, 20.0);
        assert!((bottom - 360.0).abs() < 1e-3);
        let (_, _, _, bottom) = mapping.to_source(320.0, 350.0, 10.0, 20.0);
        assert_eq!(bottom, 360.0);
        let (left, top, _, _) = mapping.to_source(-5.0, -5.0, 20.0, 20.0);
        assert_eq!((left, top), (0.0, 0.0));
    }

    #[test]
    fn the_output_keeps_the_most_confident_up_to_capacity() {
        let anchors = MAX_DETECTIONS + 8;
        let mut output = Output::new(1, anchors);
        for i in 0..anchors {
            // Separate boxes on a 10-column grid, confidence rising with the index.
            let (col, row) = ((i % 10) as f32, (i / 10) as f32);
            let confidence = 0.30 + 0.01 * i as f32;
            output.set(
                i,
                (20.0 + 60.0 * col, 20.0 + 60.0 * row, 30.0, 30.0),
                0,
                confidence,
            );
        }
        let out = run(&output, &params(&[]));
        assert_eq!(out.len(), MAX_DETECTIONS);
        let confidences: Vec<f32> = out.iter().map(|d| d.confidence).collect();
        assert!(confidences.windows(2).all(|w| w[0] >= w[1]));
        assert!((confidences[MAX_DETECTIONS - 1] - (0.30 + 0.01 * 8.0)).abs() < 1e-6);
    }

    #[test]
    fn a_wrong_output_shape_is_an_error() {
        let output = Output::new(3, 6);
        let mut scratch = DecodeScratch::new(5);
        let mut out = Detections::default();
        assert!(decode_tensor(&output.tensor(), &params(&[]), &mut scratch, &mut out).is_err());
    }

    #[test]
    fn anchors_for_the_640x352_input() {
        assert_eq!(anchor_count(640, 352), 80 * 44 + 40 * 22 + 20 * 11);
        assert_eq!(anchor_count(640, 640), 8400);
    }
}
