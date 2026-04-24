use std::time::Instant;

/// Fixed 2-second ring buffer at 60fps (120 samples, stack-only, no heap alloc)
#[derive(Debug, Clone)]
pub struct FrameRingBuffer {
    samples: [f32; 120],
    head: usize,
    count: usize,
}

#[allow(clippy::new_without_default)]
impl FrameRingBuffer {
    pub fn new() -> Self {
        Self {
            samples: [0.0; 120],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, ms: f32) {
        self.samples[self.head % 120] = ms;
        self.head = self.head.wrapping_add(1);
        if self.count < 120 {
            self.count += 1;
        }
    }

    pub fn avg(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let sum: f32 = self.samples[..self.count].iter().sum();
        sum / self.count as f32
    }

    fn sorted_copy(&self) -> ([f32; 120], usize) {
        let mut buf = [0.0f32; 120];
        buf[..self.count].copy_from_slice(&self.samples[..self.count]);
        buf[..self.count].sort_unstable_by(|a, b| a.total_cmp(b));
        (buf, self.count)
    }

    pub fn p50(&self) -> f32 {
        let (buf, n) = self.sorted_copy();
        if n == 0 {
            return 0.0;
        }
        buf[(n * 50 / 100).min(n - 1)]
    }

    pub fn p95(&self) -> f32 {
        let (buf, n) = self.sorted_copy();
        if n == 0 {
            return 0.0;
        }
        buf[(n * 95 / 100).min(n - 1)]
    }

    pub fn p99(&self) -> f32 {
        let (buf, n) = self.sorted_copy();
        if n == 0 {
            return 0.0;
        }
        buf[(n * 99 / 100).min(n - 1)]
    }

    pub fn sample_count(&self) -> usize {
        self.count
    }
}

#[derive(Debug, Clone)]
pub struct HudState {
    pub visible: bool,
    pub buffer: FrameRingBuffer,
    pub last_render_ms: Option<f32>,
    last_frame: Option<Instant>,
    last_hud_update: Option<Instant>,
    cached_fps: u32,
    cached_p95: f32,
}

impl HudState {
    pub fn new(start_visible: bool) -> Self {
        Self {
            visible: start_visible,
            buffer: FrameRingBuffer::new(),
            last_render_ms: None,
            last_frame: None,
            last_hud_update: None,
            cached_fps: 0,
            cached_p95: 0.0,
        }
    }

    pub fn record_frame(&mut self, now: Instant) {
        if let Some(prev) = self.last_frame {
            let elapsed_ms = (now - prev).as_secs_f32() * 1000.0;
            if elapsed_ms > 0.0 {
                self.buffer.push(elapsed_ms);
                self.last_render_ms = Some(elapsed_ms);
            }
        }
        self.last_frame = Some(now);

        // Refresh cached display values at most every 500ms
        let should_refresh = self
            .last_hud_update
            .map(|t| t.elapsed().as_millis() >= 500)
            .unwrap_or(true);
        if should_refresh {
            let avg_ms = self.buffer.avg();
            self.cached_fps = if avg_ms > 0.0 {
                (1000.0 / avg_ms) as u32
            } else {
                0
            };
            self.cached_p95 = self.buffer.p95();
            self.last_hud_update = Some(now);
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        log::info!("HUD {}", if self.visible { "shown" } else { "hidden" });
    }

    /// Returns ["fps: N", "p95: N.NNms"] — two static-lifetime-free strings
    pub fn text_lines(&self) -> [String; 2] {
        [
            format!("fps: {}", self.cached_fps),
            format!("p95: {:.2}ms", self.cached_p95),
        ]
    }
}
