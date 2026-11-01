//! The X11 backend (PLAN-30 §B.2): the same wallpaper on every screen's
//! root window, mirroring the layer backend's multi-output behavior.
//! `x11rb`'s pure-Rust XCB connection has a pollable fd, so it drops into
//! the supervisor's existing `poll(2)` loop unchanged.
//!
//! Render = the **same `pixcore::render::composite_into`** the layer
//! backend uses (invariant #2) into a client-side XRGB8888 buffer, then
//! `PutImage` to the root. No Xv, no GPU — v0.2 does not depend on either.
//! We never initiate a resize: RFPWM/RRC ClientMessages (and the Expose a
//! root resize always produces) make us re-query and re-render.
//!
//! Depth: the buffer is XRGB8888 on a depth-24 root. A non-24-depth root
//! is a *user error* — loud exit 1 at connect, tested not hoped for
//! (PLAN-30 §B.2 risk row).
//!
//! Teardown: `close` clears every root to solid black before disconnect —
//! the daemon takes the wallpaper with it, so a stopped daemon (or the
//! exit-43 path, best-effort) leaves no stray frame on the X server.

use std::collections::VecDeque;
use std::os::fd::{AsRawFd, RawFd};

use pixcore::{Config, Paths};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConnectionExt as _, CreateGCAux, EventMask, ImageFormat, ImageOrder,
    Rectangle,
};
use x11rb::rust_connection::RustConnection;

use crate::daemon::DaemonStartupError;
use crate::daemon::render;
use crate::ipc::OutputStatus;

use super::{Backend, BenchCounters};

/// Rolling counters, same shape and caps as the layer backend's `Bench`
/// so `status --bench` reads identically on both backends.
#[derive(Default)]
struct Bench {
    applies: u64,
    last_switch_ms: u64,
    switch_ms: VecDeque<u64>,
    render_ms: VecDeque<u64>,
    hits: u64,
    misses: u64,
}

impl Bench {
    fn push(dq: &mut VecDeque<u64>, v: u64, cap: usize) {
        if dq.len() == cap {
            dq.pop_front();
        }
        dq.push_back(v);
    }
    fn median(dq: &VecDeque<u64>) -> u64 {
        let mut v: Vec<u64> = dq.iter().copied().collect();
        v.sort_unstable();
        if v.is_empty() { 0 } else { v[v.len() / 2] }
    }
    fn max(dq: &VecDeque<u64>) -> u64 {
        dq.iter().copied().max().unwrap_or(0)
    }
    fn record_apply(&mut self, ms: u64) {
        self.applies += 1;
        self.last_switch_ms = ms;
        Self::push(&mut self.switch_ms, ms, 100);
    }
    fn record_render(&mut self, ms: u64, cache_hit: bool) {
        if cache_hit {
            self.hits += 1
        } else {
            self.misses += 1
        }
        Self::push(&mut self.render_ms, ms, 200);
    }
}

/// One screen: its root window, current size, and the client-side buffer
/// that gets PutImage'd (u32 words in the *display's* byte order).
struct ScreenOut {
    root: u32,
    gc: u32,
    w: u32,
    h: u32,
    buf: Vec<u32>,
    label: Option<String>,
}

pub struct X11Backend {
    conn: RustConnection,
    paths: Paths,
    screens: Vec<ScreenOut>,
    /// Display byte order (Setup image_byte_order); LSBFirst on all
    /// sane local servers, but we honor whatever the server reports.
    lsb: bool,
    lost: bool,
    /// RFPWM arrived but the re-query showed no size change yet (the
    /// server applies the resize after the message); the next Expose
    /// re-checks unconditionally.
    pending_resize: bool,
    wm_moveresize: u32,
    action_resize: u32,
    bench: Bench,
}

/// Depth gate (§B.2): the buffer is XRGB8888 on a depth-24 root; any
/// other depth is a loud user error — tested, not hoped for.
fn check_root_depth(screen: usize, depth: u8) -> Result<(), String> {
    if depth == 24 {
        Ok(())
    } else {
        Err(format!(
            "screen {screen} root depth is {depth}, but pixpapers' x11 backend needs a 24-bit \
             truecolor display — configure your X server for depth 24 and restart"
        ))
    }
}

impl X11Backend {
    /// Connect to `$DISPLAY` (x11rb resolves it), validate the depth,
    /// intern the RFPWM atoms and select events on every root.
    pub fn connect_to_env(paths: Paths) -> Result<Self, DaemonStartupError> {
        let (conn, _screen) = x11rb::connect(None)
            .map_err(|e| DaemonStartupError(format!("no x11 session (DISPLAY?): {e}")))?;
        let setup = conn.setup();
        for (i, scr) in setup.roots.iter().enumerate() {
            check_root_depth(i, scr.root_depth).map_err(DaemonStartupError)?;
        }
        let lsb = setup.image_byte_order == ImageOrder::LSB_FIRST;
        let mut screens: Vec<ScreenOut> = setup
            .roots
            .iter()
            .map(|s| ScreenOut {
                root: s.root,
                gc: 0,
                w: s.width_in_pixels as u32,
                h: s.height_in_pixels as u32,
                buf: vec![0u32; s.width_in_pixels as usize * s.height_in_pixels as usize],
                label: None,
            })
            .collect();
        // PutImage needs a real GC (gc=0 is BadGContext); plane-mask default.
        for s in screens.iter_mut() {
            let cid = conn
                .generate_id()
                .map_err(|e| DaemonStartupError(format!("x11 id alloc failed: {e}")))?;
            conn.create_gc(cid, s.root, &CreateGCAux::default())
                .map_err(|e| DaemonStartupError(format!("x11 create_gc failed: {e}")))?;
            s.gc = cid;
        }
        let atom = |name: &[u8]| {
            conn.intern_atom(false, name)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| r.atom)
                .unwrap_or(0)
        };
        let wm_moveresize = atom(b"_NET_WM_MOVERESIZE");
        let action_resize = atom(b"_NET_WM_ACTION_RESIZE");
        // Expose (root repaint/resize) + the structure masks that deliver
        // root ClientMessages (RFPWM/RRC). We react; we never redirect.
        let mask = EventMask::EXPOSURE
            | EventMask::STRUCTURE_NOTIFY
            | EventMask::SUBSTRUCTURE_NOTIFY
            | EventMask::SUBSTRUCTURE_REDIRECT;
        for s in &screens {
            conn.change_window_attributes(
                s.root,
                &ChangeWindowAttributesAux::new().event_mask(Some(mask)),
            )
            .map_err(|e| DaemonStartupError(format!("select input on root failed: {e}")))?;
        }
        conn.flush()
            .map_err(|e| DaemonStartupError(format!("x11 flush failed: {e}")))?;
        log::info!(
            "x11: connected, {} screen(s), byte order {}",
            screens.len(),
            if lsb { "LSB" } else { "MSB" }
        );
        Ok(Self {
            conn,
            paths,
            screens,
            lsb,
            lost: false,
            pending_resize: false,
            wm_moveresize,
            action_resize,
            bench: Bench::default(),
        })
    }

    /// Re-query every root's size; rebuild buffers on change. True if any
    /// screen's size changed.
    fn requery_sizes(&mut self) -> bool {
        let mut changed = false;
        for (i, s) in self.screens.iter_mut().enumerate() {
            match self
                .conn
                .get_geometry(s.root)
                .ok()
                .and_then(|c| c.reply().ok())
            {
                Some(geo) => {
                    let (w, h) = (geo.width as u32, geo.height as u32);
                    if (w, h) != (s.w, s.h) {
                        log::info!("x11: screen {i} is now {w}×{h}, re-rendering");
                        s.w = w;
                        s.h = h;
                        s.buf = vec![0u32; w as usize * h as usize];
                        s.label = None;
                        changed = true;
                    }
                }
                None => {
                    log::error!("x11 get_geometry(screen {i}) failed (connection gone?)");
                    self.lost = true;
                    return false;
                }
            }
        }
        changed
    }

    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::Expose(_) => {
                // A root resize always exposes; re-query so a resize we
                // learned about only via RFPWM lands at the new size.
                if self.requery_sizes() {
                    self.pending_resize = false;
                }
                self.render_all_inner();
            }
            Event::ClientMessage(e) => {
                if e.type_ == self.wm_moveresize && self.action_resize != 0 {
                    let l = e.data.as_data32();
                    if l[0] & self.action_resize != 0 {
                        // RFPWM resize: react, never initiate (§B.2).
                        log::info!("x11: RFPWM resize hint received");
                        let changed = self.requery_sizes();
                        self.pending_resize = !changed;
                        self.render_all_inner();
                    }
                }
            }
            Event::Error(e) => log::warn!("x11 protocol error: {e:?}"),
            _ => {}
        }
    }

    /// Composite + PutImage on every screen (trait `render_all` and the
    /// event handlers share this path).
    fn render_all_inner(&mut self) {
        if self.lost {
            return;
        }
        let cfg = match Config::load(&self.paths.config_file) {
            Ok(c) => c,
            Err(e) => {
                log::error!("config load failed: {e}");
                return;
            }
        };
        let Some(entry) = render::active_entry(&self.paths) else {
            log::debug!("no current entry; skipping render");
            return;
        };
        for (i, s) in self.screens.iter_mut().enumerate() {
            if s.w == 0 || s.h == 0 {
                continue;
            }
            let t0 = std::time::Instant::now();
            let rendered = render::render_into(&self.paths, &cfg, &entry, (s.w, s.h), &mut s.buf);
            self.bench.record_render(
                t0.elapsed().as_millis() as u64,
                rendered.as_ref().map(|r| r.1).unwrap_or(false),
            );
            match rendered {
                Ok((label, hit)) => {
                    s.label = Some(label);
                    log::debug!(
                        "x11: rendered screen {i} ({}×{}) in {} ms (variant cache {})",
                        s.w,
                        s.h,
                        t0.elapsed().as_millis(),
                        if hit { "hit" } else { "MISS" }
                    );
                }
                Err(e) => {
                    log::error!("x11 render for screen {i} failed: {e:#}");
                    continue;
                }
            }
            // Blit: convert the composite words to the display's byte
            // order in place, then PutImage them to the root in row bands
            // that fit the server's maximum request length (BigReq-aware).
            for w in s.buf.iter_mut() {
                // depth-24 pixel: the top byte is padding; store the word
                // in the display's byte order (swap only when host and
                // display disagree)
                let v = *w & 0x00FF_FFFF;
                let host_le = cfg!(target_endian = "little");
                *w = if self.lsb == host_le {
                    v
                } else {
                    v.swap_bytes()
                };
            }
            let bytes: &[u8] =
                unsafe { std::slice::from_raw_parts(s.buf.as_ptr() as *const u8, s.buf.len() * 4) };
            let row_bytes = s.w as usize * 4;
            let max = self
                .conn
                .maximum_request_bytes()
                .saturating_sub(64)
                .max(row_bytes);
            let rows_per = (max / row_bytes).max(1);
            let mut y = 0usize;
            let mut res: Result<(), x11rb::errors::ConnectionError> = Ok(());
            while y < s.h as usize && res.is_ok() {
                let band = rows_per.min(s.h as usize - y);
                res = self
                    .conn
                    .put_image(
                        ImageFormat::Z_PIXMAP,
                        s.root,
                        s.gc,
                        s.w as u16,
                        band as u16,
                        0,
                        y as i16,
                        0, // left_pad
                        24,
                        &bytes[y * row_bytes..(y + band) * row_bytes],
                    )
                    .map(|_| ());
                y += band;
            }
            if let Err(e) = self.conn.flush().and(res) {
                log::error!("x11 blit to screen {i} failed: {e}");
            }
        }
    }
}

impl Backend for X11Backend {
    fn name(&self) -> &'static str {
        "x11"
    }

    fn poll_fd(&self) -> Option<RawFd> {
        Some(self.conn.stream().as_raw_fd())
    }

    fn pump(&mut self) {
        if self.lost {
            return;
        }
        if let Err(e) = self.conn.flush() {
            // EPIPE/EOF here means the X server is gone (session end):
            // clean teardown, then exit 43 — same contract as wayland.
            log::error!("x11 conn.flush: {e}");
            self.lost = true;
            return;
        }
        // poll_for_event is non-blocking (internal buffer first, then one
        // non-blocking read); drain until it reports empty.
        loop {
            match self.conn.poll_for_event() {
                Ok(Some(ev)) => self.handle_event(ev),
                Ok(None) => break,
                Err(e) => {
                    log::error!("x11 connection error: {e}");
                    self.lost = true;
                    break;
                }
            }
        }
    }

    fn render_all(&mut self, paths: &Paths) {
        self.paths = paths.clone();
        self.render_all_inner();
    }

    fn snapshot(&self) -> Vec<OutputStatus> {
        self.screens
            .iter()
            .enumerate()
            .map(|(i, s)| OutputStatus {
                name: format!("screen-{i}"),
                width: s.w,
                height: s.h,
                scale: 1, // X11 has no fractional scale (§B.2)
                variant: s.label.clone(),
            })
            .collect()
    }

    fn bench_counters(&self) -> BenchCounters {
        BenchCounters {
            applies: self.bench.applies,
            last_switch_ms: self.bench.last_switch_ms,
            switch_median_ms: Bench::median(&self.bench.switch_ms),
            switch_max_ms: Bench::max(&self.bench.switch_ms),
            render_median_ms: Bench::median(&self.bench.render_ms),
            render_max_ms: Bench::max(&self.bench.render_ms),
            variant_hits: self.bench.hits,
            variant_misses: self.bench.misses,
        }
    }

    fn connection_lost(&self) -> bool {
        self.lost
    }

    fn close(&mut self) {
        // The daemon takes the wallpaper with it: every root is cleared to
        // solid black before we disconnect, so a stopped daemon leaves no
        // stray frame on the X server. On the exit-43 path the server is
        // already gone and these requests simply fail — best effort, never
        // a teardown blocker.
        for s in self.screens.iter() {
            if let Ok(cid) = self.conn.generate_id() {
                // The GC id is the one we hand to create_gc (0.14 hands
                // back a cookie, like the connect path keeps).
                if self
                    .conn
                    .create_gc(cid, s.root, &CreateGCAux::new().foreground(0))
                    .is_ok()
                {
                    let _ = self.conn.poly_fill_rectangle(
                        s.root,
                        cid,
                        &[Rectangle {
                            x: 0,
                            y: 0,
                            width: s.w as u16,
                            height: s.h as u16,
                        }],
                    );
                    let _ = self.conn.free_gc(cid);
                }
            }
        }
        // Free our GCs, flush and drain what the server still owes us
        // before the supervisor removes socket/pid.
        for s in self.screens.iter() {
            let _ = self.conn.free_gc(s.gc);
        }
        self.screens.clear();
        let _ = self.conn.flush();
        while let Ok(Some(_)) = self.conn.poll_for_event() {}
    }
}

/// Apply-latency recording for the Apply IPC (mirrors the layer path).
impl X11Backend {
    pub fn record_apply(&mut self, ms: u64) {
        self.bench.record_apply(ms);
    }
}

#[cfg(test)]
mod tests {
    use super::check_root_depth;

    #[test]
    fn depth_24_accepted() {
        assert!(check_root_depth(0, 24).is_ok());
    }

    #[test]
    fn depth_16_refused_loudly() {
        let e = check_root_depth(0, 16).unwrap_err();
        assert!(e.contains("depth is 16"), "{e}");
        assert!(e.contains("24-bit"), "{e}");
    }

    #[test]
    fn depth_32_refused_names_screen() {
        let e = check_root_depth(1, 32).unwrap_err();
        assert!(e.contains("screen 1"), "{e}");
        assert!(e.contains("depth is 32"), "{e}");
    }
}
