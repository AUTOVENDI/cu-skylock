# cu-skylock

Copper tasks for camera-to-track perception: a YOLOv8 object detector and a single-target
Kalman tracker, with the payloads they exchange and an example app that runs them on an
RTSP camera. They come from SKYLOCK, WhiteHat Design Studio's drone detection and tracking
ground station, whose tracking pipeline is being moved onto
[Copper](https://github.com/copper-project/copper-rs).

```text
camera ──► gst_to_image ──► detector ──► tracker ──► viz
(RTSP)     (CuImage)        (YOLOv8)     (Kalman)    (Rerun; also takes the image
                                                      and the detections)
```

## What this is, and isn't yet

- The detector runs stock YOLOv8n with the COCO classes. COCO has no drone class, so the
  bench target so far is a person. SKYLOCK's own model isn't here.
- The tracker follows one target at a time and picks it by confidence. There is no target
  selection, no re-identification and no gimbal output.
- Everything was built and measured on Windows 11 only. Nothing in the crates is tied to
  Windows, but Linux and macOS are untested.
- The example app has no replay binary and doesn't reconnect a lost camera.

[BREAKLOG.md](BREAKLOG.md) lists every problem met on the way, each marked as Copper's or
ours, with the state of the ones reported upstream.

## Crates

| Crate | Kind | In | Out |
|---|---|---|---|
| [`cu-skylock-payloads`](crates/cu-skylock-payloads) | payloads | | `Detection`, `Detections`, `TrackStatus`, `TrackState`, `COCO_CLASSES` |
| [`cu-skylock-detect`](crates/cu-skylock-detect) | task `YoloDetect` | `cu_sensor_payloads::CuImage<Vec<u8>>` (NV12) | `cu_skylock_payloads::Detections` |
| [`cu-skylock-tracker`](crates/cu-skylock-tracker) | task `KalmanTracker` | `cu_skylock_payloads::Detections` | `cu_skylock_payloads::TrackState` |

The detector and the tracker don't depend on each other; both depend on the payloads.
`crates/skylock-timing` is a small internal crate for the tasks' timing percentiles.

**`YoloDetect` config:** `width` and `height` (required, the frames' size), `variant`
(`yolov8n` by default, up to `yolov8x`), `conf_threshold` (0.25), `iou_threshold` (0.7) and
`classes` (the COCO class ids to keep; all when left out). The `cuda` feature runs it on an
NVIDIA GPU.

**`KalmanTracker` config**, all optional: `confirm_hits` (3), `gate_chi2` (9.21),
`max_coast_ms` (1000), `lead_ms` (0), `meas_std_px` (5), `accel_psd` (20000 px²/s³) and
`init_vel_std` (300 px/s). The values are estimates, not tuned.

Each crate's README says more.

## Depending on the crates

The crates are pinned to one commit of Copper's master and are not on crates.io:

```toml
[dependencies]
cu-skylock-payloads = { git = "https://github.com/AUTOVENDI/cu-skylock", branch = "main" }
cu-skylock-detect = { git = "https://github.com/AUTOVENDI/cu-skylock", branch = "main" }
cu-skylock-tracker = { git = "https://github.com/AUTOVENDI/cu-skylock", branch = "main" }
cu29 = { git = "https://github.com/copper-project/copper-rs", rev = "cdfb08ec1e2f195ff7eb0be1855e879a83d76387" }
cu-sensor-payloads = { git = "https://github.com/copper-project/copper-rs", rev = "cdfb08ec1e2f195ff7eb0be1855e879a83d76387" }
```

**Spell Copper exactly as above.** Cargo identifies a git dependency by its URL and `rev`
as written, not by the commit they resolve to. A `branch`, a `tag`, a short hash or a
different URL for `cu29` is a second copy of Copper, and its `CuMsg` is a different type
from the one these crates use, so your CopperList won't type-check. The same goes for the
three crates of this repository: give them all the same `git` and `branch`. When this
repository moves to a newer Copper commit, the lines above change with it.

## Wiring

In the Copper config, the tasks and messages are named by their crate paths:

```ron
tasks: [
    // ... a source and a task that outputs cu_sensor_payloads::CuImage<Vec<u8>> as "image"
    (
        id: "detector",
        type: "cu_skylock_detect::YoloDetect",
        config: { "width": 640, "height": 360, "classes": [0] },
    ),
    (
        id: "tracker",
        type: "cu_skylock_tracker::KalmanTracker",
        config: { "max_coast_ms": 1000 },
    ),
],
cnx: [
    (src: "image", dst: "detector", msg: "cu_sensor_payloads::CuImage<Vec<u8>>"),
    (src: "detector", dst: "tracker", msg: "cu_skylock_payloads::Detections"),
    // ... then something that takes cu_skylock_payloads::TrackState from "tracker"
],
```

[`examples/skylock_tracker/copperconfig.ron`](examples/skylock_tracker/copperconfig.ron) is
a complete graph.

## Run the example

You need Rust 1.98 or newer, GStreamer with its development files where `pkg-config` finds
them (tested with 1.28.9), and a Rerun viewer 0.43.x (`rerun`) on `PATH`. The `cuda` feature
needs an NVIDIA GPU and a CUDA toolkit no newer than 13.4 (tested with 12.9). On Windows,
[WINDOWS_SETUP.md](WINDOWS_SETUP.md) installs and checks the tools, and shows how to start a
session with the camera URL set.

Run from the example's directory, because the config and the log paths are relative:

```bash
cd examples/skylock_tracker
export SKYLOCK_RTSP_URL='rtsp://...'   # the camera, required
cargo run -r --features cuda           # or `cargo run -r` for the CPU
```

From the repository root, `cargo build` and `cargo test` cover the crates only and need no
GStreamer; add `-p skylock-tracker` or `--workspace` for the app.

| Variable | What it does |
|---|---|
| `SKYLOCK_RTSP_URL` | The camera's `rtsp://` URL. The app exits if it is missing or not `rtsp`, without printing it. |
| `RERUN=off` | Runs without the viewer. |
| `GST_DEBUG=2` | Prints GStreamer's warnings and errors, which the camera source doesn't report. |

Ctrl+C stops the app and closes its log (on Windows, Ctrl+Break does too) once it has
started. In its first seconds, while the tasks are being built, Ctrl+C ends it with the log
left open, and `fsck` then refuses that log.

The pipeline in `copperconfig.ron` expects H.265 (`rtph265depay ! h265parse ! avdec_h265`);
for an H.264 camera, use the `h264` elements. The app reads the config when it starts, so a
change to a task's settings needs no rebuild; a change to the graph does.

The camera source's `frame_mode` is `"fifo"`, Copper's default: every frame reaches the
tracker, oldest first. `"newest"` hands over only the latest frame and drops the ones
queued behind it. In the two 5-minute runs under [Measured](#measured) that made frames
fresher (they waited 24 ms before the loop at the 95th percentile, against 78 ms) but
dropped 451 frames against 7, because frames queue up while the loop waits on the log,
and the tracker held its target in fewer frames (368 against 505). So the example
stays on `fifo`.

### The camera URL and the log

The app puts the URL into the camera task's config in memory only, and asks Copper not
to store the effective config in the log (`save_effective_config(false)` on the app
builder). The camera source no longer writes its pipeline to the text log either. Checked
on this build with a made-up URL that held a password: a search of the log's bytes for
the password, as text, as UTF-16 and as Base64, finds nothing, and neither does a search
for `rtsp://`.

The console is separate from the log: with `GST_DEBUG` set, what GStreamer prints can
name the camera. Read any output before you share it.

Logs are large: about 12 GB per 10 minutes of camera, because each frame is logged twice,
in 256 MiB slab files that NTFS allocates in full. Each run replaces the last run's log.

To read one, with the log reader that `cargo build -r -p skylock-tracker` builds:

```bash
../../target/release/skylock-tracker-logreader logs/skylock.copper \
    extract-text-log ../../target/release/cu29_log_index
../../target/release/skylock-tracker-logreader logs/skylock.copper fsck
```

## Measured

On a Windows 11 laptop with an Intel i7-14700HX, an NVIDIA RTX 5060 Laptop GPU (8 GB),
CUDA 12.9, GStreamer 1.28.9 and Rust 1.100. Each row names the run and the Copper commit it
came from; [BREAKLOG.md](BREAKLOG.md) has each measurement in full.

| What | Result | Run |
|---|---|---|
| Detector, YOLOv8n on 640×352, CUDA | p50 14.1 and 14.2 ms, p95 18.0 and 18.1 ms a frame | Two 5-minute runs on a synthetic 1080p 30 fps H.265 stream, Copper `cdfb08e`, 2026-11-26 |
| Tracker | p50 1 µs, p99 2 µs a frame | Same two runs |
| Frames in 5 minutes, `fifo` | 8,622 of about 8,629; the source counted 7 dropped; frames waited p50 38 ms, p95 78 ms before the loop | The `fifo` run of the two |
| Frames in 5 minutes, `newest` | 8,135 of about 8,541; the source counted 451 dropped; frames waited p50 16 ms, p95 24 ms | The `newest` run of the two |
| Gaps of 100 ms or more between frames in 5 minutes | 71 with `fifo` and 63 with `newest`, the longest 500 and 607 ms | Same two runs; inline log output |
| Log | 6.2 GB per 5 minutes; `fsck` passes | The `fifo` run |
| Loop stalls in 10 minutes, inline log output | 197 gaps of 100 to 675 ms, at the log's section seals | Synthetic stream, the example as it was in a fork of copper-rs at `cd321a8`, 2026-11-25 |
| The same with `async-cl-io` | 2 gaps of 100 ms or more (7 in the count-32 run, six of them one burst of slow inference); CopperLists missing from the log: 28 % at `copperlist_count` 2, 1.7 % at 8, 0.5 % at 32, none at 128 | Same build and stream, 2026-11-25 and 2026-11-26 |
| Detector on SKYLOCK's camera, CUDA | p50 12.5 to 13.3 ms, p95 14.5 to 21.5 ms a frame, over four runs | H.265 1080p 30 fps camera, `cd321a8`, 2026-11-23 |
| Detector on the CPU | p50 200 ms, p95 211 ms a frame; the stream falls to about 5 fps | Synthetic stream, `cd321a8`, 2026-11-22 |
| Live tracking, a person walking, hiding and leaving | 37 detection dropouts of 26 to 500 ms bridged; 9 losses, each about 1 s after the last detection, all re-acquired | Camera, `cd321a8`, 2026-11-23 |
| Frames in 10 minutes on the camera, viewer on | 17,546 of about 17,588 (0.24 % dropped), longest gap 371 ms | Camera, `cd321a8`, when the example still ran the pose model, 2026-11-20 |
| Viewer memory | 1.8 to 2.2 GB, with its 2 GB limit | `cd321a8`, 2026-11-22 |
| Clock drift | Copper's clock about 65 ppm against GStreamer's; the camera's about 470 ppm | `cd321a8`, 2026-11-20 |

In the two runs on `cdfb08e` the detector kept every COCO class, so that the tracker had
the test stream's moving ball to follow; the committed config keeps people only. YOLOv8n
sees that ball in a minority of frames, so those runs compare the two frame modes and say
little about tracking quality. The camera rows come from the same code before it moved
here; they haven't been repeated on `cdfb08e`.

There is no camera-to-track latency figure: frames are stamped when the source pops them, so
the time the camera took each one isn't known. There is no comparison with the Python
SKYLOCK yet.

## Known problems

- A lost RTSP session never comes back, and nothing reports it ([#1680](https://github.com/copper-project/copper-rs/issues/1680)): after a
  10 s cable pull the app ran on without frames. In a live test the stream also stopped
  twice mid-run with no error; the cause is unknown. Run with `GST_DEBUG=2` to see
  GStreamer's reason.
- While frames are logged, the loop stalls for 100 to 675 ms at the log's section seals:
  sealing a section flushes the whole slab, and waits for the disk, on the loop thread
  (seen on Windows). Copper's `async-cl-io` feature moves the flush to a writer thread,
  and then the loop runs free, but CopperLists are dropped from the log unless
  `logging.copperlist_count` is large enough to outlast the longest flush: 128 in our
  runs, with `gst_to_image`'s `pool_size` at 130. The example keeps Copper's default
  inline output. BREAKLOG.md has the runs.
- A frame's `tov` is when the source pops it ([#1683](https://github.com/copper-project/copper-rs/issues/1683)). A camera's start-up burst
  of old frames is tracked as if it were live, with velocities that are too high, and
  frames that wait out a stall arrive bunched.
- Each frame is logged twice, by the camera source and by `gst_to_image`.
- Ctrl+C during start-up leaves the log open ([#1682](https://github.com/copper-project/copper-rs/issues/1682)).
- On Windows each 256 MiB log slab takes its full size on disk at once
  ([#1685](https://github.com/copper-project/copper-rs/issues/1685)).

## The weights and their licence

The first run downloads `yolov8n.safetensors` from Hugging Face (`lmz/candle-yolo-v8`) into
the local Hugging Face cache; this repository holds no weights. YOLOv8's weights are
Ultralytics', under AGPL-3.0 unless an Enterprise licence covers you. This repository's own
code is Apache-2.0 (see [LICENSE](LICENSE) and [NOTICE](NOTICE)).

---

Built by [WhiteHat Design Studio](https://www.whitehatdesignstudio.com).
