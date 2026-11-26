# cu-skylock-payloads

Copper payloads for object detections and a single-target track state. The detector in
[`cu-skylock-detect`](../cu-skylock-detect) outputs them and the tracker in
[`cu-skylock-tracker`](../cu-skylock-tracker) takes and outputs them, but neither crate is
needed to use these types.

## Types

| Type | What it holds |
|---|---|
| `Detection` | One box (`x_min`, `y_min`, `x_max`, `y_max`) in source-image pixels, with a `confidence` and a COCO `class_id` |
| `Detections` | One frame's detections, most confident first, in a `CuArrayVec` of up to `MAX_DETECTIONS` (32) |
| `TrackStatus` | `Searching`, `Tracking`, `Coasting` or `Lost` |
| `TrackState` | The tracker's estimate for one frame: status, position and velocity (px, px/s), their variances, the last box's size, confidence and class, a lead point, and the hit, miss and late-drop counts |
| `COCO_CLASSES` | The 80 COCO class names, by class id |

Pixel coordinates run right and down from the image's top-left corner. Every payload has a
fixed size, so none allocates on a task's hot path.

In a Copper config, name the messages as `cu_skylock_payloads::Detections` and
`cu_skylock_payloads::TrackState`.

## Depending on it

The crates in this repository are pinned to one commit of Copper. A crate that uses them
must spell its own Copper dependencies with that same `git` and `rev`; the
[repository README](../../README.md#depending-on-the-crates) has the lines to copy.

## Licence

Apache-2.0.
