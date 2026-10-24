//! Wayland session: background layer surfaces on every output, shm buffers,
//! render on change. Raw `wayland-client` 0.31 + `wlr-layer-shell` (the
//! protocol every wlroots-family compositor — niri included — takes
//! wallpapers with). Single-threaded; the `Daemon` struct here is the
//! EventQueue state and is also polled alongside the IPC socket in `super`.

use std::fs::File;
use std::os::fd::AsFd;

use memmap2::MmapMut;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{Anchor, Event as LayerEvent, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use pixcore::{Config, Paths};

use super::render;

pub const NAMESPACE: &str = "pixelated-papers";

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
    pub bw: u32, // buffer (physical) dims = cfg × scale
    pub bh: u32,
    buffers: [ShmBuf; 2],
    pub dirty: bool,
    pub label: Option<String>,
}

impl Drop for Out {
    fn drop(&mut self) {
        // tell the compositor goodbye; dropping proxies alone wouldn't
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
        if cache_hit { self.hits += 1 } else { self.misses += 1 }
        Self::push(&mut self.render_ms, ms, 200);
    }
}

/// Everything the daemon owns; also the wayland EventQueue state.
pub struct Daemon {
    pub paths: Paths,
    pub compositor: Option<WlCompositor>,
    pub shm: Option<WlShm>,
    pub layer_shell: Option<ZwlrLayerShellV1>,
    pub outputs: Vec<Out>,
    pub running: bool,
    /// The compositor went away (niri restart / session end): exit 43.
    pub compositor_lost: bool,
    pub bench: Bench,
    next_id: usize,
    qh: Option<QueueHandle<Daemon>>,
}

impl Daemon {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            compositor: None,
            shm: None,
            layer_shell: None,
            outputs: Vec::new(),
            running: true,
            compositor_lost: false,
            bench: Bench::default(),
            next_id: 0,
            qh: None,
        }
    }

    pub fn set_qh(&mut self, qh: QueueHandle<Daemon>) {
        self.qh = Some(qh);
    }

    fn globals_ready(&self) -> bool {
        self.compositor.is_some() && self.shm.is_some() && self.layer_shell.is_some()
    }

    /// Create a background layer surface for any output that lacks one
    /// (called when globals arrive and on output hotplug).
    pub fn ensure_surfaces(&mut self) {
        if !self.globals_ready() {
            return;
        }
        let (Some(comp), Some(lshell), Some(qh)) =
            (self.compositor.clone(), self.layer_shell.clone(), self.qh.clone())
        else {
            return;
        };
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
            log::info!("surface created for output {} (reg {})", out.name, out.reg_name);
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
        let Some(entry) = render::current_entry(&self.paths) else {
            log::debug!("no current entry; skipping render");
            return;
        };
        let Some(out) = self.outputs.iter_mut().find(|o| o.id == id) else { return };
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
        self.bench.record_render(t0.elapsed().as_millis() as u64, rendered.as_ref().map(|r| r.1).unwrap_or(false));
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
        let Some(buf) = out.buffers[slot].buffer.clone() else { return };
        surface.set_buffer_scale(out.scale.max(1) as i32);
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

    pub fn snapshot(&self) -> Vec<(String, u32, u32, u32, Option<String>)> {
        self.outputs
            .iter()
            .map(|o| (o.name.clone(), o.bw, o.bh, o.scale, o.label.clone()))
            .collect()
    }
}

/// shm buffer alloc: file → mmap → pool → buffer.
fn ensure_buf(
    b: &mut ShmBuf,
    shm: WlShm,
    qh: &QueueHandle<Daemon>,
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

impl Dispatch<WlRegistry, ()> for Daemon {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global { name, interface, version } => {
                match interface.as_str() {
                    "wl_compositor" => {
                        state.compositor =
                            Some(registry.bind::<WlCompositor, _, _>(name, version.min(4), qh, ()))
                    }
                    "wl_shm" => {
                        state.shm = Some(registry.bind::<WlShm, _, _>(name, version.min(1), qh, ()))
                    }
                    "zwlr_layer_shell_v1" => {
                        state.layer_shell = Some(registry.bind::<ZwlrLayerShellV1, _, _>(
                            name,
                            version.min(4),
                            qh,
                            (),
                        ));
                        log::info!("layer-shell v{version} bound");
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
                        });
                        state.next_id += 1;
                        state.ensure_surfaces();
                    }
                    _ => {}
                }
            }
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

impl Dispatch<WlOutput, usize> for Daemon {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: wl_output::Event,
        id: &usize,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(out) = state.outputs.iter_mut().find(|o| o.id == *id) else { return };
        match event {
            wl_output::Event::Scale { factor } => {
                let new = factor.max(1) as u32;
                if new != out.scale {
                    out.scale = new;
                    // buffer size depends on scale → rebuild slots
                    for b in out.buffers.iter_mut() {
                        *b = ShmBuf::default();
                    }
                    if out.configured {
                        out.bw = out.cfg_w * new;
                        out.bh = out.cfg_h * new;
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

impl Dispatch<ZwlrLayerSurfaceV1, usize> for Daemon {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        id: &usize,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(out) = state.outputs.iter_mut().find(|o| o.id == *id) else { return };
        match event {
            LayerEvent::Configure { serial, width, height } => {
                if let Some(layer) = &out.layer {
                    layer.ack_configure(serial);
                }
                let w = width.max(1);
                let h = height.max(1);
                let changed = w != out.cfg_w || h != out.cfg_h;
                out.cfg_w = w;
                out.cfg_h = h;
                if changed {
                    for b in out.buffers.iter_mut() {
                        *b = ShmBuf::default();
                    }
                }
                out.bw = w.saturating_mul(out.scale.max(1));
                out.bh = h.saturating_mul(out.scale.max(1));
                out.configured = true;
                log::info!(
                    "configure {}: {w}×{h} logical → {}×{} buffer (scale {})",
                    out.name,
                    out.bw,
                    out.bh,
                    out.scale
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

impl Dispatch<WlBuffer, (usize, usize)> for Daemon {
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

wayland_client::delegate_noop!(Daemon: ignore WlCompositor);
wayland_client::delegate_noop!(Daemon: ignore WlRegion);
wayland_client::delegate_noop!(Daemon: ignore WlShm);
wayland_client::delegate_noop!(Daemon: ignore WlShmPool);
wayland_client::delegate_noop!(Daemon: ignore WlSurface);
wayland_client::delegate_noop!(Daemon: ignore ZwlrLayerShellV1);
