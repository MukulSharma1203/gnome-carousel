use gtk4::cairo;
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, DrawingArea, EventControllerKey, EventControllerScroll,
    EventControllerScrollFlags, GestureClick,
};
use image::imageops::FilterType;
use rayon::prelude::*;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::UNIX_EPOCH;

struct DecodedCard {
    idx: usize,
    width: i32,
    height: i32,
    argb: Vec<u8>,
}

struct WallpaperItem {
    path: PathBuf,
    card_surf: Option<cairo::ImageSurface>,
}

struct AppState {
    items: Vec<WallpaperItem>,
    selected: usize,
    scroll_pos: f64,
    card_centers: Vec<(usize, f64, f64, f64)>,
}

fn cache_path_for(img_path: &Path, cache_dir: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    img_path.hash(&mut hasher);
    if let Ok(meta) = fs::metadata(img_path) {
        if let Ok(mtime) = meta.modified() {
            if let Ok(dur) = mtime.duration_since(UNIX_EPOCH) {
                dur.as_secs().hash(&mut hasher);
            }
        }
    }
    cache_dir.join(format!("v2_{:016x}.jpg", hasher.finish()))
}

fn decode_card_argb(path: &Path, cache_dir: &Path) -> Option<(i32, i32, Vec<u8>)> {
    let cached = cache_path_for(path, cache_dir);

    let rgba = if cached.exists() {
        image::open(&cached).ok()?.to_rgba8()
    } else {
        let img = image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()?;
        let resized = img.resize_to_fill(640, 360, FilterType::Triangle);
        let rgb = resized.to_rgb8();
        let _ = rgb.save(&cached);
        resized.to_rgba8()
    };

    let width = rgba.width() as i32;
    let height = rgba.height() as i32;
    let raw = rgba.into_raw();
    let mut argb = vec![0u8; raw.len()];

    for (src, dst) in raw.chunks_exact(4).zip(argb.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    Some((width, height, argb))
}

fn argb_to_surface(width: i32, height: i32, argb: Vec<u8>) -> Option<cairo::ImageSurface> {
    let stride = cairo::Format::ARgb32.stride_for_width(width as u32).ok()?;
    cairo::ImageSurface::create_for_data(argb, cairo::Format::ARgb32, width, height, stride).ok()
}

fn apply_gnome_wallpaper(path: &Path) {
    if let Some(abs) = fs::canonicalize(path)
        .ok()
        .and_then(|p| p.to_str().map(String::from))
    {
        let uri = format!("file://{}", abs);
        let _ = Command::new("gsettings")
            .args(["set", "org.gnome.desktop.background", "picture-uri", &uri])
            .spawn();
        let _ = Command::new("gsettings")
            .args(["set", "org.gnome.desktop.background", "picture-uri-dark", &uri])
            .spawn();
    }
}

fn trace_parallelogram(cr: &cairo::Context, cx: f64, cy: f64, w: f64, h: f64, slant: f64) {
    let hw = w / 2.0;
    let hh = h / 2.0;
    let hs = slant / 2.0;
    cr.new_path();
    cr.move_to(cx - hw + hs, cy - hh);
    cr.line_to(cx + hw + hs, cy - hh);
    cr.line_to(cx + hw - hs, cy + hh);
    cr.line_to(cx - hw - hs, cy + hh);
    cr.close_path();
}

fn wrapped_offset(idx: usize, scroll_pos: f64, len: usize) -> f64 {
    if len == 0 {
        return 0.0;
    }
    let n = len as f64;
    let mut diff = (idx as f64 - scroll_pos) % n;
    if diff > n / 2.0 {
        diff -= n;
    } else if diff < -n / 2.0 {
        diff += n;
    }
    diff
}

fn scan_wallpapers(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|ext| matches!(ext.to_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    paths
}

fn sync_new_wallpapers(
    state: &Rc<RefCell<AppState>>,
    dir: &Path,
    cache_dir: &Path,
    tx: &mpsc::Sender<DecodedCard>,
) {
    let paths = scan_wallpapers(dir);
    let mut st = state.borrow_mut();

    let needs_reload = st.items.len() != paths.len()
        || st
            .items
            .iter()
            .zip(paths.iter())
            .any(|(item, p)| &item.path != p);

    if !needs_reload {
        return;
    }

    let old_selected_path = st.items.get(st.selected).map(|it| it.path.clone());
    st.items = paths
        .iter()
        .map(|p| WallpaperItem {
            path: p.clone(),
            card_surf: None,
        })
        .collect();

    if let Some(prev) = old_selected_path {
        if let Some(pos) = st.items.iter().position(|it| it.path == prev) {
            st.selected = pos;
            st.scroll_pos = pos as f64;
        } else {
            st.selected = 0;
            st.scroll_pos = 0.0;
        }
    }

    let sender = tx.clone();
    let cache = cache_dir.to_path_buf();
    let tasks: Vec<(usize, PathBuf)> = paths.into_iter().enumerate().collect();
    thread::spawn(move || {
        tasks.into_par_iter().for_each_with(sender, |s, (idx, path)| {
            if let Some((width, height, argb)) = decode_card_argb(&path, &cache) {
                let _ = s.send(DecodedCard {
                    idx,
                    width,
                    height,
                    argb,
                });
            }
        });
    });
}

fn main() {
    // Single-instance D-Bus app: second invocation wakes the already-running daemon instantly
    let app = Application::builder()
        .application_id("io.github.gnome_carousel")
        .build();

    let is_daemon = env::args().any(|a| a == "--daemon");
    let built = Rc::new(RefCell::new(false));
    let win_ref: Rc<RefCell<Option<ApplicationWindow>>> = Rc::new(RefCell::new(None));
    let state_ref: Rc<RefCell<Option<Rc<RefCell<AppState>>>>> = Rc::new(RefCell::new(None));
    let tx_ref: Rc<RefCell<Option<mpsc::Sender<DecodedCard>>>> = Rc::new(RefCell::new(None));

    let built_act = built.clone();
    let win_act = win_ref.clone();
    let state_act = state_ref.clone();
    let tx_act = tx_ref.clone();

    app.connect_activate(move |app| {
        let home = env::var("HOME").unwrap_or_else(|_| ".".into());
        let dir = PathBuf::from(&home).join("Pictures/Wallpapers");
        let cache_dir = PathBuf::from(&home).join(".cache/gnome-carousel");
        let _ = fs::create_dir_all(&cache_dir);

        // If daemon is already running, toggle visibility immediately (<1ms)
        if *built_act.borrow() {
            if let Some(win) = win_act.borrow().as_ref() {
                if win.is_visible() {
                    win.set_visible(false);
                } else {
                    if let (Some(st), Some(tx)) =
                        (state_act.borrow().as_ref(), tx_act.borrow().as_ref())
                    {
                        sync_new_wallpapers(st, &dir, &cache_dir, tx);
                    }
                    win.set_visible(true);
                    win.present();
                }
            }
            return;
        }

        let paths = scan_wallpapers(&dir);
        if paths.is_empty() {
            return;
        }

        // Keep app alive in background even when the window is hidden
        let _hold = app.hold();
        Box::leak(Box::new(_hold));

        let len = paths.len();
        let mut items: Vec<WallpaperItem> = paths
            .iter()
            .map(|p| WallpaperItem {
                path: p.clone(),
                card_surf: None,
            })
            .collect();

        let (tx, rx) = mpsc::channel::<DecodedCard>();
        let bg_tasks: Vec<(usize, PathBuf)> = paths.into_iter().enumerate().collect();
        let bg_cache = cache_dir.clone();
        let bg_tx = tx.clone();

        thread::spawn(move || {
            bg_tasks.into_par_iter().for_each_with(bg_tx, |sender, (idx, path)| {
                if let Some((width, height, argb)) = decode_card_argb(&path, &bg_cache) {
                    let _ = sender.send(DecodedCard {
                        idx,
                        width,
                        height,
                        argb,
                    });
                }
            });
        });

        // If started in foreground (not --daemon), decode first 7 synchronously for immediate display
        if !is_daemon {
            let mut initial_indices = Vec::new();
            for delta in [-3isize, -2, -1, 0, 1, 2, 3] {
                let idx = ((delta % len as isize) + len as isize) as usize % len;
                initial_indices.push(idx);
            }
            initial_indices.sort_unstable();
            initial_indices.dedup();
            for idx in initial_indices {
                if let Some((w, h, argb)) = decode_card_argb(&items[idx].path, &cache_dir) {
                    items[idx].card_surf = argb_to_surface(w, h, argb);
                }
            }
        }

        let state = Rc::new(RefCell::new(AppState {
            items,
            selected: 0,
            scroll_pos: 0.0,
            card_centers: Vec::new(),
        }));

        let css = gtk4::CssProvider::new();
        css.load_from_string(
            "window, window.background, window.csd, drawingarea { background: transparent; background-color: rgba(0,0,0,0); border: 0; border-width: 0; box-shadow: none; outline: 0; }",
        );
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &css,
                gtk4::STYLE_PROVIDER_PRIORITY_USER,
            );
        }

        let monitor_w = gdk::Display::default()
            .and_then(|d| d.monitors().item(0))
            .and_then(|m| m.downcast::<gdk::Monitor>().ok())
            .map(|m| m.geometry().width())
            .unwrap_or(1920);

        let win_w = (monitor_w - 80).max(800);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("gnome-carousel")
            .decorated(false)
            .resizable(false)
            .hide_on_close(true)
            .default_width(win_w)
            .default_height(460)
            .build();

        let area = DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .can_focus(false)
            .build();

        let state_tick = state.clone();
        area.add_tick_callback(move |widget, _| {
            let mut st = state_tick.borrow_mut();
            let mut needs_redraw = false;

            while let Ok(card) = rx.try_recv() {
                if let Some(surf) = argb_to_surface(card.width, card.height, card.argb) {
                    if let Some(item) = st.items.get_mut(card.idx) {
                        item.card_surf = Some(surf);
                        needs_redraw = true;
                    }
                }
            }

            let len = st.items.len();
            let diff = wrapped_offset(st.selected, st.scroll_pos, len);
            if diff.abs() > 0.0005 {
                st.scroll_pos += diff * 0.24;
                needs_redraw = true;
            } else if diff.abs() > 0.0 {
                st.scroll_pos = st.selected as f64;
                needs_redraw = true;
            }

            if needs_redraw {
                widget.queue_draw();
            }
            glib::ControlFlow::Continue
        });

        let state_draw = state.clone();
        area.set_draw_func(move |_widget, cr, width, height| {
            cr.set_operator(cairo::Operator::Clear);
            let _ = cr.paint();
            cr.set_operator(cairo::Operator::Over);

            let mut st = state_draw.borrow_mut();
            let w = width as f64;
            let h = height as f64;

            let cx = w / 2.0;
            let cy = h / 2.0;
            let card_h = h * 0.84;
            let center_w = card_h * 1.72;
            let slice_w = card_h * 0.22;
            let gap = card_h * 0.042;
            let slant = card_h * 0.35;

            let card_width_at = |dist: f64| -> f64 {
                let t = dist.min(1.0);
                let ease = t * t * (3.0 - 2.0 * t);
                center_w * (1.0 - ease) + slice_w * ease
            };

            let card_x_at = |offset: f64| -> f64 {
                let sign = offset.signum();
                let d = offset.abs();
                if d <= 1.0 {
                    let ease = d * d * (3.0 - 2.0 * d);
                    let w_curr = center_w * (1.0 - ease) + slice_w * ease;
                    sign * ((center_w + w_curr) * 0.5 + gap) * d
                } else {
                    let first_step = (center_w + slice_w) * 0.5 + gap;
                    let extra_steps = (d - 1.0) * (slice_w + gap);
                    sign * (first_step + extra_steps)
                }
            };

            let len = st.items.len();
            let scroll_pos = st.scroll_pos;

            let mut visible: Vec<(usize, f64)> = (0..len)
                .map(|idx| (idx, wrapped_offset(idx, scroll_pos, len)))
                .filter(|&(_, off)| off.abs() <= 2.15)
                .collect();

            visible.sort_by(|a, b| {
                b.1.abs()
                    .partial_cmp(&a.1.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            st.card_centers.clear();

            for (idx, offset) in visible {
                let dist = offset.abs();
                let cw = card_width_at(dist);
                let x = cx + card_x_at(offset);
                let alpha = if dist <= 1.0 {
                    1.0 - dist * 0.08
                } else {
                    (0.92 - (dist - 1.0) * 0.78).max(0.0)
                };

                if alpha <= 0.03 {
                    continue;
                }

                let Some(surf) = st.items[idx].card_surf.clone() else {
                    continue;
                };

                st.card_centers.push((idx, x, cw, card_h));

                let _ = cr.save();

                cr.set_source_rgba(0.0, 0.0, 0.0, 0.35 * alpha);
                trace_parallelogram(cr, x + 3.0, cy + 8.0, cw, card_h, slant);
                let _ = cr.fill();

                trace_parallelogram(cr, x, cy, cw, card_h, slant);
                cr.clip();

                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                let img_scale = ((center_w + slant) / sw).max(card_h / sh);

                cr.translate(x, cy);
                cr.scale(img_scale, img_scale);
                let _ = cr.set_source_surface(&surf, -sw / 2.0, -sh / 2.0);
                let _ = cr.paint_with_alpha(alpha);
                cr.reset_clip();

                let _ = cr.restore();
            }
        });

        let key_ctrl = EventControllerKey::new();
        let state_key = state.clone();
        let win_key = window.clone();
        let area_key = area.clone();
        key_ctrl.connect_key_pressed(move |_, key, _, _| {
            let mut st = state_key.borrow_mut();
            let len = st.items.len();
            match key {
                gdk::Key::Right | gdk::Key::l | gdk::Key::d => {
                    st.selected = (st.selected + 1) % len;
                    area_key.queue_draw();
                }
                gdk::Key::Left | gdk::Key::h | gdk::Key::a => {
                    st.selected = if st.selected == 0 { len - 1 } else { st.selected - 1 };
                    area_key.queue_draw();
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    apply_gnome_wallpaper(&st.items[st.selected].path);
                    win_key.set_visible(false);
                }
                gdk::Key::Escape | gdk::Key::q => {
                    win_key.set_visible(false);
                }
                _ => {}
            }
            glib::Propagation::Stop
        });
        window.add_controller(key_ctrl);

        let scroll_ctrl = EventControllerScroll::new(EventControllerScrollFlags::BOTH_AXES);
        let state_scroll = state.clone();
        let area_scroll = area.clone();
        scroll_ctrl.connect_scroll(move |_, dx, dy| {
            let delta = if dx.abs() > dy.abs() { dx } else { dy };
            if delta.abs() > 0.15 {
                let mut st = state_scroll.borrow_mut();
                let len = st.items.len();
                st.selected = if delta > 0.0 {
                    (st.selected + 1) % len
                } else if st.selected == 0 {
                    len - 1
                } else {
                    st.selected - 1
                };
                area_scroll.queue_draw();
            }
            glib::Propagation::Stop
        });
        area.add_controller(scroll_ctrl);

        let click_ctrl = GestureClick::new();
        let state_click = state.clone();
        let win_click = window.clone();
        let area_click = area.clone();
        click_ctrl.connect_released(move |_, _, x, y| {
            let mut st = state_click.borrow_mut();
            let cy = area_click.height() as f64 / 2.0;
            let hit = st
                .card_centers
                .iter()
                .rev()
                .find(|&&(_, cx, cw, ch)| {
                    (x - cx).abs() <= cw / 2.0 && (y - cy).abs() <= ch / 2.0
                })
                .map(|&(idx, _, _, _)| idx);

            if let Some(idx) = hit {
                if idx == st.selected {
                    apply_gnome_wallpaper(&st.items[idx].path);
                    win_click.set_visible(false);
                } else {
                    st.selected = idx;
                    area_click.queue_draw();
                }
            }
        });
        area.add_controller(click_ctrl);

        window.set_child(Some(&area));
        *win_act.borrow_mut() = Some(window.clone());
        *state_act.borrow_mut() = Some(state);
        *tx_act.borrow_mut() = Some(tx);
        *built_act.borrow_mut() = true;

        if !is_daemon {
            window.present();
        }
    });

    app.run_with_args::<&str>(&[]);
}