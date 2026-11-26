//! Convert a GStreamer buffer into a CuImage for downstream tasks.
//!
//! It also keeps the frame-delivery statistics of [`crate::frame_stats`]: a line every
//! `stats_period_s` seconds (default 10, 0 turns it off) and a summary when the task stops.
//! With `stats_skip_s`, the summary leaves out the first seconds, such as a camera's
//! start-up burst of old frames. Each period also lists the gaps between frames of at least
//! `stall_ms` (default 100, 0 turns it off), and pairs Copper's clock with the system clock,
//! so the gaps can be lined up with outside events such as new log files. The summary also
//! gives the camera source's count of dropped frames, which the source puts in its
//! messages' status text.

use crate::frame_stats::FrameStats;
use cu_gstreamer::CuGstBuffer;
use cu_sensor_payloads::{CuImage, CuImageBufferFormat};
use cu29::bincode::{Decode, Encode};
use cu29::prelude::*;
use std::ops::DerefMut;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static GST_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct GstToCuImage {
    format: CuImageBufferFormat,
    #[reflect(ignore)]
    pool: Arc<CuHostMemoryPool<Vec<u8>>>,
    last_payload_ns: Option<u64>,
    last_warn_ns: u64,
    #[reflect(ignore)]
    stats: FrameStats,
    stats_period_ns: u64,
    last_stats_ns: u64,
    stats_skip_ns: u64,
    stats_restart_at_ns: Option<u64>,
    /// The source's dropped-frame count, as its last message reported it.
    source_dropped: u64,
}

impl Freezable for GstToCuImage {
    fn freeze<E: cu29::bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), cu29::bincode::error::EncodeError> {
        Encode::encode(&self.last_payload_ns, encoder)?;
        Encode::encode(&self.last_warn_ns, encoder)?;
        Ok(())
    }

    fn thaw<D: cu29::bincode::de::Decoder>(
        &mut self,
        decoder: &mut D,
    ) -> Result<(), cu29::bincode::error::DecodeError> {
        self.last_payload_ns = Decode::decode(decoder)?;
        self.last_warn_ns = Decode::decode(decoder)?;
        Ok(())
    }
}

impl CuTask for GstToCuImage {
    type Resources<'r> = ();
    type Input<'m> = input_msg!(CuGstBuffer);
    type Output<'m> = output_msg!(CuImage<Vec<u8>>);

    fn new(config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self>
    where
        Self: Sized,
    {
        let config = config.ok_or_else(|| CuError::from("GstToCuImage requires configuration"))?;

        let width = config
            .get::<u32>("width")?
            .ok_or_else(|| CuError::from("GstToCuImage requires width"))?;
        let height = config
            .get::<u32>("height")?
            .ok_or_else(|| CuError::from("GstToCuImage requires height"))?;
        let fourcc = if let Some(fourcc) = config.get::<String>("fourcc")? {
            fourcc
        } else if let Some(pixel_format) = config.get::<String>("pixel_format")? {
            pixel_format
        } else {
            return Err(CuError::from(
                "GstToCuImage requires fourcc (or pixel_format)",
            ));
        };
        let pixel_format = fourcc_to_bytes(&fourcc)?;
        let stride = config
            .get::<u32>("stride")?
            .unwrap_or_else(|| default_stride(width, pixel_format));
        let format = CuImageBufferFormat {
            width,
            height,
            stride,
            pixel_format,
        };
        if !format.is_valid() {
            return Err(CuError::from(format!(
                "Invalid image layout for fourcc={} width={} height={} stride={}",
                fourcc, width, height, stride
            )));
        }

        let buffer_size = format.byte_size();
        let pool_size = config.get::<u32>("pool_size")?.unwrap_or(4) as usize;
        let pool_id = config
            .get::<String>("pool_id")?
            .unwrap_or_else(|| "gst_image_pool".to_string());

        let pool = CuHostMemoryPool::new(&pool_id, pool_size, || vec![0u8; buffer_size])?;
        let stats_period_s = config.get::<u32>("stats_period_s")?.unwrap_or(10);
        let stats_skip_s = config.get::<u32>("stats_skip_s")?.unwrap_or(0);
        let stall_ms = config.get::<u32>("stall_ms")?.unwrap_or(100);
        let mut stats = FrameStats::new();
        stats.set_stall_threshold_ns(u64::from(stall_ms) * 1_000_000);

        Ok(Self {
            format,
            pool,
            last_payload_ns: None,
            last_warn_ns: 0,
            stats,
            stats_period_ns: u64::from(stats_period_s) * 1_000_000_000,
            last_stats_ns: 0,
            stats_skip_ns: u64::from(stats_skip_s) * 1_000_000_000,
            stats_restart_at_ns: None,
            source_dropped: 0,
        })
    }

    fn start(&mut self, ctx: &CuContext) -> CuResult<()> {
        let now_ns = ctx.now().as_nanos();
        self.last_stats_ns = now_ns;
        self.stats_restart_at_ns = (self.stats_skip_ns > 0).then(|| now_ns + self.stats_skip_ns);
        log_clock_pair(now_ns);
        Ok(())
    }

    fn process(
        &mut self,
        ctx: &CuContext,
        input: &Self::Input<'_>,
        output: &mut Self::Output<'_>,
    ) -> CuResult<()> {
        let now_ns = ctx.now().as_nanos();
        // `cu_gstreamer` reports the frames its ring dropped as "dropped: N", with or
        // without a frame in the message.
        if let Some(dropped) = input
            .metadata
            .status_txt
            .0
            .strip_prefix("dropped: ")
            .and_then(|count| count.parse::<u64>().ok())
        {
            self.source_dropped = dropped;
        }
        let Some(buffer) = input.payload() else {
            const WARN_AFTER_NS: u64 = 2_000_000_000;
            const WARN_EVERY_NS: u64 = 2_000_000_000;
            let last_payload_ns = self.last_payload_ns.unwrap_or(0);
            if now_ns.saturating_sub(last_payload_ns) >= WARN_AFTER_NS
                && now_ns.saturating_sub(self.last_warn_ns) >= WARN_EVERY_NS
            {
                warning!(
                    "gst_to_image: no frames received for {} ms",
                    now_ns.saturating_sub(last_payload_ns) / 1_000_000
                );
                self.last_warn_ns = now_ns;
            }
            output.clear_payload();
            return Ok(());
        };
        self.last_payload_ns = Some(now_ns);
        if let Tov::Time(tov) = input.tov {
            let pts_ns = buffer.as_live().and_then(|b| b.pts()).map(|t| t.nseconds());
            self.stats.record(tov.as_nanos(), pts_ns);
        }

        let mapped = buffer
            .map_readable()
            .map_err(|e| CuError::new_with_cause("Failed to map GStreamer buffer", e))?;
        let src = mapped.as_slice();
        let min_len = self.format.required_bytes();
        let log_idx = GST_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
        if log_idx < 5 {
            info!(
                "gst_to_image: buf_len={} min_len={} width={} height={} stride={} fourcc={}",
                src.len(),
                min_len,
                self.format.width,
                self.format.height,
                self.format.stride,
                String::from_utf8_lossy(&self.format.pixel_format)
            );
        }
        if src.len() < min_len {
            return Err(CuError::from(format!(
                "GStreamer buffer too small: expected >= {}, got {} bytes",
                min_len,
                src.len()
            )));
        }

        // Acquire a buffer from the pool instead of allocating
        let handle = self
            .pool
            .acquire()
            .ok_or_else(|| CuError::from("Failed to acquire buffer from image pool"))?;

        // Copy the source data into the pooled buffer
        handle.with_inner_mut(|inner| {
            let dest = inner.deref_mut();
            let copy_len = src.len().min(dest.len());
            dest[..copy_len].copy_from_slice(&src[..copy_len]);
        });

        let image = CuImage::new(self.format, handle);
        output.tov = input.tov;
        output.set_payload(image);
        Ok(())
    }

    fn postprocess(&mut self, ctx: &CuContext) -> CuResult<()> {
        let now_ns = ctx.now().as_nanos();
        if let Some(restart_at_ns) = self.stats_restart_at_ns
            && now_ns >= restart_at_ns
        {
            self.stats.reset_run();
            self.stats_restart_at_ns = None;
            info!(
                "gst_to_image: run statistics restart, leaving out the first {} s",
                self.stats_skip_ns / 1_000_000_000
            );
        }
        let elapsed_ns = now_ns.saturating_sub(self.last_stats_ns);
        if self.stats_period_ns == 0 || elapsed_ns < self.stats_period_ns {
            return Ok(());
        }
        let period = self.stats.take_period();
        match period.min_lag_ns {
            Some(min_lag_ns) => {
                info!(
                    "gst_to_image: {} frames in {} ms, longest gap {} ms, lowest lag {} ms from the first frame's",
                    period.frames,
                    elapsed_ns / 1_000_000,
                    period.max_gap_ns / 1_000_000,
                    min_lag_ns / 1_000_000
                );
            }
            None => {
                info!(
                    "gst_to_image: {} frames in {} ms, longest gap {} ms",
                    period.frames,
                    elapsed_ns / 1_000_000,
                    period.max_gap_ns / 1_000_000
                );
            }
        }
        for stall in period.stalls() {
            info!(
                "gst_to_image: gap of {} ms before the frame at {} ms",
                stall.gap_ns / 1_000_000,
                stall.tov_ns / 1_000_000
            );
        }
        if period.stalls_missed > 0 {
            info!(
                "gst_to_image: {} more gaps over the stall threshold in the period",
                period.stalls_missed
            );
        }
        log_clock_pair(now_ns);
        self.last_stats_ns = now_ns;
        Ok(())
    }

    fn stop(&mut self, _ctx: &CuContext) -> CuResult<()> {
        let run = self.stats.summary();
        info!(
            "gst_to_image: run: {} frames, {} without PTS, {} PTS resets, longest gap {} ms",
            run.frames,
            run.frames_without_pts,
            run.pts_resets,
            run.max_gap_ns / 1_000_000
        );
        info!(
            "gst_to_image: run: PTS span {} ms, PTS step {} to {} ms",
            run.pts_span_ns.map_or(0, |ns| ns / 1_000_000),
            run.min_pts_step_ns.map_or(0, |ns| ns / 1_000_000),
            run.max_pts_step_ns / 1_000_000
        );
        if let Some(lag) = run.lag {
            info!(
                "gst_to_image: run: lag above minimum p50 {} ms, p95 {} ms, p99 {} ms, max {} ms, {} out of range",
                lag.p50_ms, lag.p95_ms, lag.p99_ms, lag.max_ms, lag.out_of_range
            );
        }
        info!(
            "gst_to_image: run: the source dropped {} frames since it started",
            self.source_dropped
        );
        Ok(())
    }
}

/// Logs Copper's time next to the system clock's, both in ms, so that times in the log can
/// be matched with file times. Copper's clock drifts against the system clock (BREAKLOG.md),
/// which is why every period logs a fresh pair.
fn log_clock_pair(copper_ns: u64) {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    info!(
        "gst_to_image: clock pair: Copper {} ms is Unix {} ms",
        copper_ns / 1_000_000,
        unix_ms
    );
}

fn default_stride(width: u32, pixel_format: [u8; 4]) -> u32 {
    match &pixel_format {
        b"YUYV" => width * 2,
        b"BGR3" | b"RGB3" | b"BGR " | b"RGB " => width * 3,
        _ => width,
    }
}

fn fourcc_to_bytes(fourcc: &str) -> CuResult<[u8; 4]> {
    let bytes = fourcc.as_bytes();
    if bytes.len() != 4 {
        return Err(CuError::from("fourcc must be exactly 4 characters"));
    }
    let mut out = [0u8; 4];
    out.copy_from_slice(&bytes[0..4]);
    Ok(out)
}
