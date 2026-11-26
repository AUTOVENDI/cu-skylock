# cu-skylock-detect

A Copper task that detects objects in camera frames with YOLOv8, running on
[Candle](https://github.com/huggingface/candle).

| | |
|---|---|
| Task | `cu_skylock_detect::YoloDetect` |
| Input | `cu_sensor_payloads::CuImage<Vec<u8>>`, an NV12 frame |
| Output | `cu_skylock_payloads::Detections`: up to 32 boxes in source-image pixels, most confident first, each with a confidence and a COCO class id |

The task copies its input's time of validity onto its output. A cycle without a frame
outputs nothing.

## Config

| Key | Default | Meaning |
|---|---|---|
| `width`, `height` | required | The frames' size in pixels |
| `variant` | `"yolov8n"` | `yolov8n`, `yolov8s`, `yolov8m`, `yolov8l` or `yolov8x` |
| `conf_threshold` | 0.25 | The lowest confidence kept |
| `iou_threshold` | 0.7 | The overlap at which non-maximum suppression drops a box |
| `classes` | all | The COCO class ids to keep, such as `[0]` for people |

## CPU and GPU

Without features the task runs on the CPU. The `cuda` feature runs it on an NVIDIA GPU, and
a `cuda` build fails at start if it can't open GPU 0, rather than falling back to the CPU.
When the task stops it logs the percentiles of its time per frame; the
[repository README](../../README.md#measured) has the numbers measured so far.

## Weights

The task downloads `yolov8*.safetensors` from `lmz/candle-yolo-v8` on Hugging Face into the
local Hugging Face cache on its first run. This crate ships no weights. Ultralytics
publishes the YOLOv8 models under AGPL-3.0, with an Enterprise licence as the alternative,
so check which applies to you before you redistribute the weights or build a product on
them.

`src/yolo/model.rs` is adapted from Candle's `yolo-v8` example by way of Copper's
`examples/cu_human_pose`; see [NOTICE](../../NOTICE).

## Depending on it

The crates in this repository are pinned to one commit of Copper. A crate that uses them
must spell its own Copper dependencies with that same `git` and `rev`; the
[repository README](../../README.md#depending-on-the-crates) has the lines to copy.

## Licence

Apache-2.0 for this crate's code. The weights are not part of it; see above.
