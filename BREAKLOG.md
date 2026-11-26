# SKYLOCK port: break log

One entry per problem met while moving SKYLOCK's tracking pipeline onto Copper, written
when it happens. Doc and skill drift and perf debt go here too.
Each entry's **Upstream?** line starts with *Upstream*, naming the project or crate where a fix
belongs, or *Ours*, for this port, this machine or this camera. Some entries name
local helper scripts, which aren't in the repository, and "the plan" is the port's brief,
which isn't published.
Milestones in the headings: M0 is the baseline build of the pose example, M1 the RTSP camera,
M2a the YOLOv8n detector, M3 the Kalman tracker and M6 the hand-off; M5, the logging,
replay and camera-source fixes, was parked.
The entries up to 2026-11-23 were written in a fork of copper-rs, where this work was the
example `examples/cu_skylock_tracker` on Copper `cd321a8`; paths and task names in them are
that example's. Since then the detector and the tracker have moved into this repository's
crates, on a newer Copper.

## Status of what was reported upstream

As of 2026-11-26. The entries below stand as they were written; this table says what became
of them.

| Entry | Reported as | State |
|---|---|---|
| The camera source writes its pipeline, URL included, to the text log | privately | Fixed: [`3a2f849`](https://github.com/copper-project/copper-rs/commit/3a2f849a139) takes the pipeline out of the start-up message, and [#1664](https://github.com/copper-project/copper-rs/pull/1664) lets an app keep its effective config out of the log |
| Under Cargo 1.100 the log string index moves to `target/<profile>/build/` | not reported | Fixed by [#1668](https://github.com/copper-project/copper-rs/pull/1668) |
| Copper's integer config reads wrap negative values | not reported | Fixed by [#1665](https://github.com/copper-project/copper-rs/pull/1665) |
| The root justfile breaks when the repo path contains a space | [#1679](https://github.com/copper-project/copper-rs/pull/1679), our pull request | Merged |
| `just fmt` overflows the Windows command line in `cargo fmt --all` | [#1690](https://github.com/copper-project/copper-rs/issues/1690) | Fixed by [#1693](https://github.com/copper-project/copper-rs/pull/1693) |
| Clippy 1.100 fails `cu_consolemon`, so the milestone clippy check fails | [#1691](https://github.com/copper-project/copper-rs/issues/1691) | Fixed by [#1692](https://github.com/copper-project/copper-rs/pull/1692) |
| The source's ring of 8 hands out stale frames when the loop is slow | [#1681](https://github.com/copper-project/copper-rs/issues/1681) | Fixed by [#1707](https://github.com/copper-project/copper-rs/pull/1707): `frame_mode` and a count of dropped frames |
| An unreachable camera fails silently; the drop test; the stream that stopped twice | [#1680](https://github.com/copper-project/copper-rs/issues/1680) | Open |
| Ctrl+C in the app's first seconds kills it before it closes its log | [#1682](https://github.com/copper-project/copper-rs/issues/1682) | Open |
| The camera's start-up burst of old frames passes as live (`tov` is the pop time) | [#1683](https://github.com/copper-project/copper-rs/issues/1683) | Open |
| The source's `debug!` line on every pass without a frame | [#1684](https://github.com/copper-project/copper-rs/issues/1684) | Open |
| Each run takes 1 GB of disk: log slabs are not sparse on NTFS | [#1685](https://github.com/copper-project/copper-rs/issues/1685) | Open |
| `gen_cumsgs!` can't resolve the pose example's message path | [#1686](https://github.com/copper-project/copper-rs/issues/1686) | Open |
| `log-stats` per-edge bytes count memory, not log bytes | [#1687](https://github.com/copper-project/copper-rs/issues/1687) | Open |
| A `CuArrayVec` payload needs a hint to derive `Decode`, and has no `len` | [#1688](https://github.com/copper-project/copper-rs/issues/1688) | Open |
| `cu-human-pose`: the silent CPU fallback, each frame logged twice, the viewer's memory | [#1689](https://github.com/copper-project/copper-rs/issues/1689) | Open |
| The skill-drift entries | [copper-rs-skills #19](https://github.com/copper-project/copper-rs-skills/issues/19) | Open |
| The loop stalls at the log's section seals, and what `async-cl-io` does about it | [#1718](https://github.com/copper-project/copper-rs/issues/1718) | Open |
| Copper's clock drifts about 65 ppm against GStreamer's | not reported | |


### 2026-11-20 · M0 · Dev machine has no build toolchain
- **Symptom:** `cargo`, `rustc`, `rustup`, `just`, `pkg-config`, `gst-launch-1.0` and `rerun` are not
  on PATH. There is no `~/.cargo`, no Visual Studio Build Tools and no GStreamer install, so
  `cargo build -r` in `examples/cu_human_pose` cannot start.
- **Cause:** the laptop (Windows 11) was never provisioned. The build needs Rust 1.98 or newer
  (`rust-version` in the root `Cargo.toml`), GStreamer development files found through pkg-config
  (`cu-gstreamer` with `gst`), and a 0.43.x `rerun` viewer for the viz task's `spawn()`.
- **Fix or workaround:** moving to a new Windows machine set up with `WINDOWS_SETUP.md` (decided
  2026-11-20).
- **Upstream?** *Ours*.
- **Time lost:** M0 blocked.

### 2026-11-20 · M0 · The baseline run assumes Linux; the dev machine runs Windows
- **Symptom:** the M0 run step cannot run as written. The camera pipeline in
  `examples/cu_human_pose/copperconfig.ron` starts with `v4l2src device=/dev/video0`, and the plan
  relies on `apt`, `/dev/video0` and `timeout -s INT`. The laptop has an integrated webcam, but
  Windows has no V4L2 and WSL is not installed. The GPU is an Intel Iris Xe, so `--features cuda`
  cannot be tested here either.
- **Cause:** the example's camera pipeline is Linux-only. On Windows the source element would be
  `mfvideosrc`, a config change that M0 does not allow. CI has never built this example on Windows.
  The Windows job in `.github/workflows/reusable-native-tests.yml` runs with the `core` scope, and
  its `cargo check` list leaves out `cu-gstreamer`. The Windows step in
  `.github/actions/rust-ci-setup/action.yml` installs no GStreamer. `cu-human-pose` sets
  `ci_exclude_workspace = true`, so workspace CI skips it on every OS.
- **Fix or workaround:** native Windows on the new machine. `WINDOWS_SETUP.md` covers the
  Windows tooling; M0's two webcam runs used a temporary, uncommitted `mfvideosrc` edit of
  `examples/cu_human_pose/copperconfig.ron`, reverted afterwards; later webcam runs went
  through a local RTSP stand-in.
- **Upstream?** *Ours*. Copper's Windows breakages have their own entries.
- **Time lost:** M0 blocked.

### 2026-11-20 · M0 · Copper agent skills are not installed
- **Symptom:** The coding agent lists no `copper-*` skills, and there is no `../copper-rs-skills` clone.
- **Cause:** pre-session setup step 2 was not done.
- **Fix or workaround:** read `copper-arch`, `copper-workflow` and `copper-coding-style`
  directly from GitHub (copper-rs-skills at `1a3c6e5`, 2026-10-18), as AGENTS.md allows. The
  plugin was never installed; later sessions read the same skills from a local clone at that
  commit.
- **Upstream?** *Ours*.
- **Time lost:** none.

### 2026-11-20 · M0 · Skill drift: `copper-workflow` gives the minimum Rust as 1.95
- **Symptom:** the skill says builds need Rust 1.95+, but the root `Cargo.toml` sets
  `rust-version = "1.98"`.
- **Cause:** the skill targets Copper 1.1; this checkout is 1.4.0-dev.
- **Fix or workaround:** follow the code: Rust 1.98 or newer.
- **Upstream?** *Upstream*, in copper-rs-skills: `copper-workflow/SKILL.md`.
- **Time lost:** none.

### 2026-11-20 · M0 · Skill drift: the `copper-workflow` apt list points at the wrong file
- **Symptom:** the skill names `support/docker/Dockerfile.ubuntu` as the source of truth for its
  apt list, but that Dockerfile has none of the skill's `libwayland-dev`, `libxkbcommon-dev` or
  `cbor2`. CI installs its Linux packages from `.github/actions/rust-ci-setup/action.yml`, which
  also adds `libasound2-dev`, `python3-cbor2` and `python3-dev`.
- **Cause:** both the snapshot and the pointer have drifted.
- **Fix or workaround:** if a Linux machine joins, start from the CI action's list.
- **Upstream?** *Upstream*, in copper-rs-skills: `copper-workflow/SKILL.md` should point at the CI
  action.
- **Time lost:** none.

### 2026-11-20 · M0 · The new machine has not been set up yet
- **Symptom:** on the new machine, `rustup`, `cargo`, `just`, `nasm`, `pkg-config`, the GStreamer
  tools, `nvcc`, `rerun`, `taplo`, `fmtron`, `prek` and `typos` are not on PATH, so
  `cargo build -r` cannot start. `python3` runs the Microsoft Store stub (exit code 9009), so every
  `just` recipe would fail on load too.
- **Cause:** sections 1 to 7 of `WINDOWS_SETUP.md` have not run on this machine, and some of what
  was already installed differs from the guide: Visual Studio 2026
  instead of Build Tools 2022, python.org Pythons without a `python3.exe`, Developer Mode off, and
  a clone with CRLF line endings, no symlinks and no git identity.
- **Fix or workaround:** done in session 2 from `WINDOWS_SETUP.md`.
- **Upstream?** *Ours*.
- **Time lost:** M0 blocked until then.

### 2026-11-20 · M0 · `timeout -s INT` cannot stop a native Windows process
- **Symptom:** in Git Bash, `timeout -s INT 3` on a Python script that catches
  `KeyboardInterrupt` did not interrupt it: the script slept its full 20 s. Plain `timeout`
  (SIGTERM) and `timeout -s KILL` ended it after 3 s, but as a hard kill that skipped its handler.
  A Copper app stops cleanly only through the `ctrlc` handler that the runtime macro installs (it
  sets `STOP_FLAG`; `core/cu29_derive/src/runtime_macro.rs`). So the plan's `timeout -s INT 60 cargo run -r` would either never stop the app or kill it before it stops its
  tasks and closes its log.
- **Cause:** Git Bash's signal emulation raises no Windows console control event in a native
  (non-Cygwin) process. SIGINT appears to be dropped; SIGTERM and SIGKILL terminate the process.
- **Fix or workaround:** start the process in its own process group and send it
  `CTRL_BREAK_EVENT` after N seconds. Confirmed on the real app: two `cu-human-pose` webcam runs
  (45 s and 25 s) exited 0.6 s and 0.5 s after Ctrl+Break, both with code 0, so Copper's
  `ctrlc` handler takes Ctrl+Break like Ctrl+C. The Rerun viewer that the viz task spawns shares
  the process group, so it closes too.
- **Upstream?** *Ours* (dev environment). Copper's `ctrlc` handler already takes Ctrl+Break, so
  Copper needs no change.
- **Time lost:** about 15 min.

### 2026-11-20 · M0 · Converting the CRLF clone to LF in place takes four git steps
- **Symptom:** after setting `core.autocrlf=input` and `core.symlinks=true`,
  `git checkout-index --force --all` recreated the three symlinks but left 1,681 files in CRLF,
  because their cached stat data still matched. Once the files had been touched and rewritten,
  `git status` listed 1,679 of them as modified, although `git diff` was empty and
  `git hash-object` matched every index blob. Neither `git update-index --really-refresh` nor
  `git reset` cleared it.
- **Cause:** the index still cached each file's CRLF size. Git treats a size mismatch as a
  change without reading the content, and `git reset` keeps the cached stat data of entries
  whose blob is unchanged.
- **Fix or workaround:** `git ls-files -z | xargs -0 touch -c`, then
  `git checkout-index --force --all`, then `git read-tree HEAD` (fresh entries with no cached
  sizes), then `git update-index -q --refresh`. Status is clean, symlinks are real, and only
  `core/cu_bincode/docs/spec.md` and `core/cu_bincode_derive/README.md` stay CRLF, because they
  are committed that way. Auto mode refused the usual `git rm -r --cached . && git reset --hard`
  as destructive.
- **Upstream?** *Upstream*, small, in copper-rs: a `.gitattributes` with `* text=auto eol=lf`
  would make default Windows clones LF from the start. The conversion itself was ours.
- **Time lost:** about 15 min.

### 2026-11-20 · M0 · The GStreamer installer download stalled at 13 to 70 KB/s
- **Symptom:** the 541 MB `gstreamer-1.0-msvc-x86_64-1.28.9.exe` came down at 13 to 70 KB/s
  (about seven hours), while `static.rust-lang.org` gave 1.5 MB/s once the Visual Studio
  download had finished.
- **Cause:** the path to the GStreamer CDN: a fresh IPv6 connection gave 155 KB/s, a single
  IPv4 connection 1.5 MB/s.
- **Fix or workaround:** eight parallel `curl -4 -r` range requests fetched the whole file in
  167 s; it matched the published SHA-256 and replaced the partial download.
- **Upstream?** *Ours*.
- **Time lost:** about 20 min.

### 2026-11-20 · M0 · A silent CUDA toolkit install leaves `bin` off PATH
- **Symptom:** after `cuda_12.9.1_windows_network.exe -s <subpackages> -n`, `CUDA_PATH` and
  `CUDA_PATH_V12_9` were set, but no PATH entry pointed at `CUDA\v12.9\bin`, where nvcc and the
  `cudart`, `cublas` and `nvrtc` DLLs live.
- **Cause:** the selected subpackages don't include the installer's PATH step.
- **Fix or workaround:** `CUDA\v12.9\bin` added to the user Path.
- **Upstream?** *Ours*.
- **Time lost:** 5 min.

### 2026-11-20 · M0 · The CUDA build links a static-CRT `libmoe.a` (LNK4098)
- **Symptom:** `cargo build -r --features cuda` in `examples/cu_human_pose` passes in 3.6 min
  (PTX built for `sm_120a`), but the link warns:
  `LINK : warning LNK4098: defaultlib 'LIBCMT' conflicts with use of other libs`. The CPU build
  has no warnings.
- **Cause:** `candle-kernels` 0.11.1 compiles its mixture-of-experts kernels into `libmoe.a`
  (45 MB) with nvcc, and the archive's host objects ask for the static C runtime (`LIBCMT`,
  `LIBCPMT`), while Rust links the DLL runtime (`MSVCRT`). Found with `dumpbin /directives`;
  aws-lc-sys's archives ask for `MSVCRT` as expected.
- **Fix or workaround:** none yet. It is harmless unless C-runtime objects (heap blocks,
  `FILE*`) cross between the two; watch for crashes in CUDA runs. None was seen through M3.
- **Upstream?** *Upstream*, in huggingface/candle, not Copper: `candle-kernels` should build
  `libmoe.a` with the DLL runtime on MSVC (`-Xcompiler /MD`) or follow `crt-static`.
- **Time lost:** 10 min.

### 2026-11-20 · M0 · Each run takes 1 GB of disk: log slabs are not sparse on NTFS
- **Symptom:** after a webcam run, `logs/human-pose.copper` and `logs/human-pose_0.copper` both
  list as 1,024 MB, but they are one file. On Windows the logger makes the base name a hard link
  to slab 0 (`create()` in `core/cu29_unifiedlog/src/memmap.rs`), so both names have the same
  file ID and a link count of 2, and `du` counts 1.0 GB. `fsutil sparse queryflag` reports the
  file as not sparse.
- **Cause:** the example asks for 1 GB slabs (`SLAB_SIZE` in `src/main.rs`), and the unified log
  sizes each slab up front: `grow()` in `memmap.rs` opens it with `truncate(true)`, which empties
  an old slab of the same name, then calls `set_len(slab_size)`, the crate's only `set_len`.
  Nothing shortens a slab later. `ByteStorage` has no shrink method, and closing
  (`ByteLogger::close()`, which `Drop` also runs) only seals the open sections, marks the header
  clean and flushes. The logger writes only its header at start-up, so Linux file systems leave
  the unwritten rest sparse. NTFS allocates the whole length unless the file is marked sparse:
  here a 1 GB `SetLength` test file took 1,024 MB of free space.
- **Fix or workaround:** none for the baseline. Our app can pick a smaller slab in M1. With the
  default `CapacityPolicy::Grow`, a log that fills its first slab adds full-size `_1.copper`,
  `_2.copper` and so on, and our app's 256 MiB slabs came every 11 to 13 s with frames.
- **Upstream?** *Upstream*, in `cu29_unifiedlog`: on Windows, mark slab files sparse
  (`FSCTL_SET_SPARSE`) before sizing them. Shortening slabs on close would break reading:
  `MmapStorage::open` rejects any slab whose length differs from slab 0's.
- **Time lost:** 5 min, plus 10 min re-checking in session 3.

### 2026-11-20 · M0 · `cu-human-pose` falls back to the CPU without saying so
- **Symptom:** `src/tasks/yolo_pose.rs` picks `Device::new_cuda(0).unwrap_or(Device::Cpu)`, so a
  `--features cuda` build that fails to open the GPU still runs, on the CPU, with no message. Here
  CUDA worked: `nvidia-smi` listed `cu-human-pose.exe` as a compute process in all 12 samples of a
  25 s run, at 28 to 45 % GPU load.
- **Cause:** example design.
- **Fix or workaround:** our detector (M2a) logs the device it got, and fails if `cuda` was
  requested but no GPU opened. Done in M2a: `YoloDetect` logs `yolov8n on the GPU` and a
  `cuda` build stops at start if GPU 0 doesn't open.
- **Upstream?** *Upstream*, small, in `cu_human_pose`: log the device it runs on.
- **Time lost:** none.

### 2026-11-20 · M0 · A run outside a terminal prints nothing
- **Symptom:** stdout and stderr of both bounded webcam runs were empty. The console monitor drew
  nothing into the redirected output, and the text logs go into the `.copper` file. Frame counts
  and errors are invisible without a log reader, and this example has none.
- **Cause:** expected Copper behaviour, but it leaves automated runs unverifiable.
- **Fix or workaround:** confirmed the runs through exit codes, `nvidia-smi` and log files.
  Bring the logreader forward from M5 into M1, so M1's 10-minute check can count frames. Done in
  M1: `cu-skylock-tracker-logreader`.
- **Upstream?** *Ours*.
- **Time lost:** none.

### 2026-11-20 · M0 · The root justfile breaks when the repo path contains a space
- **Symptom:** every recipe that runs (here `just fmt`) failed while loading the justfile, with
  `error: shell command failed` at `justfile:14:23`. `just --list` still works, because it
  doesn't evaluate the variables.
- **Cause:** line 14,
  `WORKSPACE_EXCLUDES := shell('python3 $1/support/ci/workspace_excludes.py ...', ROOT)`, leaves
  `$1` unquoted, so a clone path with a space in it (here, from the Windows user name) splits
  at the space. Line 13 quotes `"$1"` and works. Here Python then ran a stray file named after
  the path's first word (see the next entry) and failed with a `SyntaxError`; without that file
  it would fail with "can't open file".
- **Fix or workaround:** override the root with the clone's space-free 8.3 path:
  `just ROOT=<short path> <recipe>`, where `cmd /c for %I in (.) do @echo %~sI` in the clone
  prints the short path. Running from the short path alone doesn't help, because
  `source_directory()` resolves to the long name.
- **Upstream?** *Upstream*, in copper-rs: quote it, `python3
  "$1/support/ci/workspace_excludes.py"`.
- **Time lost:** 10 min.

### 2026-11-20 · M0 · `just fmt` overflows the Windows command line in `cargo fmt --all`
- **Symptom:** with the root overridden, `just fmt` stops at its first step:
  `cargo +stable fmt --all` fails with `The filename or extension is too long. (os error 206)`.
  prek's `check-rust` hook runs the same command.
- **Cause:** cargo-fmt hands the whole workspace's target files to one rustfmt process, which
  is longer than Windows' 32,767-character command-line limit.
- **Fix or workaround:** a local helper script, `fmt.sh [package...]`: `cargo fmt -p` for the named packages, then the recipe's taplo, fmtron and prek steps
  without `check-rust`. Session 2's Markdown-only pass ran those steps; all passed once typos
  stopped reading two letters inside the 11-character commit hash as a misspelling (the notes
  now use the 7-character `cd321a8`).
- **Upstream?** *Upstream*: cargo-fmt (rust-lang/rustfmt) could batch rustfmt calls, and Copper's
  `fmt` recipe could format per package on Windows.
- **Time lost:** 10 min.

### 2026-11-20 · M0 · The GStreamer install log landed in a stray file in `C:\Users`
- **Symptom:** a 1.6 MB file in `C:\Users`, named after the first word of the user-profile
  path and owned by Administrators, holds the Inno Setup log of the GStreamer install.
- **Cause:** The admin setup script passed `/LOG=<path with a space>` through PowerShell 5.1's
  `Start-Process -ArgumentList`, which doesn't quote arguments that contain spaces, so the
  installer saw the path up to the space. The install itself succeeded.
- **Fix or workaround:** delete the file from an admin PowerShell with
  `Remove-Item -LiteralPath` (path in the local notes). Quote such arguments by hand next time.
- **Upstream?** *Ours*.
- **Time lost:** 5 min.

### 2026-11-20 · M1 · The camera source writes its pipeline, URL included, to the text log
- **Symptom:** `CuGStreamer::new()` logs `debug!("Creating with pipeline: {}", ...)`
  (`components/sources/cu_gstreamer/src/cu_gstreamer_impl.rs`), so once `main.rs` has put the
  URL into the pipeline, every `.copper` file holds it twice: in `effective_config_ron` and in
  the text log. Seen on the first run, with a dummy URL: `extract-text-log` prints
  `Creating with pipeline: rtspsrc location="..."` with the URL in full.
- **Cause:** the source takes the whole pipeline as one string and logs it as is, at debug
  level, which this app compiles in (`log-level-debug`).
- **Fix or workaround:** treat logs as secret, and read them only through
  a local helper script, `run_bounded.py`, which redacts the URL, its credentials and its host from everything it
  captures. The app's own error messages go through `rtsp::redact`. Not fixed: M5's local copy
  of the source, which was to read the URL inside `new()` and log the pipeline without it, is
  parked.
- **Upstream?** *Upstream*, in `cu_gstreamer`: take the URL apart from the pipeline, or mask URIs
  before logging the pipeline.
- **Time lost:** none.

### 2026-11-20 · M1 · Skill drift: `logging.slab_size_mib` doesn't size this app's slabs
- **Symptom:** `copper-ron-config` lists `logging: (slab_size_mib: ...)` as the slab size, but
  for an app that calls `builder.with_log_path(path, slab_size)`, as `cu_human_pose` does,
  only that argument counts: `with_log_path` in `core/cu29_derive/src/runtime_macro.rs` uses
  it, or 10 MiB when it is `None`. Outside config validation, only
  `core/cu29_runtime/src/distributed_replay.rs` reads `slab_size_mib`.
- **Cause:** the skill describes the config schema; the builder doesn't consult it.
- **Fix or workaround:** our `main.rs` passes 256 MiB (`SLAB_SIZE`), down from the pose
  example's 1 GiB, because NTFS allocates each slab file in full.
- **Upstream?** *Upstream*, in copper-rs-skills or copper-rs: the skill should say the builder
  argument wins, or `with_log_path` could fall back to `slab_size_mib`.
- **Time lost:** 5 min.

### 2026-11-20 · M1 · `gen_cumsgs!` can't resolve the pose example's message path
- **Symptom:** the new logreader failed to build: `error[E0433]: cannot find module or crate
  payloads in this scope` at `gen_cumsgs!("copperconfig.ron")`, although the file imports
  `payloads` at its root (rustc called that import unused).
- **Cause:** `cu_human_pose` names its message `payloads::CuPoses`, a path relative to the
  crate root. `#[copper_runtime]` resolves it there, but `gen_cumsgs!`
  (`core/cu29_derive/src/lib.rs`) emits its types inside `mod cumsgs { ... }` with no
  `use super::*`, where only extern crates and `crate::` paths resolve.
- **Fix or workaround:** the RON uses `crate::`-rooted message paths (`crate::payloads::CuPoses` then;
  `crate::payloads::Detections` and `crate::payloads::TrackState` now), which both macros
  accept, as `examples/cu_min_baremetal` does with `crate::tasks::...`.
- **Upstream?** *Upstream*, small, in `cu29_derive`: `gen_cumsgs!` could add `use super::*;`
  inside `mod cumsgs`, or `copper-ron-config` could say that message paths must be
  `crate::`-rooted or external.
- **Time lost:** 10 min.

### 2026-11-20 · M1 · An unreachable camera fails silently
- **Symptom:** with `SKYLOCK_RTSP_URL` pointing at a port where nothing listens
  (`rtsp://127.0.0.1:8554/none`), the app ran its full 20 s and exited 0 on Ctrl+Break. The
  console showed only a GLib warning (next entry). The text log has no GStreamer error, only
  `gst_to_image: no frames received for N ms` every 2 s and `0 frames in 10000 ms`.
- **Cause:** `cu_gstreamer` never reads the pipeline's bus, so `rtspsrc`'s connection error
  goes nowhere. The pose example's camera config passes `log_bus_messages`, `log_pad_events`,
  `log_src_pad_events` and `stop_timeout_ms`, but no code reads them; our RON drops them.
- **Fix or workaround:** none in M1, and still open at hand-off: reading the bus was M5's,
  which is parked. The 10 s drop test shows what a live outage does.
- **Upstream?** *Upstream*, in `cu_gstreamer`: watch the bus, log warnings, and turn errors and
  EOS into a task error or a status the monitor shows.
- **Time lost:** none.

### 2026-11-20 · M1 · GStreamer prints a `giolibproxy.dll` load failure at start
- **Symptom:** every run starts with
  `Failed to load module: C:\gstreamer\1.0\msvc_x86_64\lib\gio\modules\giolibproxy.dll` on
  stderr.
- **Cause:** GLib's GIO scans its modules folder at start; the proxy-resolver module in the
  1.28.9 MSVC install doesn't load. `rtspsrc` doesn't use GIO's proxy resolver.
- **Fix or workaround:** none needed; it's noise.
- **Upstream?** *Ours*: harmless noise. It may come from GStreamer's Windows packaging, but a clean
  install wasn't tried, so this machine may be the cause.
- **Time lost:** none.

### 2026-11-20 · M1 · With no rate target, the loop spins at about 100 kHz between frames
- **Symptom:** in the 20 s run with no camera, `fsck` counted 1,519,298 CopperLists in
  15.2 s (100 kHz) and as many text-log lines, almost all
  `Gstreamer: Empty circular buffer, sending no payload.` That came to 5.4 MiB/s of log with no
  frames at all, and one CPU core stayed busy. `cu_logmon` reported 56 to 131 kHz. With the
  camera, the spin's empty CopperLists and debug lines made 1.6 GB of the 10-minute run's
  14.0 GB; the frames made the rest (see "Each frame is logged twice").
- **Cause:** without `runtime: (rate_target_hz: N)`, Copper starts the next CopperList as
  soon as the last one ends. The source's `process()` returns at once when its ring is empty
  and logs a debug line each time; the pose example compiles debug logs in
  (`log-level-debug`).
- **Fix or workaround:** none in M1, which measures the example as it is. Decided on 2026-11-20: after M1, set `runtime: (rate_target_hz: N)`
  (`examples/cu_rate_target`) to about four times the camera's fps, and re-measure the lag.
  `LoopRateLimiter` (`core/cu29_runtime/src/curuntime.rs`) sleeps out the rest of each period,
  so the core goes idle, and a frame waits at most a quarter period longer. Done on 2026-11-22
  with `rate_target_hz: 120` (see "With the rate target, the stalls stay"); the per-pass debug
  line stays, since M5's copy of the source is parked.
- **Upstream?** *Upstream*, in `cu_gstreamer`: don't log on every empty pass; and the pose example
  could set a rate target.
- **Time lost:** none.

### 2026-11-20 · M1 · Under Cargo 1.100 the log string index moves to `target/<profile>/build/`
- **Symptom:** `extract-text-log` needs the string index, and the documented path
  `target/debug/cu29_log_index` doesn't exist here. That path appears in
  `core/cu29_export/README.md`, in the `log` recipe of both `cargo cunew` templates
  (`support/cargo_cunew/templates/*/justfile`), and as `../../target/debug/cu29_log_index` in
  `examples/cu_caterpillar/justfile`. The index is in `target/<profile>/build/cu29_log_index`.
- **Cause:** `default_log_index_dir()` in `core/cu29_intern_strs/src/lib.rs` goes three levels
  up from `LOG_INDEX_DIR` (the crate's `OUT_DIR`). That reaches `target/<profile>` with
  Cargo's old `build/<crate>-<hash>/out` layout, but Cargo 1.100 lays build scripts out as
  `build/<crate>/<hash>/out`, one level deeper. Neither the repo nor the user config sets
  `build-dir`, so this is Cargo's default.
- **Fix or workaround:** pass `../../target/release/build/cu29_log_index` from the app's
  directory.
- **Upstream?** *Upstream*, in `cu29_intern_strs`: find the profile directory without assuming the
  depth (for example, the parent of the `build` directory); the README and templates should
  follow.
- **Time lost:** 5 min.

### 2026-11-20 · M1 · Clippy 1.100 fails `cu_consolemon`, so the milestone clippy check fails
- **Symptom:** the plan's `cargo clippy -p cu-skylock-tracker --all-targets -- -D warnings`
  stops in `cu-consolemon`: `clippy::needless_bool` at
  `components/monitors/cu_consolemon/src/lib.rs:392`. The same command with `--no-deps`,
  which lints only our crate, exits 0.
- **Cause:** clippy also lints workspace path dependencies, and `-D warnings` reaches them.
  The flagged code exists only on Windows. `should_start_ui()` returns `false` from an `if`,
  then has a `#[cfg(unix)]` block (the foreground process-group check) before its final
  `true`. Off Unix that block compiles away, which leaves `if c { return false; } true`, the
  shape `needless_bool` flags. On Linux and macOS the block sits in between, so the lint has
  nothing to fire on. Here, the same clippy 1.100 passes for the Linux target
  (`cargo clippy -p cu-consolemon --target x86_64-unknown-linux-gnu -- -D warnings`).
  Copper's CI won't meet it: every clippy job runs on Ubuntu, and the
  Windows job only runs `cargo check` on `cu-consolemon`
  (`.github/workflows/reusable-native-tests.yml`). (Corrected on 2026-11-22: this entry first
  said CI would meet the lint once its stable reached 1.100.)
- **Fix or workaround:** decided on 2026-11-20: the milestone
  check runs with `--no-deps`, which lints exactly our crate, and the plan says so. Shared code
  stays untouched.
- **Upstream?** *Upstream*, small, in `cu_consolemon`: write `should_start_ui()` so neither cfg
  leaves that shape, for example by moving the TTY test into the final expression. Running
  clippy on Windows in CI would catch this kind of cfg-only lint.
- **Time lost:** 5 min.

### 2026-11-20 · M1 · Pasting the start routine at once stored a command as the URL
- **Symptom:** after the restart for M1, `SKYLOCK_RTSP_URL` held the routine's second line,
  its `cd /d ...` command, so the app would have refused it. It was found without printing the value: its length, no `://`, then an exact comparison with that line.
- **Cause:** WINDOWS_SETUP.md section 7 gave three lines. Pasted together, `set /p` takes the
  next pasted line as its answer, and the rest run as commands.
- **Fix or workaround:** section 7 now gives one line to paste
  (`cd ... && set /p SKYLOCK_RTSP_URL=RTSP URL: && code .`), with the URL pasted on its own at
  the prompt. A local start script asks for the URL on its own, strips pasted
  quotes, refuses anything that doesn't start with `rtsp`, and never prints it.
  The run helper also refuses a URL that isn't `rtsp://`.
- **Upstream?** *Ours*.
- **Time lost:** one restart.

### 2026-11-20 · M1 · VS Code started from VS 2026's x64_x86 Cross Tools prompt
- **Symptom:** in the same restart, `cl.exe` was VS 2026's
  (`18\Community\...\14.51.36231\bin\HostX64\x86`), with `VSCMD_VER=18.6.2`,
  `VSCMD_ARG_TGT_ARCH=x86` and `CommandPromptType=Cross`, so CUDA builds would have failed:
  CUDA 12.9's nvcc rejects VS 2026. Worse, inside that environment even VS 2022's
  `vcvars64.bat` hands back VS 2026's compiler, because the inherited `VSINSTALLDIR` and
  related variables win.
- **Cause:** Start lists "x64_x86 Cross Tools Command Prompt for VS" (2026) next to the
  "x64 Native Tools Command Prompt for VS 2022" that section 7 asks for.
- **Fix or workaround:** section 7 now warns about the look-alike. The local start script loads
  VS 2022's `vcvars64.bat` itself and refuses to run in a window that already has a Visual
  Studio environment. From a clean environment it gave the Build Tools 2022 compiler
  (14.44.35207, x64). It also puts `vswhere.exe`'s folder on PATH, which this machine's Path
  lacks.
- **Upstream?** *Ours*.
- **Time lost:** the same restart.

### 2026-11-20 · M1 · PowerShell 5.1 mangled the quoted `location` of the first RTSP probe
- **Symptom:** the first `gst-launch-1.0` probe stopped at
  `Progress: (connect) Connecting to <URL> protocols=tcp`, with ` protocols=tcp` glued to the
  address.
- **Cause:** in PowerShell 5.1, `location=\"...\"` reaches the program as `location=\...\`:
  backslash isn't PowerShell's escape character, so `"..."` is a string joined to the bare
  word. gst-launch then read the trailing `\ ` as an escaped space and kept
  ` protocols=tcp` in the value. Called from Python with the same quoted argument, gst-launch
  read the address correctly.
- **Fix or workaround:** pass the location unquoted, or build the argument list in Python. The app is unaffected,
  because `main.rs` quotes the URL itself.
- **Upstream?** *Ours*.
- **Time lost:** 10 min.

### 2026-11-20 · M1 · Python can't start `gst-launch-1.0` without `.exe`
- **Symptom:** `run_bounded.py ... -- gst-launch-1.0 ...` failed with
  `[WinError 2] The system cannot find the file specified`, although `gst-launch-1.0` runs
  from a shell.
- **Cause:** `CreateProcess` appends `.exe` only to names without an extension, and it takes
  the `.0` of `gst-launch-1.0` for one.
- **Fix or workaround:** name it `gst-launch-1.0.exe` in scripts.
- **Upstream?** *Ours*.
- **Time lost:** 2 min.

### 2026-11-20 · M1 · The source's ring of 8 hands out stale frames when the loop is slow
- **Symptom:** measured with `gst_to_image`'s frame statistics, 60 s per run. Lag is `tov`
  minus the buffer PTS, above the run's minimum; drops come from PTS steps against the
  camera's 30 fps.
  - Webcam stand-in (H.264 1280x720 at 30 fps through a local MediaMTX), CPU inference
    (132 to 169 ms a frame): ring 8 got 438 frames (7.4 fps), and once the ring filled every
    frame was about 214 ms older (lag p50 240 ms, max 263 ms). Ring 1 got 346 frames, about
    9 ms above the steady minimum; its lower rate came from slower inference in that run.
  - Stand-in, CUDA: ring 8 got 1,743 of about 1,747 frames (0.2 % dropped), lag p50 9 ms,
    p95 85 ms, p99 223 ms (7 frame periods, after stalls). Ring 1 got 1,422 (19 % dropped),
    about 4 ms typical.
  - SKYLOCK camera (H.265 1920x1080 at 30 fps), CUDA, start-up burst left out: ring 8 got
    1,464 of 1,464 frames, lag p50 14 ms, p95 32 ms, p99 47 ms, max 100 ms. Ring 1 got 1,309
    of about 1,461 (10 % dropped), lag p95 55 ms, max 409 ms.
  - M1's 10-minute check on the SKYLOCK camera (CUDA, ring 8, Rerun on, watched by Ahyan,
    who saw no freeze): 17,546 of about 17,588 frames (0.24 % dropped), 283 to 318 frames in
    each of the 59 periods. Its lag percentiles (p50 151 ms, max 554 ms) are mostly clock
    drift; see the clock entry.
- **Cause:** `circular-buffer`'s `push_back` overwrites the oldest frame when the ring is full,
  and `CuGStreamer::process()` pops the oldest, so a loop slower than the camera always gets
  the frame N - 1 periods old. Drops are silent: nothing counts the overwritten frames.
- **Fix or workaround:** none in M1, and still open at hand-off: counting overwrites was
  M5's, which is parked. Ring 8 is the better default for SKYLOCK's CUDA path;
  a slow path wants the newest frame instead.
- **Upstream?** *Upstream*, in `cu_gstreamer`: count overwrites, and offer a newest-frame mode
  (pop the back, drop the rest) next to the FIFO.
- **Time lost:** none.

### 2026-11-20 · M1 · CUDA runs stall the loop for 100 to 500 ms several times a minute
- **Symptom:** every 10 s period of the CUDA runs has a longest gap between frames of 60 to
  510 ms (10-minute stand-in run: longest 507 ms, six over 300 ms; CopperList end to end max
  483 ms; 10-minute SKYLOCK camera run: longest 371 ms, 10 of 59 periods over 200 ms, none
  visible to Ahyan in Rerun). In `log-stats`, the camera task's `process()`, which only pops the ring under its
  mutex, peaked at 119 to 244 ms, and the time before a CopperList starts peaked at 147 to
  227 ms. CPU runs, where the loop spins at about 200 Hz instead of 47 to 185 kHz, peaked at
  3 ms and 68 ms. The 60 s runs with `RERUN=off` (ring 8) had stalls too, but smaller: after
  the start-up period, longest gaps up to 165 ms on the stand-in and 128 ms on the camera.
  Gaps over 300 ms came only in the 10-minute runs with the viewer on.
- **Cause:** found on 2026-11-22: the log's section seals (see "With the rate target, the
  stalls stay, at the log's section seals"). The suspects as first listed, checked against
  the code on 2026-11-22:
  - The spin: the ring mutex, taken 50,000 to 180,000 times a second while GStreamer's
    thread pushes under it, and the extra log the spin writes (1.6 GB per 10 minutes).
  - Log writes on the loop thread. This build has no `cu-output` worker (`async-cl-io` is
    off, so the binary lacks the thread; this entry first named that worker as a suspect),
    so the loop thread serializes each CopperList into the mapped log itself. When a 32 MiB
    section fills, which with frames happens about every 1.6 s, it seals it and flushes the
    whole slab mapping. memmap2 0.9.12 does that on Windows with `FlushViewOfFile` and then
    `FlushFileBuffers`, which waits for the disk.
  - The text log does the same from whichever thread logs. It is a `LogStream` with 40 KiB
    sections (`4096 * 10` in `core/cu29_derive/src/runtime_macro.rs`), so the spin's
    0.9 MB/s of `debug!` lines seal a section, with that whole-slab flush, about 22 times a
    second, inside the camera's `process()`. That could explain the 119 to 244 ms peaks of a
    task that only pops a ring.
  - New 256 MiB slabs, created on the same thread: one about every 11 s in these runs
    (53 in the 10-minute camera run), because each frame is logged twice (see "Each frame is
    logged twice").
  - Memory pressure: in the 10-minute runs the Rerun viewer held up to 11.8 GiB of this
    laptop's 16 GB.
- **Fix or workaround:** re-measure with the rate target. It removes the spin and nearly all
  of the text log's flushes, but not the frames' log writes: each frame is still logged
  twice, so a section seal still comes about every 1.6 s and a new slab about every 13 s. So
  the re-measure tells the spin apart from slab creation: if the stalls go, the spin caused
  them; if they stay, `gst_to_image` logs the time of each long gap, to compare with the slab
  files' creation times. One of the runs has `RERUN=off`, to tell the viewer's memory
  pressure apart. Done on 2026-11-22: see "With the rate target, the stalls stay".
- **Upstream?** *Upstream*, in `cu29_unifiedlog`: the stalls are the log's section seals; see
  "With the rate target, the stalls stay, at the log's section seals".
- **Time lost:** 20 min, plus 20 min re-checking on 2026-11-22.

### 2026-11-20 · M1 · The Rerun viewer filled 11.8 GiB of RAM in a 10-minute run
- **Symptom:** in the 10-minute stand-in run, the viewer the viz task spawns logged
  `Exceeded gRPC proxy server memory limit (1.0 GiB)` after 2 minutes and
  `Reached memory limit of 11.8 GiB` after 9, on this 16 GB laptop. The 10-minute run on the
  SKYLOCK camera did the same, at 1 and 9 minutes. It also printed
  `Listening for gRPC connections on 0.0.0.0:9876`, which is every network interface.
- **Cause:** `RerunPoseViz` logs every frame as a raw RGB image (640x360x3 bytes at 30 fps,
  about 20 MB/s), and the viewer keeps history until it hits its default memory limit.
- **Fix or workaround:** none in M1. M2a's viz rework should log fewer or compressed images
  and spawn the viewer with a memory limit. Done in M2a: `RerunDetectViz` sends NV12 frames,
  half the bytes of RGB, to a viewer spawned with a 2 GB memory limit, a 64 MB gRPC buffer
  and `--bind 127.0.0.1`. Over 5 minutes at 30 fps its working set grew about 12 MB/s to
  2 GB in 2.5 minutes, then stayed between 1.8 and 2.2 GB, and it listened on
  127.0.0.1:9876 only.
- **Upstream?** *Upstream*, in `cu_human_pose`'s viz.
- **Time lost:** none.

### 2026-11-20 · M1 · The SKYLOCK camera usually opens with several seconds of old frames
- **Symptom:** `fpsdisplaysink` counted 256 frames in the first 1.5 s (about 190 fps), then
  a steady 30 fps. In the app, the first frames were about 5.8 s staler than the steady ones,
  and their PTS went backwards 177 times in that burst, which broke the lag statistics. Five
  of the six connections today had the burst; the drop-test run, which connected a minute
  after the previous run ended, didn't.
- **Cause:** the camera's RTSP server or its encoder queue flushes buffered frames to a new
  client.
- **Fix or workaround:** `gst_to_image` takes `stats_skip_s` (10 in the RON) and restarts its
  run statistics after it. M3's tracker does trust them, since a frame's `tov` is when the
  source pops it; M5's PTS-based `tov`, which would tell those frames from live ones, is parked.
- **Upstream?** *Upstream*, in `cu_gstreamer`, for the timing: it stamps `tov` when
  `process()` pops a frame (its TODO says so), so the burst passes as live; stamping from the
  buffer PTS would tell it apart. The burst itself is the camera's.
- **Time lost:** 15 min.

### 2026-11-20 · M1 · Clocks drift: Copper about 65 ppm against GStreamer, the camera about 470 ppm
- **Symptom:** the lowest lag of each 10 s period, which strips out ring waits, moves steadily.
  With the stand-in, whose publisher runs on this laptop, it rose about 19 ms in 5.5 minutes,
  so Copper's clock ran about 65 ppm fast against GStreamer's. With the SKYLOCK camera it fell
  227 ms in 8 minutes of the 10-minute run (11 ms in 30 s in a short one), so the camera's RTP
  clock ran about 470 ppm fast against Copper's. Over 10 minutes that drift is most of the
  lag spread `gst_to_image` reports.
- **Cause:** Copper's x86-64 clock calibrates the TSC against the OS clock over one 10 ms sleep
  (`core/cu29_clock/src/calibration.rs`), and a few µs of jitter in that window is 1e-4. The
  camera's oscillator is its own.
- **Fix or workaround:** none needed for M3, whose `dt` comes from `tov` alone. A mapping from
  PTS to Copper time would have to track the drift instead of fixing an offset once; it was
  M5's, which is parked.
- **Upstream?** *Upstream*, in `cu29_clock`: calibrate over a longer window, or refine the
  frequency while running.
- **Time lost:** none.

### 2026-11-20 · M1 · Drop test: a lost connection never comes back, and nothing says so
- **Symptom:** on the SKYLOCK camera (CUDA, ring 8, 120 s run), Ahyan pulled the camera's
  network cable for about 10 s at about 34 s. Before it, frames came at an exact 30 fps (PTS
  step 33 ms, none dropped). After it, the app got 0 frames for the remaining 85 s, although
  the cable was back and the Ethernet link came up again within seconds. No error anywhere,
  only `no frames received` warnings.
- **Symptom, stand-in:** with the webcam stand-in, during 60 s runs (CUDA, ring 8):
  - The publisher frozen for 10 s (a network stall; the TCP connection stays open): frames
    stopped for 10.04 s and came back at 30 fps on the same connection. The PTS ran 10 s
    ahead of `tov` for the rest of the run (span 68.5 s for a 58.5 s run).
  - The publisher killed and restarted 10 s later (a camera reboot): the server closed our
    session, and the app got 0 frames for the remaining 30 s although the stream was back.
    No error anywhere, only `no frames received` warnings.
- **Cause:** `rtspsrc`'s TCP timeout (20 s) outlasts a 10 s stall on an open connection, but
  a closed session or a dropped link ends the stream: Windows takes the interface down when
  the cable is pulled, which most likely breaks the TCP connection. `cu_gstreamer` neither
  watches the bus nor reconnects, so the app runs on blind.
- **Fix or workaround:** none in M1. For SKYLOCK this is the most serious M1 finding: a cable
  glitch blinds the tracker until someone restarts it. Still open at hand-off: reconnecting was
  M5's, which is parked, and M3's live check saw the stream stop twice (see "The camera stream
  stopped twice mid-run").
- **Upstream?** *Upstream*, in `cu_gstreamer`: surface bus errors and EOS, and reconnect.
- **Time lost:** none.

### 2026-11-20 · M1 · Perf debt carried over from the pose example
- **Symptom:** allocations, copies and locks on the hot path that came with `cu_human_pose`,
  listed as the plan asks:
  - `CuGStreamer::process()` locks the ring's `Mutex` on every pass (upstream component).
  - Its `process()` logs `debug!` on every empty pass, which takes the log sink's `Mutex`.
  - `GstToCuImage` copies each frame into a pooled buffer, and `info!`s its first 5 frames.
  - `YoloPose` allocates tensors in preprocessing and inference (Candle).
  - `RerunPoseViz` allocates an RGB `Vec` per frame and `Vec`s for keypoints, lines and boxes.
- **Cause:** example design.
- **Fix or workaround:** accepted until the tasks are rewritten (M2a for the detector and viz).
  Our own `frame_stats` code records without allocating or locking. M2a replaced `YoloPose`
  and `RerunPoseViz`; their successors' debt is in "Perf debt in the detector and its view".
- **Upstream?** *Upstream* for the first two, which are `cu_gstreamer`'s; the rest is ours.
- **Time lost:** none.

### 2026-11-22 · M1 · Each frame is logged twice: 12.4 GB per 10 minutes
- **Symptom:** the 10-minute camera run wrote 14.0 GB in 53 slabs (`fsck`: 13.47 GB of
  CopperLists, 0.54 GB of text log), which the port's notes had put down to the loop's spin.
  Fitting `fsck`'s CopperList bytes against the lists with and without a frame, over five M1
  runs (12,723 to 23.7 million lists, CPU and CUDA), gives about 693 KB per list that carries
  a frame and 46 bytes per empty one. For the 10-minute run that is 12.4 GB of frames
  (17,864 lists), 1.09 GB of empty lists and 0.54 GB of the spin's `debug!` lines.
- **Cause:** Copper logs every task's output unless the task has `logging: (enabled: false)`,
  and our RON keeps the pose example's default. So each list that carries a frame holds it
  twice: the camera's `CuGstBuffer`, whose `Encode` writes the whole mapped buffer
  (`components/sources/cu_gstreamer/src/cu_gstreamer_impl.rs`), and `gst_to_image`'s
  `CuImage<Vec<u8>>`, the same NV12 640x360 frame (345,600 bytes) copied into a pool buffer.
  At 30 fps that is 20.7 MB/s, 12.4 GB per 10 minutes, and with 256 MiB slabs a new slab
  about every 13 s, rate target or not.
- **Fix or workaround:** not fixed: M5, which was to cut it and measure both ways, is parked.
  The two ways: logging off for `gst_to_image` (resim recomputes its output from the camera's),
  or a log codec on the image as in `examples/cu_image_codec_demo`.
- **Upstream?** *Upstream*, in `cu_human_pose`, whose RON logs each frame twice the same way, into
  1 GiB slabs.
- **Time lost:** none.

### 2026-11-22 · M1 · `log-stats` per-edge bytes count memory, not log bytes
- **Symptom:** for the 10-minute run, `log-stats` gives 16 bytes per `camera -> gst_to_image`
  message and 345,632 bytes per message on each of the two `gst_to_image` edges. Read as log
  volume, the camera's frames cost nothing and `gst_to_image`'s count once per reader. The
  edge totals add up to 12.35 GB, close to the real 12.4 GB of frames only because the two
  errors cancel.
- **Cause:** the edge statistics (`core/cu29_export/src/logstats.rs`) take
  `payload_raw_bytes()`, which reports `payload_io_stats(...).resident_bytes`: `size_of::<T>()`
  plus the bytes held in Copper pool handles (`core/cu29_runtime/src/monitoring.rs`). A
  `CuGstBuffer` is a 16-byte value whose frame lives in GStreamer's memory, so it counts as
  16 bytes. An output that feeds two tasks is one slot in the CopperList but two edges.
  `payload_io_stats` also computes `encoded_bytes`, what reaches the log, but `log-stats`
  doesn't report it.
- **Fix or workaround:** count log bytes from `fsck`'s totals, as in the previous entry.
- **Upstream?** *Upstream*, in `cu29_export`: report `encoded_bytes` per output slot, and say that
  the per-edge figure is memory.
- **Time lost:** 10 min.

### 2026-11-22 · M1 · With the rate target, the stalls stay, at the log's section seals
- **Symptom:** a 10-minute run with `runtime: (rate_target_hz: 120)` and `RERUN=off`, CUDA,
  ring 8, on a synthetic stand-in (H.265 1920x1080 at 30.1 fps, `videotestsrc` through
  `qsvh265enc` and MediaMTX, because the SKYLOCK camera was unplugged and the webcam gave
  only 10 fps in the evening light). The spin is gone: 76 CopperLists a second
  (45,296 in all; a pass with a frame takes longer than the 8.3 ms period and skips one),
  63 % of one core, and the log is 12.4 GB in 47 slabs, almost all frames. The stalls are not: 37 gaps of 100 to 578 ms (median
  292 ms), 17,584 of about 17,676 frames (0.5 % dropped, in the periods with the longest
  gaps), lag p50 13 ms, p95 44 ms, p99 196 ms. The longest CopperList took 90 ms from its first
  task to its last, so the time goes outside the tasks.
- **Cause:** the frames' log writes, at the seal of each 32 MiB section, with its
  whole-slab flush on the loop thread (see "CUDA runs stall the loop").
  The gaps come in clusters, and within a cluster they are 1.44 to 2.03 s apart (median
  1.68 s); a section fills every 1.62 s. New slabs came every 12.8 s, eight sections each,
  and the gaps start on average 199 ms from an eighth of a slab's life, against 400 ms for
  random times (none of 2,000 random placements came closer than 263 ms). Only 5 of the 37
  gaps hold a new slab, so slab creation adds little on top. The viewer isn't needed:
  this run had no viewer and longer gaps than M1's runs with one.
  Confirmed with the same binary and 128 MiB sections (`section_size_mib` is read at run
  time; 5 minutes): the gaps then came 6.26 to 6.50 s apart, or a multiple of that, against
  6.49 s for a section to fill, and every second one held a new slab, now that two sections
  fill one. Most were 100 to 115 ms, as each seal now flushes four times as much, but six
  reached 0.5 to 1.5 s (longest 1,468 ms), and 1.2 % of frames were dropped. So the seal's
  synchronous flush is the cause, and the section size only trades how often for how long.
- **Fix or workaround:** not fixed; the RON stays at 32 MiB. Fewer bytes per frame would mean
  fewer seals, and `async-cl-io`, with enough CopperLists and image-pool buffers to cover a slow
  flush, could take the flush off the loop; both were M5's, which is parked.
- **Upstream?** *Upstream*, in `cu29_unifiedlog`: sealing a section flushes the whole slab
  mapping, and on Windows memmap2's `flush()` adds `FlushFileBuffers`, which waits for the disk,
  on the thread that writes the log (here the loop). Flushing only the section's range, without
  waiting, or off the real-time thread, would avoid it. Copper's
  `doc/design/concurrent-logging.md` lists a narrower flush policy as remaining work.
- **Time lost:** 1 hour.

### 2026-11-22 · M2a · Perf debt in the detector and its view
- **Symptom:** what is left on the hot path of the M2a tasks, as the plan asks:
  - `YoloDetect`: our code doesn't allocate per frame. The frame is resized into a reused
    planar RGB buffer, decoding uses buffers sized at start, NMS sorts in place, and the
    output is a fixed-size `Detections`. Candle allocates: `Tensor::from_slice` copies the
    input (to the GPU in a `cuda` build), every layer makes new tensors, the head rebuilds
    its anchor grids each frame, and a `cuda` build copies the output back to the CPU.
    Reading the output takes the tensor's storage `RwLock`, which nothing else holds.
  - `RerunDetectViz`: the labels are rewritten in place in strings allocated at start.
    Rerun copies the NV12 frame (345,600 bytes) and the boxes into Arrow buffers and queues
    them for its own sender thread.
  - Both format an error message only when a frame fails.
- **Cause:** Candle's and Rerun's APIs take owned data.
- **Fix or workaround:** accepted: the plan allows allocation inside inference. The anchor grids could be built once per input
  size by changing the head.
- **Upstream?** *Ours*. Candle's yolo-v8 example also rebuilds the anchors each frame.
- **Time lost:** none.

### 2026-11-22 · M2a · On the CPU, the detector starves the stream
- **Symptom:** the CPU build's 60 s run on the synthetic 30 fps stand-in: `YoloDetect` took
  p50 200 ms and p95 211 ms a frame, the app used 627 % of one core, and frames reached
  `gst_to_image` at about 5 fps (PTS steps 166 to 266 ms) with a low lag (p50 24 ms). So
  frames were lost before the ring, not left waiting in it as in M1's CPU run (lag p50 240 ms
  on the webcam stand-in).
- **Cause:** not checked. Candle's CPU backend uses every core, and the 1080p H.265 decode
  (16 % of one core on its own) falls behind; where the frames go (decoder, `rtspsrc` or the
  appsink) isn't known, and nothing counts them.
- **Fix or workaround:** none: SKYLOCK runs the `cuda` build (p50 14.7 ms, 30 fps). If a CPU
  target ever matters, cap Candle's threads or decode on the GPU (`nvh265dec`).
- **Upstream?** *Ours*.
- **Time lost:** none.

### 2026-11-22 · M2a · A `CuArrayVec` payload needs a hint to derive `Decode`, and has no `len`
- **Symptom:** `Detections` holds a `CuArrayVec<Detection, 32>`, as the plan asks for new
  payloads. The first build failed on `items.len()`: `CuArrayVec` has no `len`, `is_empty` or
  `iter` of its own, so code reaches through `.0` to the `ArrayVec`. A plain
  `#[derive(Decode)]` would not fit either: the derive implements `Decode<C>` for any
  context `C`, while `CuArrayVec` implements only `Decode<()>`
  (`core/cu29_runtime/src/payload.rs`). `components/tasks/cu_apriltag` writes its
  `Decode<()>` by hand.
- **Cause:** gaps in `CuArrayVec`'s API.
- **Fix or workaround:** `#[bincode(decode_context = "()")]` on the payload, which Copper's
  bincode derive accepts (`core/cu_bincode_derive/src/attribute.rs`); a round-trip test covers
  it. `.0` for the length and the items.
- **Upstream?** *Upstream*, in `cu29_runtime`: `CuArrayVec` could implement `Decode<C>` for any
  context, and `Deref` to its `ArrayVec`; the payload docs or `copper-coding-style` could show
  the attribute.
- **Time lost:** 10 min.

### 2026-11-23 · M3 · Skill gap: pixel quantities have no `cu29::units` type
- **Symptom:** `copper-component-design` asks for `cu29::units` types for physical quantities
  in public payloads, and rules out both bare numeric fields whose unit is only in a name or
  comment and local unit wrappers. `TrackState` holds positions in px, velocities in px/s and
  variances in px² and (px/s)².
- **Cause:** `cu29_units` wraps `uom`'s SI quantities, and pixels aren't SI, so no type fits
  and the rule leaves no third way.
- **Fix or workaround:** plain `f32` fields with their units in the doc comments, as
  `Detection` does, and as `cu_sensor_payloads`' `CuImageBufferFormat` does for its width and
  height in pixels (without saying so). Time isn't in this gap: the tracker's times and
  durations are Copper's `CuTime` and `CuDuration`, and only the filter maths inside works in
  `f64` seconds. The first version held them as bare `u64` ns and `f64` s, which the M3
  review caught.
- **Upstream?** *Upstream*, in copper-rs-skills: say what image-plane quantities should use.
- **Time lost:** 5 min.

### 2026-11-23 · M3 · A scripted edit wrote CRLF line endings into four files
- **Symptom:** after a small Python edit, git warned that CRLF would be replaced by LF in
  `Cargo.toml`, `src/lib.rs`, `src/main.rs` and `src/tasks/mod.rs`.
- **Cause:** `pathlib.Path.write_text` writes in text mode, which on Windows turns each `\n`
  into `\r\n`; this clone's working tree is LF.
- **Fix or workaround:** `sed -i 's/\r$//'` on the four files, before any build or commit.
  Scripts that write files here pass `newline="\n"`.
- **Upstream?** *Ours*.
- **Time lost:** 2 min.

### 2026-11-23 · M3 · Perf debt in the tracker and its view
- **Symptom:** what the M3 tasks add on the hot path, as the plan asks:
  - `KalmanTracker`: nothing. `process()` works on fixed-size arrays and a `TrackState` on
    the stack, and reads the clock twice to time itself; it doesn't allocate, lock or log.
    `postprocess()` logs confirmations, recoveries, losses and dropped messages, which takes
    the log's lock outside `process()`.
  - `RerunTrackViz`: in each frame with a target, `Lost` included, three `log` calls on top
    of the image and the detections: the box, the velocity arrow and the lead point with its
    label, each with 0 or 1 instances, which Rerun copies into Arrow buffers and queues. A
    `Clear` once when the target goes. The label is rewritten in place in a string
    allocated at start.
- **Cause:** Rerun's API takes owned data.
- **Fix or workaround:** accepted: the plan's M3 asks the viz to show the box, velocity and
  lead point, which takes these calls.
- **Upstream?** *Ours*.
- **Time lost:** none.

### 2026-11-23 · M3 · A target outlived `max_coast_ms` when a detection fell in its gate
- **Symptom:** in the first headless run on a synthetic stand-in (a moving ball, all COCO
  classes kept), the tracker logged `target recovered ... 1000 ms after its previous
  detection` with `max_coast_ms` at 1000, so a coast had passed its limit and still
  recovered. The M3 review found the worse case: after a 10 s freeze of the stream, like the
  one in M1's drop test, the first frame's prediction spans 10 s, its gate covers the whole
  640x360 frame (radius about 8000 px), and the track jumps to any detection with no loss
  logged and no confirmation.
- **Cause:** `follow()` predicted across the whole gap and checked `max_coast` only when no
  detection fell in the gate. A cycle without a frame gives the tracker no message, so a
  stall reaches it as one long `dt`.
- **Fix or workaround:** `follow()` checks the gap first: past `max_coast` the target is
  `Lost`, holding the estimate from before, whatever the frame holds, and the next frame's
  detection starts a candidate. A test covers a 2 s gap. `max_coast_ms` of 0 is now refused,
  and its doc says it must be longer than the frame interval, since it also bounds a
  candidate's gaps.
- **Upstream?** *Ours*.
- **Time lost:** 20 min.

### 2026-11-23 · M3 · Copper's integer config reads wrap negative values
- **Symptom:** a RON value of `-1` for a key read with `config.get::<u32>(...)` comes back as
  4294967295, not an error. `"max_coast_ms": -1` would never lose a target, and
  `"lead_ms": -50` would put the lead point millions of px away.
- **Cause:** `impl_try_from_value_for_int!` in `core/cu29_runtime/src/config/value.rs` converts
  each RON number with `*n as $target`, so a negative or too-large value wraps or truncates.
- **Fix or workaround:** the tracker reads its integer keys as `i64` and checks the range with
  `u32::try_from`, naming the key in the error. The other tasks still use `get::<u32>`.
- **Upstream?** *Upstream*, in `cu29_runtime`: convert with `<$target>::try_from(*n)` and return a
  range error.
- **Time lost:** 5 min.

### 2026-11-23 · M3 · Rerun fits the 2D view to every overlay, and puts box labels under the box
- **Symptom:** found by the M3 review, before the first live run: an arrow or a coasting
  estimate drawn past the frame's edge would make Rerun zoom the view out and keep
  re-fitting it, and the target's label would sit on the detection's label, because the
  target's box has the same place and size as the detection it took.
- **Cause:** with no pinhole and no blueprint, Rerun 0.43's 2D view frames the union of
  everything drawn in it (a single image as the default bounds is a TODO in
  `re_view_spatial`), and it draws each `Boxes2D` label centred under the box's bottom edge,
  without avoiding overlaps.
- **Fix or workaround:** the viz cuts the target to the frame (the box to its visible part,
  the arrow where it leaves the frame, the lead point onto the nearest edge), and puts one
  label, with the status, hits and speed, on the lead point. Lines and dots are sized in UI
  points.
- **Upstream?** *Ours*.
- **Time lost:** 15 min.

### 2026-11-23 · M3 · The camera stream stopped twice mid-run, with nothing reported
- **Symptom:** in M3's live check on the SKYLOCK camera (CUDA, Rerun on, Ahyan moving in
  view), frames stopped arriving 194 s into a 240 s run and 25 s into the next, 120 s run.
  The loop kept running at its rate target for the rest of each run, the tracker and the log
  carried on with empty cycles, and nothing logged an error, only `no frames received`. The
  Ethernet link stayed up. A third run of 120 s with GStreamer's warnings on
  (`GST_DEBUG=2,rtspsrc:4`) ran clean at 30 fps, as did the day's 30 s headless run and M1's
  10-minute run. GStreamer logged only a decoder warning at start (a missing reference
  picture, normal when joining mid-stream) and, at Ctrl+Break, `PAUSE interrupted` and
  `receive interrupted` from `rtspsrc`: the runs end without a clean RTSP teardown, so the
  camera keeps a stale session until it times it out. Both stops came within about 1 s of a
  new log slab, but the 20 other new slabs in today's camera runs didn't stop the stream.
  Ahyan isn't sure whether the camera's cable was touched during the first run.
- **Cause:** not known. `cu_gstreamer` doesn't watch the GStreamer bus, so whatever ended the
  stream in the two failing runs, which had no `GST_DEBUG`, went unreported.
- **Fix or workaround:** none: reconnecting and reporting were M5's, which is parked. Run with
  `GST_DEBUG=2`, so that a stop leaves GStreamer's reason in the app's output.
- **Upstream?** *Upstream*, in `cu_gstreamer`, as for the M1 drop test: surface bus errors and
  EOS, and reconnect. Whether the camera or our client ends the session is ours to find.
- **Time lost:** 25 min.

### 2026-11-23 · M6 · Ctrl+C in the app's first seconds kills it before it closes its log
- **Symptom:** checking the README's log-reader commands, an 8 s run against an address where
  nothing listens ended with exit code 0xC000013A, not 0, and `fsck` then refused its log:
  "The log has a temporary end marker; its writer has not closed it cleanly" (our log reader
  turns the error into a panic, exit 101). The camera source had spent about 6 s in `new()`
  while GStreamer failed to connect. A 20 s run stopped cleanly on Ctrl+Break, and both
  commands worked on its log.
- **Cause:** the runtime macro installs its Ctrl+C handler at the start of the generated
  `run()` (`core/cu29_derive/src/runtime_macro.rs`), after the builder has built the tasks.
  Until then Windows' default handler ends the process, and the log stays open.
- **Fix or workaround:** stop a run only once it has started; the README says so.
- **Upstream?** *Upstream*, in `cu29_derive`: install the handler before the tasks are built,
  so that a stop during start-up still closes the log.
- **Time lost:** 5 min.

### 2026-11-25 · M1 · With `async-cl-io` the stalls leave the loop, and the log gets holes
- **Symptom:** Copper's author asked whether the seal stalls had been tested with
  `async-cl-io`; they hadn't. Two 10-minute runs of one commit, back to back, set up as in
  "With the rate target, the stalls stay": the synthetic stand-in (H.265 1920x1080 at
  30 fps), CUDA, ring 8, `RERUN=off`, 32 MiB sections, `rate_target_hz: 120`. The second run
  differs only by `"async-cl-io"` among cu29's features in the example's `Cargo.toml`.
  - Inline output on 2026-11-22 (that entry's run, with the pose model): 37 gaps of 100 ms
    or more (100 to 578 ms, median 292 ms); 17,584 of about 17,676 frames (0.5 % dropped);
    longest CopperList 90 ms; a 12.4 GB log in 47 slabs holding all 45,296 lists.
  - Inline output, the day's control: 197 gaps (100 to 675 ms, median 138 ms; 182 of them
    after the first 10 s of frames); 17,484 of about 17,584 frames (0.6 % dropped, PTS steps
    up to 433 ms); longest CopperList 112 ms; a 12.3 GB log in 47 slabs holding all 62,456
    lists, and `fsck` passes.
  - `async-cl-io`: 2 gaps (100 and 178 ms); 17,671 of about 17,671 frames (none dropped:
    the PTS step never passed 41 ms); longest CopperList 162 ms; an 11.9 GB log in 46 slabs.
    Ctrl+Break ended the run in 0.5 s with exit code 0 and a cleanly closed log, and
    `extract-text-log`, `fsck` and `log-stats` read it. But `fsck` reports "The log is
    structurally valid but contains CopperList holes": of 70,525 lists, 19,989 (28 %) are
    missing, in 16,715 ranges. Of the 17,970 lists that carried a frame, 703 (3.9 %) are
    missing, and so are 4 of the 8 keyframes.
  - In the control the gaps sit at the section seals, as before: 179 of the 197, lining
    each section's first CopperList id up with the gaps. About half of its 370 seals cost a
    gap of 100 ms or more, against one in ten on 2026-11-22, while the gaps over 300 ms are
    as many (18 and 17). Why more seals pass 100 ms on this day isn't known.
- **Cause:** with `async-cl-io` a `cu-output` thread writes the completed CopperLists, and
  the loop hands each one over without waiting. The seal's whole-slab flush is unchanged: it
  now blocks that thread instead of the loop, and what the thread can't take is dropped.
  - `logging.copperlist_count` defaults to 2 (`DEFAULT_COPPERLIST_COUNT` in
    `core/cu29_runtime/src/config.rs`), and the async manager keeps one list for the loop:
    `try_submit` in `core/cu29_runtime/src/curuntime.rs` drops a list once
    `pending_count >= NBCL - 1`. So one list can wait for the writer, and a list that
    completes while the one before it is still being written is dropped and counted.
    Copper's `doc/design/concurrent-logging.md` says so: the hand-off never blocks, and
    "counters and missing CopperList IDs expose the gap".
  - That drops the list after each frame. A pass with a frame takes about 13 ms, past the
    8.3 ms period, so the next pass starts at once and ends, empty, while the writer is
    still copying the frame. `fsck`'s first holes are #4, #8, #12 and so on, at four passes
    per frame. Listing the log's ids (next entry) gives 16,370 missing lists of this kind.
  - It also drops whatever completes during a seal's flush, frames included: 3,619 lists,
    with all 703 frames. A seal held the writer for 50 ms at the median and for 567 ms at
    the longest.
  - A run without a terminal doesn't report the drops. The runtime counts them
    (`CopperListIoStats`), and the count is a row on `cu_consolemon`'s screen; `cu_logmon`'s
    line and the text log don't carry it. `fsck` shows the holes afterwards.
  - The two gaps left are in the camera task's `process()` (162 ms and 69 ms in the
    monitor's lines), which only pops the ring and, on an empty pass, writes a `debug!`
    line. The text log still seals its 40 KiB sections, with the same whole-slab flush, on
    the thread that logs (32 seals in this run); the design note says structured logs
    "still serialize on their callers' threads". The 178 ms gap began 16 ms before one of
    those seals. The 100 ms gap matches no seal, and its cause isn't known.
- **Fix or workaround:** not adopted: the example stays on inline output, so its log stays
  complete. To repeat the run, add `"async-cl-io"` to cu29's features in the example's
  `Cargo.toml`; the build, clippy and the tests pass with it. A larger `copperlist_count`
  is the next entry.
- **Upstream?** *Upstream*, in `cu29_unifiedlog`, as for the seal stalls: the flush is the
  cause, and `async-cl-io` moves it without shortening it. Two smaller points, for `cu29`
  and `cu_logmon`: the feature's description says that drops are reported, but not that the
  default `copperlist_count` of 2 lets one list wait; and `cu_logmon` doesn't print the
  count, so a run without a terminal has no report of the lists it drops.
- **Time lost:** 1.5 hours.

### 2026-11-26 · M1 · `copperlist_count` 8, 32 and 128 under `async-cl-io`: only 128 keeps every list
- **Symptom:** Copper's author, on the run above: "no need to have too many copperlists but
  more than 2 can help if this is just a spike like this". Three more 10-minute runs, with
  the commit, stream, settings and `Cargo.lock` of the 2026-11-25 `async-cl-io` run. Only
  `logging.copperlist_count` changes, and with it `gst_to_image`'s `pool_size`, set to the
  count plus 2.
  - Count 2 (2026-11-25): 2 gaps of 100 ms or more; 19,989 of 70,525 lists missing (28.3 %)
    in 16,715 ranges, 703 of them with a frame, and 4 of 8 keyframes; 11.9 GB in 46 slabs.
  - Count 8: 2 gaps (133 and 157 ms); 17,675 of about 17,676 frames; longest CopperList
    114 ms; 1,217 of 70,256 lists missing (1.7 %) in 141 ranges, 284 with a frame, no
    keyframe; 12.2 GB in 47 slabs.
  - Count 32: 7 gaps (102 to 236 ms, median 110 ms); 17,650 of about 17,690 frames; longest
    CopperList 236 ms; 346 of 70,192 lists missing (0.5 %) in 19 ranges, 85 with a frame, no
    keyframe; 12.4 GB in 47 slabs. Six of the gaps are one burst of slow inference (next
    entry), not the log.
  - Count 128: 2 gaps (113 and 139 ms); 17,593 of about 17,593 frames; longest CopperList
    107 ms; all 70,482 lists and all 8 keyframes in the log, and `fsck` passes; 12.4 GB in
    47 slabs.
  - Where the missing lists are: at count 2, 3,619 inside a seal's flush and 16,370
    elsewhere, 16,305 of those holes right after a list with a frame. At 8 and 32 all of
    them are inside a seal's flush, and none anywhere else.
  - How long a seal held `cu-output`: median 40, 28 and 40 ms at 8, 32 and 128; longest 565,
    501 and 510 ms; 32, 33 and 50 of the 368 to 372 seals took over 100 ms. At count 2,
    from the log, the median was 50 ms and the longest 567 ms.
  - The process's private bytes peaked at 972, 985 and 1,017 MiB, which is the larger frame
    pool: 130 buffers of 345,600 bytes are 45 MB.
- **Cause:** a small count cures the list after each frame, because a few lists can wait
  while the writer copies one. It doesn't shorten a seal, so what is left depends on how many
  lists can wait out the flush. At 8, every seal that held the writer for 60 ms or more
  dropped lists, and some from 40 ms: 141 of the 368, up to 63 lists at once. At 32, the 19
  seals of 278 ms or more did, up to 32 lists at once, and none of 212 ms or less. At 128
  the queue outlasted the longest seal, 510 ms.
  One gap in each run is the text log's own seal, which still flushes on the loop thread:
  it started 13 to 34 ms into the gap and held for 139, 85 and 103 ms, the only text-log
  seals over 50 ms among the 31 or 32 in each run. The other gap at 8 and at 128 (133 and
  113 ms) matches no seal.
- **How it was measured:** local helper scripts read the log from outside the app. One
  polls the slab files about once a millisecond while the app runs: a seal closes the
  section's commit block, flushes, and only then creates the stream's next section, so the
  time between the two is how long the seal held its thread. The other lists the ids and
  times of the lists in a finished log, which gives every hole; its counts match `fsck`'s in
  all four logs. `pool_size` is the count plus 2 because a recycled list keeps its frame
  until the task overwrites it (`reset_for_runtime_use` in
  `core/cu29_runtime/src/copperlist.rs`), so every list can hold one buffer while the task
  takes another.
- **Fix or workaround:** not adopted: the example stays on inline output. `async-cl-io` with
  `copperlist_count: 128` and `pool_size: 130` gave a complete log and a loop with two gaps
  in 10 minutes. It rides out the flush, it doesn't fix it, and a slower disk or a longer
  flush would need more.
- **Upstream?** *Upstream*, in `cu29_unifiedlog`: the seal's whole-slab flush, as before.
  For `cu29`: say that `copperlist_count` 2 lets one list wait. For `cu29_export`: `fsck`
  lists the first 16 holes only, so the holes had to be read from the log's bytes.
- **Time lost:** 3 hours, half of it on the two helper scripts.

### 2026-11-26 · M1 · In one run the detector's inference stalled for 0.8 s
- **Symptom:** 476 s into the count-32 run above, six frames in a row took the detector 106
  to 236 ms each, against its usual 13 ms. That made six gaps of 106 to 236 ms within 0.8 s
  and cost about 40 frames at the source's ring (PTS steps up to 168 ms). The nearest log
  seal was 0.8 s away.
- **Cause:** not known. It is on the GPU side of the detector (the driver, heat, or another
  program using the GPU). The other five 10-minute runs of these two days don't show it.
- **Fix or workaround:** none. The run was kept, with the burst named beside its numbers.
- **Upstream?** *Ours*.
- **Time lost:** 10 min.

### 2026-11-26 · M1 · A log section can run across two slab files, and lists have two header shapes
- **Symptom:** the first version of the helper script that lists a log's CopperList ids
  found 48,481 of the 50,536 lists `fsck` counts in the 2026-11-25 log, and a later one
  6,396 of 6,410 in a test log.
- **Cause:** two wrong guesses about the log's bytes. A section can start in one slab and
  end in the next, because the log is one byte-addressed space cut into slab files. And a
  list's header, after its id, starts with `02 0f` or, for some of a run's first lists, a
  single `00`.
- **Fix or workaround:** the script joins the two parts of such a section and accepts both
  shapes, and its counts are checked against `fsck`'s for every log before they are used.
- **Upstream?** *Ours*. An `fsck` option to list every hole would have made the script
  unnecessary.
- **Time lost:** 40 min.

### 2026-11-26 · M1 · A scripted edit turned byte escapes into raw bytes
- **Symptom:** the seal-timing helper script stopped compiling ("bytes can only contain
  ASCII literal characters") after an edit made through a shell here-document, and the
  count-8 run that had just started with it had to be stopped and started again.
- **Cause:** the edit's doubled backslashes reached Python single, so it wrote the two bytes
  where the escape text belonged.
- **Fix or workaround:** repaired with an exact replace. Text with backslashes now goes
  through an editor or a script file, and each helper is compiled before a run starts.
- **Upstream?** *Ours*.
- **Time lost:** 10 min.

### 2026-11-26 · M6 · Moving to Copper `cdfb08e` and into its own repository broke nothing
- **Symptom:** none, which is worth a line. The detector, the tracker and the example app
  left the fork, on Copper `cd321a8`, for this repository, on Copper `cdfb08e`, 63 commits
  later. Every source file compiled unchanged against the new commit: `cargo check`, clippy
  with `-D warnings` and the same 40 tests passed on the first build.
- **Cause:** nothing these crates call changed its signature in those commits. Three things
  changed by choice:
  - The config names the tasks and messages by crate (`cu_skylock_detect::YoloDetect`,
    `cu_skylock_payloads::Detections`). That also ends the `gen_cumsgs!` workaround, because
    both macros resolve a crate path the same way.
  - The app builder gets `.save_effective_config(false)` (#1664). With it, and with the
    pipeline gone from the source's start-up message (`3a2f849`), a made-up password in the
    URL no longer shows anywhere in the log's bytes.
  - The camera has a `frame_mode` (#1707); see the next entry.
- **Fix or workaround:** none needed. Copper is pinned by a full commit hash, spelled the
  same for every Copper crate, and Copper's own bincode comes from that commit too.
- **Upstream?** *Ours*.
- **Time lost:** none.

### 2026-11-26 · M6 · `frame_mode` "newest" drops 5 % of the frames while the loop waits on the log
- **Symptom:** two 5-minute runs of the example on the synthetic stream (CUDA, no viewer,
  inline output, every COCO class kept so that the tracker has the stream's moving ball):
  - `fifo`: 8,622 of about 8,629 frames, and the source counted 7 dropped; frames waited
    p50 38 ms, p95 78 ms and at most 258 ms before the loop; 71 gaps of 100 ms or more. The
    tracker held a target in 505 of 8,919 frames, confirmed 55, recovered 277 and lost 54.
  - `newest`: 8,135 of about 8,541 frames, and the source counted 451 dropped; frames waited
    p50 16 ms, p95 24 ms and at most 55 ms; 63 gaps. The tracker held a target in 368 of
    8,390 frames, confirmed 41, recovered 203 and lost 41.
  The detector took p50 14.1 and 14.2 ms a frame, and `fsck` passed both logs.
- **Cause:** with inline output the loop stalls at each section seal, and frames queue in
  the source's ring meanwhile. `fifo` then hands them over late, one per pass; `newest`
  hands over the latest and counts the others as dropped. The tracker confirms a target on
  detections in consecutive frames, so dropped frames cost it confirmations.
- **Fix or workaround:** the example stays on `fifo`. `newest` would suit a loop that
  doesn't stall. This is one run each, and YOLOv8n sees the ball in a minority of frames, so
  the tracker's counts are a weak measure.
- **Upstream?** *Ours*, a choice of setting. #1707's count is right: the 7 it reported under
  `fifo` are the 7 frames missing from the PTS sequence.
- **Time lost:** 15 min.

### 2026-11-26 · M6 · Cargo's own git client crawled on the Copper dependency
- **Symptom:** the first `cargo fetch` for this repository sat at "Updating git repository
  `https://github.com/copper-project/copper-rs`" for five minutes and had 18 MB of it.
- **Cause:** not looked into: Cargo's built-in git client. With git's command line the
  whole fetch, crates included, took three minutes.
- **Fix or workaround:** set `CARGO_NET_GIT_FETCH_WITH_CLI=true` for the first build, or
  `net.git-fetch-with-cli = true` in Cargo's config.
- **Upstream?** *Ours* (Cargo on this machine).
- **Time lost:** 6 min.
