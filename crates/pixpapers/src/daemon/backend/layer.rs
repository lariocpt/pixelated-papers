//! The layer-shell backend: background layer surfaces on every output, shm
//! buffers, render on change. Raw `wayland-client` 0.31 + `wlr-layer-shell`
//! (the protocol every wlroots-family compositor — niri included — takes
//! wallpapers with). Single-threaded; the supervisor in `crate::daemon`
//! drives this through [`Backend`] and polls the connection fd alongside
//! the IPC socket. `LayerState` is the EventQueue state.

use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, RawFd};

use memmap2::MmapMut;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{
        Anchor, Event as LayerEvent, KeyboardInteractivity, ZwlrLayerSurfaceV1,
    },
};

use pixcore::{Config, Paths};

use crate::daemon::render;
use crate::ipc::OutputStatus;

use super::Backend;

pub const NAMESPACE: &str = "pixelated-papers";

// ── fractional-scale math (pure, unit-tested) ──────────────────────────
//
// `wp_fractional_scale_v1.preferred_scale` carries a u32 that is the
// numerator of a fraction with denominator 120 (1.5 ⇒ 180). The wl_fixed
// encoding that floated around in protocol drafts never shipped — the
// merged spec (and sway/niri on the wire) is uint/120.

/// Denominator of the `preferred_scale` numerator (spec: "the numerator
/// of a fraction with a denominator of 120").
pub const FRAC_DEN: u32 = 120;

/// Which buffer path an output renders through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScalePath {
    /// Legacy integer oversampling: buffer = logical × `wl_output.scale`,
    /// `wl_surface.set_buffer_scale(S)`. The only path before this
    /// feature; unchanged byte-for-byte.
    Legacy,
    /// Exact physical buffer: buffer = round(logical × frac), viewporter
    /// destination = logical, buffer scale stays 1.
    Exact,
}

/// Path selection. The exact path requires **both** protocols bound
/// (viewporter AND fractional-scale manager) **and** a negotiated
/// non-integer scale ≥ 1.0. Everything else — missing global, no
/// `preferred_scale` yet, or an integer scale — stays on the legacy
/// path, so compositors without fractional scale keep working unchanged.
pub fn select_scale_path(
    has_viewporter: bool,
    has_fractional_scale: bool,
    scale_num: Option<u32>,
) -> ScalePath {
    match scale_num {
        Some(n)
            if has_viewporter && has_fractional_scale && n >= FRAC_DEN && n % FRAC_DEN != 0 =>
        {
            ScalePath::Exact
        }
        _ => ScalePath::Legacy,
    }
}

/// Buffer dimension from a logical size and a `preferred_scale`
/// numerator: nearest integer, halves **away from zero** — the rounding
/// rule the fractional-scale spec states for surface sizes, applied to
/// buffer dims. Exact integer arithmetic, no floats:
/// `round_half_away(logical·n/120) = (2·logical·n + 120) / 240`.
pub fn buffer_dim(logical: u32, scale_num: u32) -> u32 {
    let num = 2u64 * logical as u64 * scale_num as u64 + FRAC_DEN as u64;
    ((num / (2 * FRAC_DEN as u64)).max(1)) as u32
}

/// The full geometry decision for one configured output:
/// `(buffer width, buffer height, viewport destination)`. Exact path:
/// buffer = round(logical × frac), destination = the logical size (the
/// compositor maps the physical buffer back onto it 1:1). Legacy path:
/// buffer = logical × integer scale, no destination (`None` — the
/// viewport stays inert or reset).
pub fn geometry(
    path: ScalePath,
    cfg_w: u32,
    cfg_h: u32,
    legacy_scale: u32,
    frac_num: Option<u32>,
) -> (u32, u32, Option<(u32, u32)>) {
    match path {
        ScalePath::Exact => {
            let n = frac_num.unwrap_or(FRAC_DEN);
            (buffer_dim(cfg_w, n), buffer_dim(cfg_h, n), Some((cfg_w, cfg_h)))
        }
        ScalePath::Legacy => (
            cfg_w.saturating_mul(legacy_scale.max(1)),
            cfg_h.saturating_mul(legacy_scale.max(1)),
            None,
        ),
    }
}


/// Double-buffered shm slot for one output.
#[derive(Default)]
pub struct ShmBuf {
    _file: Option<File>,
    mmap: Option<MmapMut>,
    _pool: Option<WlShmPool>,
    pub buffer: Option<WlBuffer>,
    busy: bool,
    bw: u32,
    bh: u32,
}

/// One physical output + its background surface.
pub struct Out {
    pub id: usize, // stable id, never reused
    reg_name: u32,
    pub name: String,
    output: WlOutput,
    pub scale: u32,
    surface: Option<WlSurface>,
    layer: Option<ZwlrLayerSurfaceV1>,
    cfg_w: u32, // configured logical size from layer configure
    cfg_h: u32,
    configured: bool,
    pub bw: u32, // buffer (physical) dims — see `recompute`
    pub bh: u32,
    buffers: [ShmBuf; 2],
    pub dirty: bool,
    pub label: Option<String>,
    /// wp_viewport for this surface — created only when the compositor
    /// offers viewporter AND fractional-scale (the exact path's pair).
    viewport: Option<WpViewport>,
    /// wp_fractional_scale_v1 for this surface (same condition).
    fscale: Option<WpFractionalScaleV1>,
    /// Last `preferred_scale` numerator (×120) the compositor sent.
    frac_num: Option<u32>,
    /// Current buffer path, recomputed on every configure/scale event.
    scale_path: ScalePath,
    /// Whether we currently have a viewport destination set (so the
    /// exact→legacy transition knows to reset it).
    dest_set: bool,
}

impl Out {
    /// Re-derive buffer dims + viewport destination from the current
    /// (configure size, legacy scale, negotiated fractional scale).
    /// Rebuilds the shm slots when the size changes. Returns whether the
    /// buffer size changed. The decision is a pure function of the three
    /// inputs (`select_scale_path` + `geometry`), so a settled output
    /// never flip-flops paths — no variant-cache thrash between paths.
    fn recompute(&mut self, has_viewporter: bool, has_fscale: bool) -> bool {
        self.scale_path = select_scale_path(has_viewporter, has_fscale, self.frac_num);
        let (bw, bh, dest) = if self.configured {
            geometry(self.scale_path, self.cfg_w, self.cfg_h, self.scale, self.frac_num)
        } else {
            (0, 0, None)
        };
        let changed = bw != self.bw || bh != self.bh;
        if changed {
            for b in self.buffers.iter_mut() {
                *b = ShmBuf::default();
            }
        }
        self.bw = bw;
        self.bh = bh;
        // Viewport bookkeeping: exact sets the destination to the logical
        // size every time (idempotent); a fall back to legacy resets it so
        // the buffer-scale path composites exactly as if no viewport
        // existed. A viewport whose destination was never set is inert —
        // integer-scale compositors see byte-for-byte unchanged behavior.
        if let Some(vp) = &self.viewport {
            match dest {
                Some((dw, dh)) => {
                    vp.set_destination(dw as i32, dh as i32);
                    self.dest_set = true;
                }
                None if self.dest_set => {
                    // set_destination(-1, -1) is the documented reset in
                    // this binding set (no separate reset request exists).
                    vp.set_destination(-1, -1);
                    self.dest_set = false;
                }
                None => {}
            }
        }
        changed
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        // tell the compositor goodbye; dropping proxies alone wouldn't
        if let Some(f) = self.fscale.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
        if let Some(l) = self.layer.take() {
            l.destroy();
        }
        if let Some(s) = self.surface.take() {
            s.destroy();
        }
        // wl_output v1 has no destroy request; the compositor frees it with
        // the connection. (v4 destroy isn't in the generated bindings.)
    }
}

/// Rolling latency stats — pure counters around the real render path, no
/// extra work (`status --bench` surfaces them; nothing samples on idle).
#[derive(Default)]
pub struct Bench {
    pub applies: u64,
    pub last_switch_ms: u64,
    pub switch_ms: std::collections::VecDeque<u64>, // last 100 Apply latencies
    pub render_ms: std::collections::VecDeque<u64>, // last 200 output renders
    pub hits: u64,
    pub misses: u64,
}

impl Bench {
    fn push(dq: &mut std::collections::VecDeque<u64>, v: u64, cap: usize) {
        if dq.len() == cap {
            dq.pop_front();
        }
        dq.push_back(v);
    }
    fn sorted(dq: &std::collections::VecDeque<u64>) -> Vec<u64> {
        let mut v: Vec<u64> = dq.iter().copied().collect();
        v.sort_unstable();
        v
    }
    pub fn switch_median_ms(&self) -> u64 {
        let v = Self::sorted(&self.switch_ms);
        if v.is_empty() { 0 } else { v[v.len() / 2] }
    }
    pub fn switch_max_ms(&self) -> u64 {
        self.switch_ms.iter().copied().max().unwrap_or(0)
    }
    pub fn render_median_ms(&self) -> u64 {
        let v = Self::sorted(&self.render_ms);
        if v.is_empty() { 0 } else { v[v.len() / 2] }
    }
    pub fn render_max_ms(&self) -> u64 {
        self.render_ms.iter().copied().max().unwrap_or(0)
    }
    pub fn record_apply(&mut self, ms: u64) {
        self.applies += 1;
        self.last_switch_ms = ms;
        Self::push(&mut self.switch_ms, ms, 100);
    }
    pub fn record_render(&mut self, ms: u64, cache_hit: bool) {
        if cache_hit {
            self.hits += 1
        } else {
            self.misses += 1
        }
        Self::push(&mut self.render_ms, ms, 200);
    }
}

/// The layer backend's state; also the wayland EventQueue state. The
/// supervisor owns loop termination (Quit/signal/lost); the state only
/// records that the compositor went away (niri restart / session end):
/// exit 43.
pub struct LayerState {
    pub paths: Paths,
    pub compositor: Option<WlCompositor>,
    pub shm: Option<WlShm>,
    pub layer_shell: Option<ZwlrLayerShellV1>,
    /// wp_viewporter — stable global; present on wlroots-family compositors
    /// (sway, niri, labwc…). Optional: absence pins the legacy path.
    pub viewporter: Option<WpViewporter>,
    /// wp_fractional_scale_manager_v1 — staging global. Exact buffers
    /// need BOTH this and the viewporter; either absence pins the legacy
    /// integer path (most compositors, unchanged).
    pub fscale_manager: Option<WpFractionalScaleManagerV1>,
    pub outputs: Vec<Out>,
    pub compositor_lost: bool,
    pub bench: Bench,
    next_id: usize,
    qh: Option<QueueHandle<LayerState>>,
}

impl LayerState {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            compositor: None,
            shm: None,
            layer_shell: None,
            viewporter: None,
            fscale_manager: None,
            outputs: Vec::new(),
            compositor_lost: false,
            bench: Bench::default(),
            next_id: 0,
            qh: None,
        }
    }

    pub fn set_qh(&mut self, qh: QueueHandle<LayerState>) {
        self.qh = Some(qh);
    }

    fn globals_ready(&self) -> bool {
        self.compositor.is_some() && self.shm.is_some() && self.layer_shell.is_some()
    }

    /// (viewporter?, fractional-scale manager?) — the pair the exact path
    /// needs. Tracked as one tuple so every recompute sees a consistent view.
    fn scale_globals(&self) -> (bool, bool) {
        (self.viewporter.is_some(), self.fscale_manager.is_some())
    }

    /// Attach the viewporter + fractional-scale helpers to a fresh surface.
    /// Only ever called with both globals present: the viewport must exist
    /// before `get_fractional_scale` (the spec pairs them), and one
    /// fractional-scale object per surface is the protocol's error case.
    fn attach_scale_helpers(
        out: &mut Out,
        vp: &WpViewporter,
        fsm: &WpFractionalScaleManagerV1,
        qh: &QueueHandle<LayerState>,
    ) {
        let Some(surface) = out.surface.clone() else {
            return;
        };
        out.viewport = Some(vp.get_viewport(&surface, qh, ()));
        out.fscale = Some(fsm.get_fractional_scale(&surface, qh, out.id));
    }

    /// A scale global just arrived: adopt the pair where needed and
    /// re-render every output whose buffer geometry changed.
    fn adopt_scale_globals(&mut self, conn: &Connection) {
        for id in self.adopt_scale_helpers() {
            self.render_output(id, conn);
        }
    }

    /// The compositor offered viewporter/fractional-scale *after* surfaces
    /// existed (registry event order is not guaranteed): adopt the pair on
    /// every surface and re-derive geometry. Returns the ids whose buffer
    /// size changed so the caller can re-render them.
    fn adopt_scale_helpers(&mut self) -> Vec<usize> {
        let (hv, hf) = self.scale_globals();
        let (Some(vp), Some(fsm), Some(qh)) =
            (self.viewporter.clone(), self.fscale_manager.clone(), self.qh.clone())
        else {
            return Vec::new();
        };
        let mut changed = Vec::new();
        for out in self.outputs.iter_mut() {
            if out.surface.is_some() && out.viewport.is_none() {
                Self::attach_scale_helpers(out, &vp, &fsm, &qh);
            }
            if out.recompute(hv, hf) {
                changed.push(out.id);
            }
        }
        changed
    }

    /// Create a background layer surface for any output that lacks one
    /// (called when globals arrive and on output hotplug).
    pub fn ensure_surfaces(&mut self) {
        if !self.globals_ready() {
            return;
        }
        let (Some(comp), Some(lshell), Some(qh)) = (
            self.compositor.clone(),
            self.layer_shell.clone(),
            self.qh.clone(),
        ) else {
            return;
        };
        let scale_pair = self
            .viewporter
            .clone()
            .zip(self.fscale_manager.clone())
            .zip(self.qh.clone());
        for out in self.outputs.iter_mut() {
            if out.surface.is_some() {
                continue;
            }
            let surface = comp.create_surface(&qh, ());
            let layer = lshell.get_layer_surface(
                &surface,
                Some(&out.output),
                Layer::Background,
                NAMESPACE.into(),
                &qh,
                out.id,
            );
            // Full output, reserve nothing, take no input (Background layer
            // doesn't take input per protocol anyway).
            layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1); // ignore panels' zones: true wallpaper
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            // Belt for lax compositors: empty input region ⇒ clicks always
            // pass through (the gesso pattern; protocol-wise redundant).
            let region = comp.create_region(&qh, ());
            surface.set_input_region(Some(&region));
            surface.commit();
            out.surface = Some(surface);
            out.layer = Some(layer);
            out.configured = false;
            // Exact-path pair, when the compositor offers both (the
            // compositor may also send preferred_scale right away).
            if let Some(((vp, fsm), sqh)) = &scale_pair {
                Self::attach_scale_helpers(out, vp, fsm, sqh);
            }
            log::info!(
                "surface created for output {} (reg {})",
                out.name,
                out.reg_name
            );
        }
    }

    // ── rendering ─────────────────────────────────────────────────────

    /// Re-render one output if configured. Slots are recycled; if both are
    /// still held by the compositor the render is deferred (`dirty`) and
    /// retried on buffer release.
    pub fn render_output(&mut self, id: usize, conn: &Connection) {
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
        let Some(out) = self.outputs.iter_mut().find(|o| o.id == id) else {
            return;
        };
        if !out.configured || out.bw == 0 || out.bh == 0 {
            return;
        }
        let (Some(surface), Some(shm), Some(qh)) =
            (out.surface.clone(), self.shm.clone(), self.qh.clone())
        else {
            return;
        };

        let slot = match (0..2).find(|i| !out.buffers[*i].busy) {
            Some(s) => s,
            None => {
                out.dirty = true;
                log::debug!("output {id}: no free buffer, deferring");
                return;
            }
        };

        {
            let b = &mut out.buffers[slot];
            if b.buffer.is_none() || b.bw != out.bw || b.bh != out.bh {
                if let Err(e) = ensure_buf(b, shm.clone(), &qh, out.bw, out.bh, (id, slot)) {
                    log::error!("buffer alloc failed on {}: {e:#}", out.name);
                    return;
                }
            }
        }

        let b = &mut out.buffers[slot];
        let Some(mmap) = b.mmap.as_mut() else { return };
        let words = mmap_to_words(mmap);
        let t0 = std::time::Instant::now();
        let rendered = render::render_into(&self.paths, &cfg, &entry, (out.bw, out.bh), words);
        self.bench.record_render(
            t0.elapsed().as_millis() as u64,
            rendered.as_ref().map(|r| r.1).unwrap_or(false),
        );
        match rendered {
            Ok((label, hit)) => {
                out.label = Some(label);
                log::debug!(
                    "rendered {} ({}×{} buffer) {} in {} ms (variant cache {})",
                    out.name,
                    out.bw,
                    out.bh,
                    out.label.as_deref().unwrap_or("?"),
                    t0.elapsed().as_millis(),
                    if hit { "hit" } else { "MISS" }
                );
            }
            Err(e) => {
                log::error!("render for {} failed: {e:#}", out.name);
                return;
            }
        }
        let Some(buf) = out.buffers[slot].buffer.clone() else {
            return;
        };
        // Exact path: the buffer already IS the physical grid, so the
        // buffer scale stays 1 and the viewport destination (set in
        // `recompute`) maps it onto the logical size 1:1. Legacy path:
        // unchanged integer scale.
        surface.set_buffer_scale(match out.scale_path {
            ScalePath::Exact => 1,
            ScalePath::Legacy => out.scale.max(1) as i32,
        });
        surface.attach(Some(&buf), 0, 0);
        surface.damage_buffer(0, 0, out.bw as i32, out.bh as i32);
        surface.commit();
        out.buffers[slot].busy = true;
        out.dirty = false;
        let _ = conn;
    }

    pub fn render_all(&mut self, conn: &Connection) {
        let ids: Vec<usize> = self.outputs.iter().map(|o| o.id).collect();
        for id in ids {
            self.render_output(id, conn);
        }
    }

    pub fn snapshot(&self) -> Vec<OutputStatus> {
        self.outputs
            .iter()
            .map(|o| OutputStatus {
                name: o.name.clone(),
                width: o.bw,
                height: o.bh,
                scale: o.scale,
                variant: o.label.clone(),
                // Additive observability (media-keys pattern): the width×
                // height above is the buffer size; these two say HOW it was
                // negotiated. scale_path is always known on this backend;
                // frac_scale only once the compositor sent preferred_scale.
                scale_path: Some(
                    match o.scale_path {
                        ScalePath::Exact => "exact",
                        ScalePath::Legacy => "legacy",
                    }
                    .to_string(),
                ),
                frac_scale: o
                    .frac_num
                    .map(|n| format!("{:.2}", n as f64 / FRAC_DEN as f64)),
            })
            .collect()
    }
}

/// shm buffer alloc: file → mmap → pool → buffer.
fn ensure_buf(
    b: &mut ShmBuf,
    shm: WlShm,
    qh: &QueueHandle<LayerState>,
    bw: u32,
    bh: u32,
    user: (usize, usize),
) -> anyhow::Result<()> {
    *b = ShmBuf::default();
    let size = (bw as usize) * (bh as usize) * 4;
    let file = tempfile::tempfile()?;
    file.set_len(size as u64)?;
    let mmap = unsafe { MmapMut::map_mut(&file)? };
    let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        bw as i32,
        bh as i32,
        (bw * 4) as i32,
        wl_shm::Format::Xrgb8888,
        qh,
        user,
    );
    b._file = Some(file);
    b.mmap = Some(mmap);
    b._pool = Some(pool);
    b.buffer = Some(buffer);
    b.bw = bw;
    b.bh = bh;
    Ok(())
}

fn mmap_to_words(mmap: &mut MmapMut) -> &mut [u32] {
    let len = mmap.len() / 4;
    // safety: /tmp page-aligned ⇒ 4-aligned; len*4 ≤ mmap.len()
    unsafe { std::slice::from_raw_parts_mut(mmap.as_mut_ptr() as *mut u32, len) }
}

// ── Dispatch impls ─────────────────────────────────────────────────────

impl Dispatch<WlRegistry, ()> for LayerState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" => {
                    state.compositor =
                        Some(registry.bind::<WlCompositor, _, _>(name, version.min(4), qh, ()))
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind::<WlShm, _, _>(name, version.min(1), qh, ()))
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell =
                        Some(registry.bind::<ZwlrLayerShellV1, _, _>(name, version.min(4), qh, ()));
                    log::info!("layer-shell v{version} bound");
                }
                "wp_viewporter" => {
                    state.viewporter =
                        Some(registry.bind::<WpViewporter, _, _>(name, version.min(1), qh, ()));
                    log::info!("viewporter bound");
                    state.adopt_scale_globals(conn);
                }
                "wp_fractional_scale_manager_v1" => {
                    state.fscale_manager = Some(registry.bind::<
                        WpFractionalScaleManagerV1,
                        _,
                        _,
                    >(name, version.min(1), qh, ()));
                    log::info!("fractional-scale manager bound");
                    state.adopt_scale_globals(conn);
                }
                "wl_output" => {
                    let id = state.next_id;
                    let out = registry.bind::<WlOutput, _, _>(name, version.min(4), qh, id);
                    state.outputs.push(Out {
                        id,
                        reg_name: name,
                        name: format!("output-{name}"),
                        output: out,
                        scale: 1,
                        surface: None,
                        layer: None,
                        cfg_w: 0,
                        cfg_h: 0,
                        configured: false,
                        bw: 0,
                        bh: 0,
                        buffers: Default::default(),
                        dirty: false,
                        label: None,
                        viewport: None,
                        fscale: None,
                        frac_num: None,
                        scale_path: ScalePath::Legacy,
                        dest_set: false,
                    });
                    state.next_id += 1;
                    state.ensure_surfaces();
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(pos) = state.outputs.iter().position(|o| o.reg_name == name) {
                    let out = state.outputs.remove(pos); // Drop destroys surface
                    log::info!("output {} removed", out.name);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, usize> for LayerState {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: wl_output::Event,
        id: &usize,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let (hv, hf) = state.scale_globals();
        let Some(out) = state.outputs.iter_mut().find(|o| o.id == *id) else {
            return;
        };
        match event {
            wl_output::Event::Scale { factor } => {
                let new = factor.max(1) as u32;
                if new != out.scale {
                    out.scale = new;
                    // Legacy buffer size depends on the integer scale. On
                    // the exact path the fractional negotiation wins and
                    // the legacy event alone changes nothing.
                    out.recompute(hv, hf);
                    if out.configured {
                        out.dirty = true;
                        state.render_output(*id, conn);
                    }
                }
            }
            wl_output::Event::Name { name } => {
                if out.name != name {
                    out.name = name;
                }
            }
            wl_output::Event::Done if out.configured && out.dirty => {
                state.render_output(*id, conn);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, usize> for LayerState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        id: &usize,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let (hv, hf) = state.scale_globals();
        let Some(out) = state.outputs.iter_mut().find(|o| o.id == *id) else {
            return;
        };
        match event {
            LayerEvent::Configure {
                serial,
                width,
                height,
            } => {
                if let Some(layer) = &out.layer {
                    layer.ack_configure(serial);
                }
                let w = width.max(1);
                let h = height.max(1);
                out.cfg_w = w;
                out.cfg_h = h;
                out.configured = true;
                // Slots are rebuilt inside recompute when the size moves.
                out.recompute(hv, hf);
                log::info!(
                    "configure {}: {w}×{h} logical → {}×{} buffer ({}{})",
                    out.name,
                    out.bw,
                    out.bh,
                    match out.scale_path {
                        ScalePath::Exact => "exact",
                        ScalePath::Legacy => "legacy",
                    },
                    match (out.scale_path, out.frac_num) {
                        (ScalePath::Exact, Some(n)) =>
                            format!(" · scale {:.2}", n as f64 / FRAC_DEN as f64),
                        _ => format!(" · scale {}", out.scale),
                    }
                );
                if let Some(surface) = &out.surface {
                    surface.commit(); // honour the configure
                }
                state.render_output(*id, conn);
            }
            LayerEvent::Closed => {
                out.configured = false;
                for b in out.buffers.iter_mut() {
                    *b = ShmBuf::default();
                }
                log::info!("surface closed for {}", out.name);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, (usize, usize)> for LayerState {
    fn event(
        state: &mut Self,
        _proxy: &WlBuffer,
        event: wl_buffer::Event,
        (id, slot): &(usize, usize),
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            if let Some(out) = state.outputs.iter_mut().find(|o| o.id == *id) {
                if let Some(b) = out.buffers.get_mut(*slot) {
                    b.busy = false;
                }
                if out.dirty {
                    state.render_output(*id, conn);
                }
            }
        }
    }
}

impl Dispatch<WpFractionalScaleV1, usize> for LayerState {
    fn event(
        state: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: <WpFractionalScaleV1 as Proxy>::Event,
        id: &usize,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let scale = match event {
            wp_fractional_scale_v1::Event::PreferredScale { scale } => scale,
            _ => return,
        };
        let (hv, hf) = state.scale_globals();
        let Some(out) = state.outputs.iter_mut().find(|o| o.id == *id) else {
            return;
        };
        if Some(scale) == out.frac_num {
            return; // repeat event, nothing moved
        }
        out.frac_num = Some(scale);
        log::info!(
            "fractional scale for {}: {scale}/120 = {:.2}",
            out.name,
            scale as f64 / FRAC_DEN as f64
        );
        out.recompute(hv, hf);
        if out.configured {
            out.dirty = true;
            state.render_output(*id, conn);
        }
    }
}

wayland_client::delegate_noop!(LayerState: ignore WlCompositor);
wayland_client::delegate_noop!(LayerState: ignore WlRegion);
wayland_client::delegate_noop!(LayerState: ignore WlShm);
wayland_client::delegate_noop!(LayerState: ignore WlShmPool);
wayland_client::delegate_noop!(LayerState: ignore WlSurface);
wayland_client::delegate_noop!(LayerState: ignore ZwlrLayerShellV1);
wayland_client::delegate_noop!(LayerState: ignore WpViewporter);
wayland_client::delegate_noop!(LayerState: ignore WpViewport);
wayland_client::delegate_noop!(LayerState: ignore WpFractionalScaleManagerV1);

// ── the backend the supervisor drives ───────────────────────────────────

/// The layer-shell [`Backend`]: owns the wayland connection, the event
/// queue and its state. The supervisor sees only the trait (PLAN-30
/// §B.1); `LayerState` remains the EventQueue state the Dispatch impls
/// above hang off.
pub struct LayerBackend {
    conn: Connection,
    queue: EventQueue<LayerState>,
    /// Public so the supervisor's status/bench plumbing can reach the
    /// counters (S4: one backend; S5's status rework moves this behind the
    /// trait).
    pub state: LayerState,
}

impl LayerBackend {
    /// Connect to the compositor from `WAYLAND_DISPLAY`, build the queue
    /// and state, and request the registry (events arrive via
    /// `Dispatch<WlRegistry, ()>`).
    pub fn connect_to_env(paths: Paths) -> anyhow::Result<Self> {
        let conn = match Connection::connect_to_env() {
            Ok(c) => c,
            Err(e) => {
                anyhow::bail!("no wayland session (WAYLAND_DISPLAY?): {e}");
            }
        };
        let queue = conn.new_event_queue();
        let qh = queue.handle();

        let mut state = LayerState::new(paths);
        state.set_qh(qh.clone());

        let registry = conn.display().get_registry(&qh, ());
        let _ = registry; // events arrive via Dispatch<WlRegistry, ()>

        Ok(Self { conn, queue, state })
    }
}

impl Backend for LayerBackend {
    fn name(&self) -> &'static str {
        "layer"
    }

    fn poll_fd(&self) -> Option<RawFd> {
        Some(self.conn.backend().poll_fd().as_raw_fd())
    }

    fn pump(&mut self) {
        if let Err(e) = self.conn.flush() {
            // EPIPE here means the compositor is gone (session end / niri
            // restart): clean teardown, then exit 43 (see TROUBLESHOOTING).
            log::error!("conn.flush: {e}");
            self.state.compositor_lost = true;
        }
        // prepare_read → read → dispatch_pending: the documented manual-loop
        // pattern. The read is non-blocking — a would-block means the
        // connection has nothing to give this tick (the fd wasn't ready);
        // the guard is consumed either way. dispatch_pending alone never
        // reads the socket, which is why globals never arrived before.
        if let Some(guard) = self.queue.prepare_read() {
            if let Err(e) = guard.read() {
                use wayland_client::backend::WaylandError;
                match e {
                    WaylandError::Io(ref io) if io.kind() == std::io::ErrorKind::WouldBlock => {}
                    _ => {
                        log::error!("read_events: {e}");
                        self.state.compositor_lost = true;
                    }
                }
            }
        }
        match self.queue.dispatch_pending(&mut self.state) {
            Ok(n) if n > 0 => log::debug!("dispatched {n} wayland events"),
            Err(wayland_client::DispatchError::Backend(e)) => {
                log::error!("dispatch backend error: {e}");
                self.state.compositor_lost = true;
            }
            _ => {}
        }
    }

    fn render_all(&mut self, paths: &Paths) {
        // The supervisor passes the Paths it built us with; keep the stored
        // copy authoritative for the render path.
        self.state.paths = paths.clone();
        self.state.render_all(&self.conn);
    }

    fn snapshot(&self) -> Vec<OutputStatus> {
        self.state.snapshot()
    }

    fn bench_counters(&self) -> super::BenchCounters {
        let b = &self.state.bench;
        super::BenchCounters {
            applies: b.applies,
            last_switch_ms: b.last_switch_ms,
            switch_median_ms: b.switch_median_ms(),
            switch_max_ms: b.switch_max_ms(),
            render_median_ms: b.render_median_ms(),
            render_max_ms: b.render_max_ms(),
            variant_hits: b.hits,
            variant_misses: b.misses,
        }
    }

    fn connection_lost(&self) -> bool {
        self.state.compositor_lost
    }

    fn close(&mut self) {
        // teardown: drop outputs (Out::Drop destroys surfaces), then flush
        // and drain the connection
        self.state.outputs.clear();
        let _ = self.conn.flush();
        let _ = self.queue.dispatch_pending(&mut self.state);
    }
}

// ── unit tests: the pure scale math (no compositor needed) ─────────────

#[cfg(test)]
mod scale_tests {
    use super::*;

    // preferred_scale numerators (×120) for the common scales.
    const S_100: u32 = 120; // 1.00
    const S_125: u32 = 150; // 1.25
    const S_150: u32 = 180; // 1.50
    const S_175: u32 = 210; // 1.75
    const S_200: u32 = 240; // 2.00

    #[test]
    fn buffer_dims_exact_at_common_scales() {
        // The dev-panel case: 2560×1440 logical at 1.5 → the panel's own
        // 3840×2160 grid (not the legacy 5120×2880 oversample).
        assert_eq!(geometry(ScalePath::Exact, 2560, 1440, 2, Some(S_150)).0, 3840);
        assert_eq!(geometry(ScalePath::Exact, 2560, 1440, 2, Some(S_150)).1, 2160);
        // The container testbed case: 1920×1080 at 1.5 → 2880×1620.
        assert_eq!(geometry(ScalePath::Exact, 1920, 1080, 2, Some(S_150)), (2880, 1620, Some((1920, 1080))));
        // 1.25 and 1.75 on the same logical size.
        assert_eq!(geometry(ScalePath::Exact, 1920, 1080, 2, Some(S_125)), (2400, 1350, Some((1920, 1080))));
        assert_eq!(geometry(ScalePath::Exact, 1920, 1080, 2, Some(S_175)), (3360, 1890, Some((1920, 1080))));
        // Integer numerators never reach the exact path (see path test),
        // but the math is defined: 1.0 and 2.0 are exact multiples.
        assert_eq!(buffer_dim(1920, S_100), 1920);
        assert_eq!(buffer_dim(1920, S_200), 3840);
    }

    #[test]
    fn viewport_destination_is_the_logical_size() {
        // Exact path: destination = configure size, whatever the scale.
        for n in [S_125, S_150, S_175] {
            let (bw, bh, dest) = geometry(ScalePath::Exact, 1366, 768, 2, Some(n));
            assert_eq!(dest, Some((1366, 768)), "destination must be logical at scale {n}/120");
            assert!(bw >= 1366 && bh >= 768, "buffer {bw}×{bh} must cover the logical size");
        }
        // Legacy path: no destination at all (viewport inert / reset).
        assert_eq!(geometry(ScalePath::Legacy, 1920, 1080, 2, Some(S_150)).2, None);
    }

    #[test]
    fn buffer_dims_round_half_away_from_zero() {
        // Rounding rule: nearest, halves away from zero — the spec's rule
        // for surface sizes. Exact integer arithmetic, so boundaries are
        // provable: logical·n/120 with remainder exactly 60 rounds up.
        // 2 × 1.25 = 2.5 → 3 (half away from zero)
        assert_eq!(buffer_dim(2, S_125), 3);
        // 6 × 1.75 = 10.5 → 11
        assert_eq!(buffer_dim(6, S_175), 11);
        // 10 × 1.5 = 15 exact (no rounding)
        assert_eq!(buffer_dim(10, S_150), 15);
        // non-half products round to nearest
        assert_eq!(buffer_dim(796, 150), 995); // 796×1.25 = 995 exact
        assert_eq!(buffer_dim(191, 180), 287); // 191×1.5 = 286.5 → 287
        assert_eq!(buffer_dim(189, 210), 331); // 189×1.75 = 330.75 → 331
        assert_eq!(buffer_dim(190, 210), 333); // 190×1.75 = 332.5 → 333
        // A dimension never collapses to zero (a 0-sized buffer is a
        // protocol error even at absurd numerators).
        assert_eq!(buffer_dim(0, S_100), 1);
    }

    #[test]
    fn path_selection_requires_both_protocols_and_fractional_scale() {
        // Full truth table over {viewporter?, fractional_scale?, scale}.
        // Exact requires BOTH protocols AND a non-integer scale.
        let cases: &[(&str, bool, bool, Option<u32>, ScalePath)] = &[
            ("no protocols, fractional scale", false, false, Some(S_150), ScalePath::Legacy),
            ("viewporter only", true, false, Some(S_150), ScalePath::Legacy),
            ("fractional-scale only", false, true, Some(S_150), ScalePath::Legacy),
            ("both protocols, fractional", true, true, Some(S_150), ScalePath::Exact),
            ("both protocols, 1.25", true, true, Some(S_125), ScalePath::Exact),
            ("both protocols, 1.75", true, true, Some(S_175), ScalePath::Exact),
            ("both protocols, no preferred_scale yet", true, true, None, ScalePath::Legacy),
            ("both protocols, integer 1.0", true, true, Some(S_100), ScalePath::Legacy),
            ("both protocols, integer 2.0", true, true, Some(S_200), ScalePath::Legacy),
            ("no protocols, integer", false, false, Some(S_200), ScalePath::Legacy),
            ("degenerate 0/120 stays legacy", true, true, Some(0), ScalePath::Legacy),
            ("sub-unit 90/120 (0.75) stays legacy", true, true, Some(90), ScalePath::Legacy),
        ];
        for (what, vp, fs, num, want) in cases {
            assert_eq!(
                select_scale_path(*vp, *fs, *num),
                *want,
                "case: {what}"
            );
        }
    }

    #[test]
    fn integer_scales_keep_the_legacy_geometry_byte_for_byte() {
        // The legacy geometry is exactly the pre-feature formula:
        // buffer = logical × integer scale, no viewport destination.
        assert_eq!(geometry(ScalePath::Legacy, 3840, 2160, 1, None), (3840, 2160, None));
        assert_eq!(geometry(ScalePath::Legacy, 2560, 1440, 2, None), (5120, 2880, None));
        assert_eq!(geometry(ScalePath::Legacy, 1920, 1080, 1, Some(S_100)), (1920, 1080, None));
        // Even with a fractional numerator in flight, a legacy decision
        // (missing protocol) uses the integer scale — the old oversample.
        assert_eq!(geometry(ScalePath::Legacy, 2560, 1440, 2, Some(S_150)), (5120, 2880, None));
    }

    #[test]
    fn path_selection_is_a_pure_function_of_its_inputs() {
        // No thrash: the same (globals, scale) inputs always select the
        // same path, so a settled output cannot flip paths between renders
        // (the variant canvas cache keys on the concrete buffer size).
        for _ in 0..8 {
            assert_eq!(select_scale_path(true, true, Some(S_150)), ScalePath::Exact);
            assert_eq!(select_scale_path(true, true, Some(S_100)), ScalePath::Legacy);
        }
    }
}
