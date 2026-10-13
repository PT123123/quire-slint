// quire-shot — headless visual regression tool.
//
// Renders the real AppWindow through Slint's software renderer into an
// offscreen buffer and writes a BMP (PNG encoding would need an extra
// dependency; the capture script converts to PNG). No window is created,
// so shots are deterministic and immune to foreground/z-order issues —
// see docs/UI_ARCHITECTURE.md "Visual regression".
//
// Build: cargo build --features software --bin quire-shot
// Usage: quire-shot --out shot.bmp [--scene menu] [--w 1280] [--h 800]
//        [--click x,y] [--key escape]
//        quire-shot --motion sidebar-toggle --scene default --gif clip.gif
//                   [--frames 20] [--frame-ms 40] [--gif-w 560] [--seq-dir dir]
//
// --click/--key dispatch a real input event through the window after the
// first render (and after the scene overlay), then re-render and print a
// probe line with the open/closed state of every popup. This is what
// verifies the close-on-click-outside behavior headlessly.
//
// --motion captures the *transit* rather than a state. The frames are not
// screenshots and not a redraw: Slint's animation tick is
// `Platform::duration_since_start()`, which this binary owns (see CLOCK), so
// frame k is the UI's own state at exactly k × --frame-ms into the motion — an
// `animate width { 220ms }` drawn at 40 ms, 80 ms, 120 ms… The property write
// per frame is `controller::apply_motion`, which writes the same property a
// real click would, and the easing is the engine's.

use image::codecs::gif::{GifEncoder, Repeat};
use image::imageops::FilterType;
use image::{Delay, Frame, ImageBuffer, ImageFormat, RgbaImage};
use quire::app::controller;
use quire::app::state::{AppState, HandleArgs};
use quire::{AppWindow, UIState};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model, PhysicalSize, Rgb8Pixel, SharedPixelBuffer, SharedString};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    static WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
    /// `--motion` drives Slint's clock by hand. The animation tick is
    /// `Platform::duration_since_start()`, so returning a counter the frame loop
    /// moves makes frame N be exactly `N × --frame-ms` into the motion.
    ///
    /// A wall-clock sleep cannot do that: rasterising 1280×800 in a debug build
    /// takes longer than the 220 ms the sidebar animates over, so every sample
    /// lands after the tween is finished and the clip records a jump while
    /// claiming a motion. Measured, not assumed — the first version of this file
    /// slept, and 22 frames came out as two distinct images.
    static CLOCK: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Nothing but the trait's own wall clock: `HeadlessPlatform` falls back to it
/// when no `--motion` capture is running, so a single-shot sweep keeps the
/// timing it has always had. Calling the default through a second type is the
/// only way to reach it without recursing into this one.
struct WallClock;

impl Platform for WallClock {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        unreachable!("used only for Platform::duration_since_start's default")
    }
}

struct HeadlessPlatform;

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        WINDOW.with(|slot| *slot.borrow_mut() = Some(window.clone()));
        Ok(window)
    }

    fn duration_since_start(&self) -> Duration {
        match CLOCK.with(|c| c.get()) {
            Some(ms) => Duration::from_millis(ms),
            None => WallClock.duration_since_start(),
        }
    }
}

/// Minimal 24-bit BMP writer (bottom-up rows, 4-byte padding).
fn write_bmp(path: &str, buffer: &SharedPixelBuffer<Rgb8Pixel>) -> Result<(), String> {
    let w = buffer.width() as usize;
    let h = buffer.height() as usize;
    let stride = (w * 3 + 3) & !3;
    let data_size = (stride * h + 54) as u32;
    let mut out = Vec::with_capacity(data_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bpp
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&((stride * h) as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes()); // 72 dpi
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    let src = buffer.as_bytes();
    for y in (0..h).rev() {
        let row = &src[y * w * 3..(y + 1) * w * 3];
        for px in row.chunks_exact(3) {
            out.push(px[2]);
            out.push(px[1]);
            out.push(px[0]);
        }
        out.resize(out.len() + (stride - w * 3), 0);
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
}

fn render(w: u32, h: u32) -> bool {
    WINDOW.with(|slot| -> bool {
        let window = slot.borrow().clone().expect("window adapter");
        window.draw_if_needed(|renderer| {
            let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
            let stride = buffer.width() as usize;
            renderer.render(buffer.make_mut_slice(), stride);
            BUFFER.with(|b| *b.borrow_mut() = Some(buffer));
        })
    })
}

fn parse(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// The rendered buffer as an RGBA image (the GIF encoder's currency).
fn to_rgba(buffer: &SharedPixelBuffer<Rgb8Pixel>) -> RgbaImage {
    let px: Vec<u8> = buffer
        .as_bytes()
        .chunks_exact(3)
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    ImageBuffer::from_raw(buffer.width(), buffer.height(), px).expect("frame matches pixel count")
}

/// Draw one frame now and hand back its pixels, downscaled to `small` wide
/// when `small` is set. The scale-down is the whole reason a clip of this shell
/// is a few hundred kilobytes rather than a few megabytes.
fn grab(window: &Rc<MinimalSoftwareWindow>, w: u32, h: u32, small: u32) -> Option<RgbaImage> {
    window.request_redraw();
    let drew = window.draw_if_needed(|renderer| {
        let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
        let stride = buffer.width() as usize;
        renderer.render(buffer.make_mut_slice(), stride);
        BUFFER.with(|b| *b.borrow_mut() = Some(buffer));
    });
    if !drew {
        return None;
    }
    BUFFER
        .with(|b| b.borrow_mut().take())
        .map(|buffer| {
            let img = to_rgba(&buffer);
            if small > 0 && img.width() > small {
                let gh = (small as u64 * img.height() as u64 / img.width() as u64) as u32;
                image::imageops::resize(&img, small, gh, FilterType::Lanczos3)
            } else {
                img
            }
        })
}

/// Encode the frames into an animated GIF that loops forever.
///
/// Consecutive identical frames are merged into the previous one's delay: a
/// motion holds still at both ends (the settle before it starts, the rest after
/// it lands), and a GIF writes every frame as its own full image, so those
/// stills cost a copy of the whole window each.
fn encode_gif(path: &str, frames: &[RgbaImage], frame_ms: u32) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = GifEncoder::new(file);
    enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
    let mut held: u32 = 0;
    let mut prev: Option<&RgbaImage> = None;
    for img in frames {
        if prev == Some(img) {
            held += frame_ms;
            continue;
        }
        // the frame being emitted carries every duplicate before it
        let delay = held + frame_ms;
        held = 0;
        enc.encode_frame(Frame::from_parts(img.clone(), 0, 0, Delay::from_numer_denom_ms(delay, 1)))
            .map_err(|e| e.to_string())?;
        prev = Some(img);
    }
    Ok(())
}

fn write_png(img: &RgbaImage, path: &str) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    img.save_with_format(path, ImageFormat::Png).map_err(|e| e.to_string())
}

/// `--motion`: capture the transit, not the state.
///
/// Slint eases an `animate` block against the platform clock and nothing else,
/// so the clock is the whole instrument: `CLOCK` steps it by `--frame-ms` and
/// `update_timers_and_animations()` — the pump an event loop would run at the
/// top of each iteration, and there is no event loop here — publishes it to
/// every binding that read the tick while it was evaluating. A sweep could never
/// show a motion because it never pumped.
fn capture_motion(
    ui: &AppWindow,
    state: &Rc<AppState>,
    motion: &str,
    scene: Option<&str>,
    w: u32,
    h: u32,
    frames: usize,
    frame_ms: u32,
    gif_w: u32,
    out_gif: Option<&str>,
    seq_dir: Option<&str>,
) -> Result<(), String> {
    let window = WINDOW
        .with(|slot| slot.borrow().clone())
        .ok_or("window adapter")?;
    // The scene has to be laid out before a property change can animate: an
    // element that has never been positioned eases from a zero, and a clip
    // that opens with a jump documents an animation the app never plays.
    if !render(w, h) {
        return Err("window never requested a redraw".into());
    }
    if let Some(scene) = scene {
        controller::apply_scene_overlay(ui, state, scene);
    }
    let mut shots: Vec<RgbaImage> = Vec::with_capacity(frames);
    for k in 0..frames {
        // one step of virtual time per frame, then the pump that publishes it:
        // `update_timers_and_animations` reads the clock through the platform,
        // so this is the whole of what makes the tween sample instead of skip
        CLOCK.with(|c| c.set(Some((k as u64 + 1) * frame_ms as u64)));
        controller::apply_motion(ui, state, motion, k, frames);
        slint::platform::update_timers_and_animations();
        match grab(&window, w, h, gif_w) {
            Some(img) => shots.push(img),
            None => return Err(format!("frame {k} drew nothing")),
        }
    }
    if shots.len() < frames {
        return Err("frame count short".into());
    }
    // A still sequence is the proof: an interpolated motion shows up as
    // successive frames differing, which a GIF alone cannot be checked for
    // without a viewer. `diffbbox.ps1` compares two of these.
    if let Some(dir) = seq_dir {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        for (k, img) in shots.iter().enumerate() {
            write_png(img, &format!("{dir}/f{k:03}.png"))?;
        }
    }
    let gif = out_gif.unwrap_or("quire-motion.gif");
    if let Some(parent) = std::path::Path::new(gif).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    encode_gif(gif, &shots, frame_ms)?;
    let bytes = std::fs::metadata(gif).map(|m| m.len()).unwrap_or(0);
    println!("wrote {gif} ({} frames, {frames}@{frame_ms}ms, {bytes} bytes)", shots.len());
    Ok(())
}

/// Print the open/closed state of every popup (the --click/--key probe).
fn probe(ui: &AppWindow, label: &str) {
    let g = ui.global::<UIState>();
    println!(
        "{label} menu-open={} slash-open={} slash-insert={} block-menu-id={} palette-open={} search-open={} capture-open={}",
        g.get_menu_open(),
        g.get_slash_open(),
        g.get_slash_insert(),
        g.get_block_menu_open_id(),
        g.get_palette_open(),
        g.get_search_open(),
        g.get_org_capture_open(),
    );
}

/// Deliver a synthetic click (press + release) at logical coordinates —
/// the same route a real mouse takes through the engine's event dispatch.
fn dispatch_click(window: &slint::Window, x: f32, y: f32) {
    let pos = LogicalPosition::new(x, y);
    window.dispatch_event(WindowEvent::PointerPressed { position: pos, button: PointerEventButton::Left });
    window.dispatch_event(WindowEvent::PointerReleased { position: pos, button: PointerEventButton::Left });
}

/// Park the pointer at logical coordinates without pressing anything: the
/// engine's enter events fire on the move, which is the only way a headless
/// shot can light up hover-only UI (a table's edge toolbar, a row's handle).
fn dispatch_hover(window: &slint::Window, x: f32, y: f32) {
    window.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(x, y) });
}

/// Parse an "x,y" argument value.
fn parse_xy(spec: &str) -> (f32, f32) {
    spec.split_once(',')
        .and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?)))
        .unwrap_or_else(|| panic!("expected x,y, got {spec}"))
}

/// Print the block projection as `id:kind` pairs (the --probe-blocks flag):
/// lets a scripted run assert what a popup action did to the document.
fn probe_blocks(ui: &AppWindow) {
    let g = ui.global::<UIState>();
    let kinds: Vec<String> = (0..g.get_blocks().row_count())
        .filter_map(|i| g.get_blocks().row_data(i))
        .map(|b| format!("{}:{}", b.id, b.kind))
        .collect();
    println!("blocks: {}", kinds.join(" "));
}

fn main() -> Result<(), String> {
    // Same rationale as the GUI binary: Slint's initial binding/layout
    // evaluation is deep; give it headroom (ADR-0009).
    let child = std::thread::Builder::new().stack_size(8 * 1024 * 1024)
        .spawn(|| run()).expect("spawn render thread");
    child.join().unwrap_or_else(|_| Err("render thread panicked".into()))
}

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().collect();
    let out = parse(&argv, "--out").unwrap_or_else(|| "quire-shot.bmp".into());
    let scene = parse(&argv, "--scene");
    let w: u32 = parse(&argv, "--w").and_then(|v| v.parse().ok()).unwrap_or(1280);
    let h: u32 = parse(&argv, "--h").and_then(|v| v.parse().ok()).unwrap_or(800);
    let motion = parse(&argv, "--motion");

    slint::platform::set_platform(Box::new(HeadlessPlatform))
        .map_err(|e| format!("set_platform: {e:?}"))?;
    // A motion capture owns the clock from here on, and it has to be armed
    // before the component is built: `AppWindow::new()` lays the tree out at the
    // current tick, and a tick that moved backwards to 0 later would trip
    // Slint's monotonic-clock assertion.
    if motion.is_some() {
        CLOCK.with(|c| c.set(Some(0)));
    }

    let ui = AppWindow::new().map_err(|e| e.to_string())?;
    ui.window().set_size(PhysicalSize::new(w, h));

    let args = HandleArgs { blocks: 0, auto_exit_secs: 0.0, bench_pages: 0, pictures: 0, marks: 0, code: 0 };
    // The backlink scenes read `marks` and `blocks.page_ref` out of a database,
    // because the panel *is* a query rather than a fixture — so those scenes
    // get a library and every other scene keeps the repo-less setup it has
    // always had. An empty in-memory library still falls back to the demo
    // session (`AppState::new` keeps the mock pages when nothing was
    // persisted), so the scenes around the panel are the same ones as before:
    // switching this on is meant to change the pixels of exactly the new
    // scenes, and a sweep with a baseline says whether it did.
    // SPEC §三十九: the database scenes join them for the same reason — the
    // table draws *records*, and records live in SQL (ADR-0067); a repo-less
    // session has none, so `database-table` would photograph an empty grid.
    let needs_db = scene
        .as_deref()
        .map(|s| s.contains("backlinks") || s.contains("database"))
        .unwrap_or(false);
    let repo = if needs_db {
        Some(std::sync::Arc::new(
            quire::storage::SqliteRepository::in_memory().map_err(|e| e.to_string())?,
        ))
    } else {
        None
    };
    let state = AppState::new(&args, repo);
    controller::bind(&ui, &state);
    controller::wire(&ui, &state);
    // `renderer_name()` is a compile-time guess about which renderer *feature*
    // was asked for, and this process did not ask: `HeadlessPlatform` above is
    // Slint's software rasterizer whatever else is compiled in — a
    // `--features software` build keeps the `femtovg` default on, which is why
    // every swept PNG used to caption itself "FemtoVG · GL" over a picture no
    // GPU drew. The sweep is evidence about a renderer, so it says which one ran.
    ui.global::<UIState>()
        .set_renderer_name("Software · headless".into());
    if let Some(scene) = &scene {
        controller::apply_scene(&ui, &state, scene);
    }
    // --theme <id>, **after** the scene: `apply_scene` owns the theme (dark-*
    // scenes are dark, the rest light), so an explicit ask has to land after it
    // to win — the one way to photograph a scene the catalog already seeds in a
    // palette the scene's own name does not spell.
    if let Some(id) = parse(&argv, "--theme") {
        ui.global::<UIState>().set_theme(id.into());
    }
    // --scroll-y drives the same viewport mirror scene F uses. It exists
    // because a scroll that moves nothing still burns CPU, and the bench
    // harness shipped with that bug: a run at 0 and a run at ±2000 have to
    // give different pixels before either number means anything.
    if let Some(spec) = parse(&argv, "--scroll-y") {
        let v: f32 = spec.parse().unwrap_or(0.0);
        ui.global::<UIState>().set_editor_scroll_y(v);
    }
    ui.show().map_err(|e| e.to_string())?;

    // --motion replaces the single-shot path: it owns its own first render,
    // because a frame sequence needs the window laid out *before* the property
    // that animates is written.
    if let Some(motion) = motion {
        let frames: usize = parse(&argv, "--frames").and_then(|v| v.parse().ok()).unwrap_or(20);
        let frame_ms: u32 = parse(&argv, "--frame-ms").and_then(|v| v.parse().ok()).unwrap_or(40);
        let gif_w: u32 = parse(&argv, "--gif-w").and_then(|v| v.parse().ok()).unwrap_or(560);
        let gif = parse(&argv, "--gif");
        let seq = parse(&argv, "--seq-dir");
        return capture_motion(
            &ui,
            &state,
            &motion,
            scene.as_deref(),
            w,
            h,
            frames,
            frame_ms,
            gif_w,
            gif.as_deref(),
            seq.as_deref(),
        );
    }

    let drawn = render(w, h);
    if !drawn {
        return Err("window never requested a redraw".into());
    }


    if let Some(scene) = &scene {
        controller::apply_scene_overlay(&ui, &state, scene);
    }

    // --click/--key: drive a real input event, then re-render so the BMP
    // shows the post-input state; the probe lines document the popup flags
    // before and after the input (the "before" one proves the scene really
    // opened the popup the "after" one claims to have dismissed).
    let mut interacted = false;
    if let Some(spec) = parse(&argv, "--hover") {
        let (x, y) = parse_xy(&spec);
        probe(&ui, "before-input:");
        dispatch_hover(ui.window(), x, y);
        interacted = true;
    }
    if let Some(spec) = parse(&argv, "--click") {
        let (x, y) = parse_xy(&spec);
        probe(&ui, "before-input:");
        dispatch_click(ui.window(), x, y);
        interacted = true;
    }
    if let Some(key) = parse(&argv, "--key") {
        let text: SharedString = match key.as_str() {
            "escape" => slint::platform::Key::Escape.into(),
            "return" => slint::platform::Key::Return.into(),
            other => other.into(),
        };
        probe(&ui, "before-input:");
        ui.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        ui.window().dispatch_event(WindowEvent::KeyReleased { text });
        interacted = true;
    }
    if interacted {
        probe(&ui, "after-input: ");
    }
    if argv.iter().any(|a| a == "--probe-blocks") {
        probe_blocks(&ui);
    }
    BUFFER.with(|b| *b.borrow_mut() = None);
    WINDOW.with(|slot| {
        if let Some(window) = slot.borrow().clone() {
            // the overlay flags mark popup state, not the window's redraw
            // flag — request the second pass explicitly
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
                let stride = buffer.width() as usize;
                renderer.render(buffer.make_mut_slice(), stride);
                BUFFER.with(|b| *b.borrow_mut() = Some(buffer));
            });
        }
    });

    let buffer = BUFFER.with(|b| b.borrow_mut().take());
    match buffer {
        Some(buffer) => {
            if let Some(parent) = std::path::Path::new(&out).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
            }
            write_bmp(&out, &buffer)?;
            println!("wrote {out}");
            Ok(())
        }
        None => Err("render produced no buffer".into()),
    }
}

thread_local! {
    static BUFFER: RefCell<Option<SharedPixelBuffer<Rgb8Pixel>>> = const { RefCell::new(None) };
}
