//! Promptagotchi: a pixel desk pet that keeps an eye on every Claude Code session.
//!
//! The Rust core owns everything: sessions, the hook server, the windows (pet, session list,
//! whip, and the app window with its Run / Editor / Settings / Debug tabs), menus, shortcuts and
//! placement. The screens themselves are small HTML pages in src/renderer, drawn by the
//! system's WebKit, talking to this side through `ipc` (see src/renderer/bridge.js).
pub mod config;
pub mod describe;
pub mod focus;
pub mod geometry;
pub mod hooks;
pub mod paths;
pub mod pets;
pub mod server;
pub mod session_files;
pub mod sessions;
pub mod simulator;
pub mod transcripts;
pub mod util;

use config::{Config, Place};
use geometry::{clamp_rect, feet_from_place, feet_of, panel_rect, pet_metrics, place_from_feet, rect_for_feet, Display, Metrics, PanelPlace, PetShape, Rect};
use serde_json::{json, Value};
use server::Store;
use sessions::{SessionStore, StoreEvent};
use simulator::Simulator;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use util::now_ms;

tauri_panel! {
    panel!(OverlayPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true
        }
    })
}

const PET: &str = "pet";
const PANEL: &str = "panel";
const WHIP: &str = "whip";
const APP: &str = "app";

struct Geo {
    display: Display,
    m: Metrics,
    pet: Rect,
    panel: PanelPlace,
}

#[derive(Default)]
struct Ui {
    hover_pet: bool,
    hover_panel: bool,
    pinned: bool,
    open: bool,
    close_at: Option<Instant>,
    /// window x, y and pointer x, y when the drag started; when it last moved
    drag: Option<(f64, f64, f64, f64, Instant)>,
}

struct Whip {
    display: Display,
}

pub struct App {
    store: Store,
    config: Arc<Mutex<Config>>,
    hidden: Arc<AtomicBool>,
    ui: Mutex<Ui>,
    geo: Mutex<Option<Geo>>,
    pet: Mutex<Value>,
    hot: Mutex<HashMap<String, Vec<[f64; 4]>>>,
    ignoring: Mutex<HashMap<String, bool>>,
    events: Mutex<VecDeque<Value>>,
    pending: Mutex<Vec<Value>>,
    event_count: AtomicU64,
    server: Mutex<Value>,
    sim: Mutex<Option<Simulator>>,
    whip: Mutex<Option<Whip>>,
    hustle_until: AtomicI64,
    shortcut_ok: Mutex<HashMap<String, bool>>,
    os: String,
}

/// PROMPTAGOTCHI_DEBUG=1 prints what the windows and the core say to each other.
fn debug(msg: impl AsRef<str>) {
    if std::env::var_os("PROMPTAGOTCHI_DEBUG").is_some() {
        eprintln!("[promptagotchi] {}", msg.as_ref());
    }
}

fn st(app: &AppHandle) -> Arc<App> {
    app.state::<Arc<App>>().inner().clone()
}

fn cfg(app: &AppHandle) -> Config {
    st(app).config.lock().unwrap().clone()
}

fn set_cfg(app: &AppHandle, f: impl FnOnce(&mut Config)) {
    let a = st(app);
    let mut c = a.config.lock().unwrap();
    f(&mut c);
    c.save(&paths::get().config_file());
    drop(c);
    push_app_state(app);
}

fn emit(app: &AppHandle, label: &str, event: &str, payload: Value) {
    let _ = app.emit_to(label, event, payload);
}

// ------------------------------------------------------------------ screens
fn displays(app: &AppHandle) -> Vec<Display> {
    let mons = app.available_monitors().unwrap_or_default();
    mons.iter()
        .enumerate()
        .map(|(i, m)| {
            let s = m.scale_factor();
            let (p, z, wa) = (m.position(), m.size(), m.work_area());
            Display {
                id: m.name().cloned().unwrap_or_else(|| format!("display-{i}")),
                bounds: Rect { x: p.x as f64 / s, y: p.y as f64 / s, width: z.width as f64 / s, height: z.height as f64 / s },
                work_area: Rect { x: wa.position.x as f64 / s, y: wa.position.y as f64 / s, width: wa.size.width as f64 / s, height: wa.size.height as f64 / s },
            }
        })
        .collect()
}

fn primary_display(app: &AppHandle) -> Display {
    let all = displays(app);
    let primary = app.primary_monitor().ok().flatten().and_then(|m| m.name().cloned());
    all.iter().find(|d| Some(&d.id) == primary.as_ref()).or(all.first()).cloned().unwrap_or(Display {
        id: "main".into(),
        bounds: Rect { x: 0.0, y: 0.0, width: 1440.0, height: 900.0 },
        work_area: Rect { x: 0.0, y: 25.0, width: 1440.0, height: 875.0 },
    })
}

fn display_at(app: &AppHandle, x: f64, y: f64) -> Display {
    let all = displays(app);
    let inside = |d: &Display| x >= d.bounds.x && x < d.bounds.x + d.bounds.width && y >= d.bounds.y && y < d.bounds.y + d.bounds.height;
    if let Some(d) = all.iter().find(|d| inside(d)) {
        return d.clone();
    }
    let dist = |d: &Display| {
        let cx = d.bounds.x + d.bounds.width / 2.0;
        let cy = d.bounds.y + d.bounds.height / 2.0;
        (cx - x).powi(2) + (cy - y).powi(2)
    };
    all.into_iter().min_by(|a, b| dist(a).total_cmp(&dist(b))).unwrap_or_else(|| primary_display(app))
}

/// The cursor in logical screen points.
fn cursor(app: &AppHandle) -> Option<(f64, f64)> {
    let p = app.cursor_position().ok()?;
    for m in app.available_monitors().ok()? {
        let (pos, size, s) = (m.position(), m.size(), m.scale_factor());
        if p.x >= pos.x as f64 && p.x < pos.x as f64 + size.width as f64 && p.y >= pos.y as f64 && p.y < pos.y as f64 + size.height as f64 {
            return Some((p.x / s, p.y / s));
        }
    }
    let s = app.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(2.0);
    Some((p.x / s, p.y / s))
}

// ---------------------------------------------------------------- placement
fn set_bounds(app: &AppHandle, label: &str, r: &Rect) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.set_size(LogicalSize::new(r.width, r.height));
        let _ = w.set_position(LogicalPosition::new(r.x, r.y));
    }
}

fn metrics_json(m: &Metrics) -> Value {
    json!({ "petH": m.pet_h, "petW": m.pet_w, "zoom": m.zoom, "pad": m.pad, "clipBottom": m.clip_bottom })
}

/// Put the pet so its feet are on `feet`, sized for the screen it's on.
fn place_at(app: &AppHandle, feet: (f64, f64), display: Option<Display>) {
    let a = st(app);
    let d = display.unwrap_or_else(|| display_at(app, feet.0, feet.1));
    let shape = PetShape::from_pet(&a.pet.lock().unwrap());
    let m = pet_metrics(&d.work_area, &shape, &cfg(app).size);
    let r = clamp_rect(rect_for_feet(feet, &m), &d);
    let p = panel_rect(&r, &d, m.zoom);
    debug(format!("place: screen {} work {:?} size {} → pet {}×{} (×{}), window {:?}", d.id, d.work_area, cfg(app).size, m.pet_w, m.pet_h, m.scale, r));
    set_bounds(app, PET, &r);
    set_bounds(app, PANEL, &p.rect);
    emit(app, PET, "metrics", metrics_json(&m));
    emit(app, PANEL, "layout", p.layout(m.zoom));
    *a.geo.lock().unwrap() = Some(Geo { display: d, m, pet: r, panel: p });
}

fn place_from_config(app: &AppHandle) {
    let c = cfg(app);
    let d = c.place.as_ref().and_then(|p| p.display.as_ref()).and_then(|id| displays(app).into_iter().find(|d| &d.id == id)).unwrap_or_else(|| primary_display(app));
    let feet = feet_from_place(c.place.as_ref().map(|p| p.fx), c.place.as_ref().map(|p| p.fy), &d);
    place_at(app, feet, Some(d));
}

fn remember_place(app: &AppHandle) {
    let a = st(app);
    let geo = a.geo.lock().unwrap();
    let Some(g) = geo.as_ref() else { return };
    let (fx, fy) = place_from_feet(feet_of(&g.pet, &g.m), &g.display);
    let id = g.display.id.clone();
    drop(geo);
    set_cfg(app, |c| c.place = Some(Place { display: Some(id), fx, fy }));
}

fn replace_keeping_feet(app: &AppHandle) {
    let a = st(app);
    let current = a.geo.lock().unwrap().as_ref().map(|g| (feet_of(&g.pet, &g.m), g.display.clone()));
    match current {
        Some((feet, d)) => place_at(app, feet, Some(d)),
        None => place_from_config(app),
    }
}

// ---------------------------------------------------------- session list
fn set_open(app: &AppHandle, open: bool) {
    let a = st(app);
    let mut ui = a.ui.lock().unwrap();
    ui.close_at = None;
    if ui.open == open {
        return;
    }
    ui.open = open;
    if !open {
        ui.pinned = false;
    }
    let payload = json!({ "open": open, "pinned": ui.pinned });
    drop(ui);
    emit(app, PET, "panel-state", payload.clone());
    emit(app, PANEL, "panel-state", payload);
}

fn sync_open(app: &AppHandle) {
    let a = st(app);
    let mut ui = a.ui.lock().unwrap();
    if ui.drag.is_some() {
        return;
    }
    if ui.hover_pet && !ui.open {
        drop(ui);
        return set_open(app, true);
    }
    if ui.hover_pet || ui.hover_panel || ui.pinned {
        ui.close_at = None;
    } else if ui.open && ui.close_at.is_none() {
        ui.close_at = Some(Instant::now() + Duration::from_millis(380));
    }
}

fn on_pet_click(app: &AppHandle) {
    let snap = st(app).store.lock().unwrap().snapshot();
    let urgent = snap.iter().find(|s| s["state"] == "question").or_else(|| snap.iter().find(|s| s["state"] == "done"));
    if let Some(u) = urgent {
        let key = u["key"].as_str().unwrap_or("").to_string();
        let app = app.clone();
        std::thread::spawn(move || { open_session(&app, &key); });
        return;
    }
    let a = st(app);
    let (open, pinned) = { let u = a.ui.lock().unwrap(); (u.open, u.pinned) };
    if open && pinned {
        return set_open(app, false);
    }
    a.ui.lock().unwrap().pinned = true;
    if open {
        emit(app, PET, "panel-state", json!({ "open": true, "pinned": true }));
        emit(app, PANEL, "panel-state", json!({ "open": true, "pinned": true }));
    } else {
        set_open(app, true);
    }
}

/// Jump to a session. { ok, reason } so the list can say why nothing happened.
fn open_session(app: &AppHandle, key: &str) -> Value {
    let a = st(app);
    let view = {
        let mut s = a.store.lock().unwrap();
        let v = s.snapshot().into_iter().find(|v| v["key"] == key);
        if v.is_some() {
            s.release_session(key);
            s.mark_seen(key);
        }
        v
    };
    let Some(v) = view else { return json!({ "ok": false, "reason": "gone" }) };
    if v["host"]["termProgram"] == "Simulator" {
        return json!({ "ok": false, "reason": "pretend" });
    }
    if v["state"] == "ended" {
        return json!({ "ok": false, "reason": "closed" });
    }
    let ok = focus::focus_session(&v);
    json!({ "ok": ok, "reason": if ok { Value::Null } else { json!("unknown") } })
}

// --------------------------------------------------------------- the pet
fn load_current_pet(app: &AppHandle) {
    let want = cfg(app).pet;
    let pet = pets::load_pet(&want).or_else(|_| pets::load_pet("builtin:pip")).unwrap_or(Value::Null);
    *st(app).pet.lock().unwrap() = pet;
}

fn set_pet(app: &AppHandle, key: &str) {
    set_cfg(app, |c| c.pet = key.to_string());
    load_current_pet(app);
    replace_keeping_feet(app);
    send_init(app, PET);
    send_init(app, PANEL);
    rebuild_menus(app);
}

fn set_size(app: &AppHandle, size: &str) {
    if !geometry::SIZES.iter().any(|(k, _)| *k == size) {
        return;
    }
    set_cfg(app, |c| c.size = size.to_string());
    replace_keeping_feet(app);
    remember_place(app);
    rebuild_menus(app);
}

fn show_pet(app: &AppHandle) {
    st(app).hidden.store(false, Ordering::Relaxed);
    if st(app).ui.lock().unwrap().drag.is_none() {
        place_from_config(app); // always come back fully on screen
    }
    for label in [PANEL, PET] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.show();
        }
    }
    rebuild_menus(app);
    push_app_state(app);
}

fn toggle_pet(app: &AppHandle) {
    let a = st(app);
    if a.hidden.load(Ordering::Relaxed) {
        return show_pet(app);
    }
    a.hidden.store(true, Ordering::Relaxed);
    set_open(app, false);
    for label in [PET, PANEL] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.hide();
        }
    }
    rebuild_menus(app);
    push_app_state(app);
}

fn send_init(app: &AppHandle, label: &str) {
    let a = st(app);
    let base = json!({
        "config": cfg(app),
        "hooks": hooks::status(&paths::get().claude_settings()),
        "sessions": a.store.lock().unwrap().snapshot(),
        "demo": false,
    });
    if label == PET {
        let mut p = base.clone();
        p["pet"] = a.pet.lock().unwrap().clone();
        emit(app, PET, "init", p);
    } else if label == PANEL {
        emit(app, PANEL, "init", base);
    }
    if let Some(g) = a.geo.lock().unwrap().as_ref() {
        emit(app, PET, "metrics", metrics_json(&g.m));
        emit(app, PANEL, "layout", g.panel.layout(g.m.zoom));
    }
    let (open, pinned) = { let u = a.ui.lock().unwrap(); (u.open, u.pinned) };
    emit(app, label, "panel-state", json!({ "open": open, "pinned": pinned }));
    emit(app, label, "whip-mode", json!({ "on": a.whip.lock().unwrap().is_some() }));
    let until = a.hustle_until.load(Ordering::Relaxed);
    if until > now_ms() {
        emit(app, label, "hustle", json!({ "until": until }));
    }
}

// ------------------------------------------------------------- the whip
fn set_whip(app: &AppHandle, on: bool) {
    let a = st(app);
    let active = a.whip.lock().unwrap().is_some();
    if on == active {
        return;
    }
    if on {
        let (cx, cy) = cursor(app).unwrap_or((0.0, 0.0));
        let d = display_at(app, cx, cy);
        if app.get_webview_window(WHIP).is_none() {
            let _ = overlay_panel(app, WHIP, "whip.html", PanelLevel::Custom(1002));
        }
        set_bounds(app, WHIP, &d.bounds);
        if let Some(w) = app.get_webview_window(WHIP) {
            let _ = w.set_ignore_cursor_events(true);
            let _ = w.show();
        }
        *a.whip.lock().unwrap() = Some(Whip { display: d });
    } else {
        *a.whip.lock().unwrap() = None;
        if let Some(w) = app.get_webview_window(WHIP) {
            let _ = w.hide();
        }
    }
    emit(app, PET, "whip-mode", json!({ "on": on }));
    emit(app, PANEL, "whip-mode", json!({ "on": on }));
    rebuild_menus(app);
    push_app_state(app);
}

fn whip_tick(app: &AppHandle) {
    let a = st(app);
    let Some((cx, cy)) = cursor(app) else { return };
    let mut whip = a.whip.lock().unwrap();
    let Some(w) = whip.as_mut() else { return };
    let d = display_at(app, cx, cy);
    if d.id != w.display.id {
        set_bounds(app, WHIP, &d.bounds);
        emit(app, WHIP, "whip:reset", json!({}));
        w.display = d;
    }
    let o = w.display.bounds;
    drop(whip);
    let pet = a.geo.lock().unwrap().as_ref().filter(|g| g.display.id == display_at(app, cx, cy).id && !a.hidden.load(Ordering::Relaxed)).map(|g| json!({ "x": g.pet.x - o.x, "y": g.pet.y - o.y, "w": g.pet.width, "h": g.pet.height }));
    emit(app, WHIP, "whip:frame", json!({ "x": cx - o.x, "y": cy - o.y, "sound": cfg(app).whip_sound, "pet": pet }));
}

/// Cracked across the pet: it flinches and hustles (each hit adds time, up to a minute).
/// Hustle mode is only the pet's animation; it never touches Claude or your code.
fn on_whip_hit(app: &AppHandle, h: &Value) {
    let a = st(app);
    let now = now_ms();
    let until = (a.hustle_until.load(Ordering::Relaxed).max(now) + 12_000).min(now + 60_000);
    a.hustle_until.store(until, Ordering::Relaxed);
    emit(app, PET, "pet:hit", json!({ "speed": h["speed"] }));
    emit(app, PET, "hustle", json!({ "until": until }));
    emit(app, PANEL, "hustle", json!({ "until": until }));
    push_app_state(app);
}

fn register_shortcuts(app: &AppHandle) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let c = cfg(app);
    let mut ok = HashMap::new();
    for (key, accel) in [("shortcut", c.shortcut.clone()), ("whipShortcut", c.whip_shortcut.clone())] {
        let k = key.to_string();
        let res = gs.on_shortcut(accel.as_str(), move |app, _sc, ev| {
            if ev.state == ShortcutState::Pressed {
                if k == "shortcut" {
                    toggle_pet(app);
                } else {
                    let on = st(app).whip.lock().unwrap().is_none();
                    set_whip(app, on);
                }
            }
        });
        ok.insert(key.to_string(), res.is_ok());
    }
    *st(app).shortcut_ok.lock().unwrap() = ok;
}

fn set_shortcut(app: &AppHandle, x: &Value) -> Value {
    let name = x["name"].as_str().unwrap_or("");
    let accel = x["accel"].as_str().unwrap_or("").to_string();
    if !["shortcut", "whipShortcut"].contains(&name) || accel.is_empty() {
        return json!({ "ok": false, "error": "That isn’t a shortcut." });
    }
    let c = cfg(app);
    let (before, other) = if name == "shortcut" { (c.shortcut.clone(), c.whip_shortcut.clone()) } else { (c.whip_shortcut.clone(), c.shortcut.clone()) };
    if other == accel {
        return json!({ "ok": false, "error": "That one is already used for the other shortcut." });
    }
    let apply = |c: &mut Config, v: String| if name == "shortcut" { c.shortcut = v } else { c.whip_shortcut = v };
    set_cfg(app, |c| apply(c, accel.clone()));
    register_shortcuts(app);
    if !st(app).shortcut_ok.lock().unwrap().get(name).copied().unwrap_or(false) {
        set_cfg(app, |c| apply(c, before));
        register_shortcuts(app);
        return json!({ "ok": false, "error": "Another app (or macOS) already uses that one. Try a different combination." });
    }
    rebuild_menus(app);
    json!({ "ok": true })
}

// --------------------------------------------------------------- windows
fn overlay_panel(app: &AppHandle, label: &str, page: &str, level: PanelLevel) -> tauri::Result<()> {
    PanelBuilder::<_, OverlayPanel>::new(app, label)
        .url(WebviewUrl::App(page.into()))
        .level(level)
        .transparent(true)
        .has_shadow(false)
        .no_activate(true)
        .hides_on_deactivate(false)
        .becomes_key_only_if_needed(true)
        .style_mask(StyleMask::empty().nonactivating_panel().borderless())
        .collection_behavior(CollectionBehavior::new().can_join_all_spaces().stationary().full_screen_auxiliary().ignores_cycle())
        .size(tauri::Size::Logical(LogicalSize::new(100.0, 100.0)))
        // The panel's own `transparent` only clears the NSWindow; the web view inside still paints
        // white unless it's told to be transparent too.
        .with_window(|w| w.transparent(true).background_color(tauri::window::Color(0, 0, 0, 0)).decorations(false).resizable(false).skip_taskbar(true).always_on_top(true).visible(false).focused(false).shadow(false).accept_first_mouse(true))
        .build()?;
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.set_ignore_cursor_events(true);
    }
    st(app).ignoring.lock().unwrap().insert(label.to_string(), true);
    Ok(())
}

/// The app window: Run / Editor / Settings / Debug tabs.
fn open_app_window(app: &AppHandle, tab: Option<&str>, pet_key: Option<&str>) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(w) = app.get_webview_window(APP) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(t) = tab { emit(app, APP, "app:tab", json!(t)); }
        if let Some(k) = pet_key { emit(app, APP, "studio:open", json!(k)); }
        return;
    }
    let init = format!(
        "window.__INITIAL_TAB__ = {}; window.__INITIAL_PET__ = {};",
        json!(tab.unwrap_or("home")),
        json!(pet_key)
    );
    let built = WebviewWindowBuilder::new(app, APP, WebviewUrl::App("app.html".into()))
        .title("Promptagotchi")
        .inner_size(1180.0, 800.0)
        .min_inner_size(900.0, 620.0)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .initialization_script(&init)
        .build();
    if let Ok(w) = built {
        let app2 = app.clone();
        w.on_window_event(move |e| {
            if let tauri::WindowEvent::Destroyed = e {
                // Closing the window keeps the pet running (it lives in the menu bar).
                #[cfg(target_os = "macos")]
                let _ = app2.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        });
    }
}

// ----------------------------------------------------------------- menus
fn menu_item(app: &AppHandle, id: &str, label: &str, enabled: bool) -> MenuItem<Wry> {
    MenuItem::with_id(app, id, label, enabled, None::<&str>).unwrap()
}
fn check(app: &AppHandle, id: &str, label: &str, checked: bool) -> CheckMenuItem<Wry> {
    CheckMenuItem::with_id(app, id, label, true, checked, None::<&str>).unwrap()
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let a = st(app);
    let c = cfg(app);
    let menu = Menu::new(app)?;
    let sep = || PredefinedMenuItem::separator(app).unwrap();
    menu.append(&menu_item(app, "open-app", "Open Promptagotchi…", true))?;
    menu.append(&sep())?;
    let current = a.pet.lock().unwrap()["key"].as_str().unwrap_or("").to_string();
    let list = pets::list_pets();
    for (src, label) in [("builtin", "Pets"), ("user", "Your pets"), ("codex", "From Codex"), ("petdex", "From Petdex")] {
        let group: Vec<&Value> = list.iter().filter(|p| p["source"] == src).collect();
        if group.is_empty() { continue; }
        menu.append(&menu_item(app, "noop", label, false))?;
        for p in group {
            let key = p["key"].as_str().unwrap_or("");
            let name = p["name"].as_str().unwrap_or(key);
            let item = CheckMenuItem::with_id(app, format!("pet:{key}"), if p["error"].is_string() { format!("{name} (broken)") } else { name.to_string() }, p["error"].is_null(), key == current, None::<&str>)?;
            menu.append(&item)?;
        }
    }
    let tools = Submenu::with_items(app, "Make or install pets", true, &[
        &menu_item(app, "editor", "Pet Editor…", true),
        &menu_item(app, "install-pet", "Install a pet from a folder…", true),
        &menu_item(app, "pets-folder", "Open pets folder", true),
        &menu_item(app, "reload-pets", "Reload pets", true),
    ])?;
    menu.append(&tools)?;
    menu.append(&sep())?;
    menu.append(&menu_item(app, "noop", "Size (scales with your screen)", false))?;
    for (k, label) in [("small", "Small"), ("medium", "Medium"), ("large", "Large"), ("huge", "Huge")] {
        menu.append(&check(app, &format!("size:{k}"), label, c.size == k))?;
    }
    menu.append(&sep())?;
    let n = a.store.lock().unwrap().snapshot().len();
    menu.append(&menu_item(app, "noop", &format!("{n} Claude session{}", if n == 1 { "" } else { "s" }), false))?;
    let status = hooks::status(&paths::get().claude_settings());
    let hold = Submenu::with_items(app, "Hold prompts for", c.answer_from_pet, &[
        &check(app, "hold:20", "20 seconds", c.hold_seconds == 20),
        &check(app, "hold:45", "45 seconds", c.hold_seconds == 45),
        &check(app, "hold:90", "90 seconds", c.hold_seconds == 90),
        &check(app, "hold:180", "3 minutes", c.hold_seconds == 180),
    ])?;
    let status_label = match status { "full" => "Live hooks: connected (with answering)", "status" => "Live hooks: connected", _ => "Live hooks: not connected" };
    let claude = Submenu::with_items(app, "Claude Code", true, &[
        &menu_item(app, "noop", status_label, false),
        &if status == "none" { menu_item(app, "hooks-connect", "Connect hooks (adds them to ~/.claude/settings.json)", true) } else { menu_item(app, "hooks-disconnect", "Disconnect hooks", true) },
        &sep(),
        &check(app, "answer", "Answer prompts from the pet", c.answer_from_pet),
        &hold,
        &sep(),
        &menu_item(app, "settings-json", "Open Claude settings.json", true),
    ])?;
    menu.append(&claude)?;
    menu.append(&sep())?;
    menu.append(&check(app, "quiet", "Show quiet sessions", c.show_quiet_sessions))?;
    menu.append(&check(app, "sound", "Sound on questions", c.sound))?;
    menu.append(&sep())?;
    menu.append(&check(app, "whip", "Whip mode", a.whip.lock().unwrap().is_some()))?;
    menu.append(&menu_item(app, "corner", "Move back to the corner", true))?;
    menu.append(&menu_item(app, "toggle", if a.hidden.load(Ordering::Relaxed) { "Show pet" } else { "Hide pet" }, true))?;
    menu.append(&menu_item(app, "help", "Help: making your own pet", true))?;
    menu.append(&menu_item(app, "quit", "Quit Promptagotchi", true))?;
    Ok(menu)
}

fn rebuild_menus(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("tray"), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open-app" => open_app_window(app, Some("home"), None),
        "editor" => {
            let key = st(app).pet.lock().unwrap()["key"].as_str().map(String::from);
            open_app_window(app, Some("editor"), key.as_deref());
        }
        "install-pet" => install_pet(app),
        "pets-folder" => open_pets_folder(),
        "reload-pets" => { load_current_pet(app); replace_keeping_feet(app); send_init(app, PET); send_init(app, PANEL); rebuild_menus(app); }
        "hooks-connect" => connect_hooks(app, cfg(app).answer_from_pet),
        "hooks-disconnect" => disconnect_hooks(app),
        "answer" => set_answer(app, !cfg(app).answer_from_pet),
        "settings-json" => { let _ = std::process::Command::new("open").arg(paths::get().claude_settings()).status(); }
        "quiet" => { set_cfg(app, |c| c.show_quiet_sessions = !c.show_quiet_sessions); send_init(app, PANEL); rebuild_menus(app); }
        "sound" => { set_cfg(app, |c| c.sound = !c.sound); send_init(app, PET); rebuild_menus(app); }
        "whip" => { let on = st(app).whip.lock().unwrap().is_none(); set_whip(app, on); }
        "corner" => { set_cfg(app, |c| c.place = None); place_from_config(app); }
        "toggle" => toggle_pet(app),
        "help" => { let _ = std::process::Command::new("open").arg(paths::get().docs.join("making-pets.md")).status(); }
        "quit" => { stop_simulator(app); app.exit(0); }
        _ => {
            if let Some(k) = id.strip_prefix("pet:") { set_pet(app, k); }
            else if let Some(s) = id.strip_prefix("size:") { set_size(app, s); }
            else if let Some(h) = id.strip_prefix("hold:") { if let Ok(h) = h.parse() { set_cfg(app, |c| c.hold_seconds = h); rebuild_menus(app); } }
        }
    }
}

fn tray_icon() -> tauri::image::Image<'static> {
    let art = [
        "....##########....",
        "....##########....",
        "....#..####..#....",
        "..###..####..###..",
        "..##############..",
        "....##########....",
        "....##########....",
        "....#.#....#.#....",
        "....#.#....#.#....",
    ];
    let (s, w, h, top) = (2usize, 36usize, 36usize, 6usize);
    let mut buf = vec![0u8; w * h * 4];
    for (y, row) in art.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            if ch != '#' { continue; }
            for dy in 0..s {
                for dx in 0..s {
                    let i = (((y + top) * s + dy) * w + (x * s + dx)) * 4;
                    buf[i + 3] = 255;
                }
            }
        }
    }
    tauri::image::Image::new_owned(buf, w as u32, h as u32)
}

// --------------------------------------------------------------- actions
fn connect_hooks(app: &AppHandle, answer: bool) {
    set_cfg(app, |c| c.answer_from_pet = answer);
    let p = paths::get();
    if let Err(e) = hooks::install(&p.claude_settings(), &p.hook_script_ref(), answer) {
        app.dialog().message(format!("Couldn't update {}:\n{e}\n\nNothing was changed.", p.claude_settings().display())).title("Promptagotchi").show(|_| {});
    }
    send_init(app, PET);
    send_init(app, PANEL);
    rebuild_menus(app);
}

fn disconnect_hooks(app: &AppHandle) {
    if let Err(e) = hooks::uninstall(&paths::get().claude_settings()) {
        app.dialog().message(e).title("Promptagotchi").show(|_| {});
    }
    send_init(app, PET);
    send_init(app, PANEL);
    rebuild_menus(app);
}

fn set_answer(app: &AppHandle, on: bool) {
    set_cfg(app, |c| c.answer_from_pet = on);
    if hooks::status(&paths::get().claude_settings()) != "none" {
        connect_hooks(app, on);
    } else {
        rebuild_menus(app);
    }
}

fn open_pets_folder() {
    let dir = paths::get().user_pets();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("open").arg(dir).status();
}

fn install_pet(app: &AppHandle) {
    let app2 = app.clone();
    app.dialog().file().set_title("Choose a pet folder (it has a pet.json inside)").pick_folder(move |p| {
        let Some(p) = p.and_then(|p| p.into_path().ok()) else { return };
        match pets::install_pet_folder(&p) {
            Ok(key) => set_pet(&app2, &key),
            Err(e) => { app2.dialog().message(e).title("Promptagotchi").show(|_| {}); }
        }
    });
}

fn start_simulator(app: &AppHandle, n: usize) {
    let a = st(app);
    let mut sim = a.sim.lock().unwrap();
    if sim.is_none() {
        *sim = Some(Simulator::start(a.store.clone(), n, true));
    }
    drop(sim);
    push_app_state(app);
}

fn stop_simulator(app: &AppHandle) {
    let sim = st(app).sim.lock().unwrap().take();
    if let Some(s) = sim {
        std::thread::spawn(move || s.stop());
    }
    push_app_state(app);
}

fn app_state(app: &AppHandle) -> Value {
    let a = st(app);
    let p = paths::get();
    let home = &p.home;
    let packaged = !cfg!(debug_assertions);
    json!({
        "config": cfg(app),
        "hooks": hooks::status(&p.claude_settings()),
        "pets": pets::list_pets(),
        "pet": a.pet.lock().unwrap().clone(),
        "hidden": a.hidden.load(Ordering::Relaxed),
        "sessions": a.store.lock().unwrap().snapshot(),
        "server": a.server.lock().unwrap().clone(),
        "simulator": { "running": a.sim.lock().unwrap().as_ref().map(|s| s.is_running()).unwrap_or(false) },
        "paths": {
            "data": util::tilde(&p.data_dir, home), "pets": util::tilde(&p.user_pets(), home), "hookScript": util::tilde(&p.hook_script(), home),
            "claudeSettings": util::tilde(&p.claude_settings(), home), "claudeSessions": util::tilde(&p.claude_sessions(), home),
        },
        "login": { "supported": packaged, "enabled": packaged && app.autolaunch().is_enabled().unwrap_or(false) },
        "whip": { "on": a.whip.lock().unwrap().is_some(), "hustleUntil": a.hustle_until.load(Ordering::Relaxed) },
        "shortcuts": { "shortcut": cfg(app).shortcut, "whipShortcut": cfg(app).whip_shortcut, "ok": a.shortcut_ok.lock().unwrap().clone() },
        "eventCount": a.event_count.load(Ordering::Relaxed),
        "version": app.package_info().version.to_string(),
        "packaged": packaged,
        "runtime": format!("Rust core · Tauri {} · system WebKit", tauri::VERSION),
        "os": a.os,
    })
}

static LAST_PUSH: Mutex<Option<Instant>> = Mutex::new(None);
fn push_app_state(app: &AppHandle) {
    if app.get_webview_window(APP).is_none() {
        return;
    }
    let mut last = LAST_PUSH.lock().unwrap();
    if last.map(|t| t.elapsed() < Duration::from_millis(200)).unwrap_or(false) {
        return; // the dispatcher's next tick sends the latest state
    }
    *last = Some(Instant::now());
    drop(last);
    emit(app, APP, "control:state", app_state(app));
}

fn act(app: &AppHandle, name: &str, arg: Value) -> Value {
    let a = st(app);
    let out = match name {
        "setPet" => { set_pet(app, arg.as_str().unwrap_or("")); Value::Bool(true) }
        "setSize" => { set_size(app, arg.as_str().unwrap_or("")); Value::Bool(true) }
        "togglePet" => { toggle_pet(app); Value::Bool(true) }
        "moveToCorner" => { set_cfg(app, |c| c.place = None); place_from_config(app); Value::Bool(true) }
        "connectHooks" => { connect_hooks(app, arg.as_bool().unwrap_or(true)); Value::Bool(true) }
        "disconnectHooks" => { disconnect_hooks(app); Value::Bool(true) }
        "setAnswer" => { set_answer(app, arg.as_bool().unwrap_or(false)); Value::Bool(true) }
        "setHold" => { if let Some(h) = arg.as_u64().filter(|h| [20, 45, 90, 180].contains(h)) { set_cfg(app, |c| c.hold_seconds = h); } Value::Bool(true) }
        "openSession" => open_session(app, arg.as_str().unwrap_or("")),
        "dismiss" => json!(a.store.lock().unwrap().dismiss(arg.as_str().unwrap_or(""))),
        "clearFinished" => { a.store.lock().unwrap().clear_finished(); Value::Bool(true) }
        "startSim" => { start_simulator(app, arg.as_u64().unwrap_or(8) as usize); Value::Bool(true) }
        "stopSim" => { stop_simulator(app); Value::Bool(true) }
        "openData" => { let _ = std::fs::create_dir_all(&paths::get().data_dir); let _ = std::process::Command::new("open").arg(&paths::get().data_dir).status(); Value::Bool(true) }
        "openSettings" => { let _ = std::process::Command::new("open").arg(paths::get().claude_settings()).status(); Value::Bool(true) }
        "copyDiagnostics" => {
            let state = app_state(app);
            let diag = json!({
                "app": format!("Promptagotchi {}", state["version"].as_str().unwrap_or("")), "runtime": state["runtime"], "os": state["os"],
                "hooks": state["hooks"], "server": state["server"], "pet": state["pet"]["key"], "config": state["config"],
                "sessions": state["sessions"].as_array().map(|s| s.len()).unwrap_or(0),
                "recentEvents": a.events.lock().unwrap().iter().rev().take(20).cloned().collect::<Vec<_>>(),
            });
            if let Ok(mut child) = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() { let _ = stdin.write_all(serde_json::to_string_pretty(&diag).unwrap_or_default().as_bytes()); }
                let _ = child.wait();
            }
            Value::Bool(true)
        }
        "setSound" => { set_cfg(app, |c| c.sound = arg.as_bool().unwrap_or(false)); send_init(app, PET); Value::Bool(true) }
        "setShowQuiet" => { set_cfg(app, |c| c.show_quiet_sessions = arg.as_bool().unwrap_or(true)); send_init(app, PANEL); Value::Bool(true) }
        "setLogin" => {
            if !cfg!(debug_assertions) {
                let _ = if arg.as_bool().unwrap_or(false) { app.autolaunch().enable() } else { app.autolaunch().disable() };
            }
            set_cfg(app, |c| c.login_chosen = true);
            Value::Bool(true)
        }
        "openPetsFolder" => { open_pets_folder(); Value::Bool(true) }
        "installPet" => { install_pet(app); Value::Bool(true) }
        "reloadPets" => { load_current_pet(app); replace_keeping_feet(app); send_init(app, PET); send_init(app, PANEL); Value::Bool(true) }
        "setWhip" => { set_whip(app, arg.as_bool().unwrap_or(false)); Value::Bool(true) }
        "setWhipSound" => { set_cfg(app, |c| c.whip_sound = arg.as_bool().unwrap_or(true)); Value::Bool(true) }
        "setShortcut" => set_shortcut(app, &arg),
        _ => return json!({ "error": format!("unknown action {name}") }),
    };
    rebuild_menus(app);
    push_app_state(app);
    out
}

// ------------------------------------------------------------------ IPC
/// Everything the screens ask for comes through here (see src/renderer/bridge.js).
#[tauri::command]
async fn ipc(app: AppHandle, window: tauri::WebviewWindow, channel: String, payload: Value) -> Result<Value, String> {
    let label = window.label().to_string();
    let a = st(&app);
    let ok = Value::Bool(true);
    if channel != "pet:drag" {
        debug(format!("{label} → {channel} {}", util::clip(&payload.to_string(), 120)));
    }
    Ok(match channel.as_str() {
        "ui:ready" => { send_init(&app, &label); ok }
        "ui:hot" => {
            let rects: Vec<[f64; 4]> = serde_json::from_value(payload).unwrap_or_default();
            a.hot.lock().unwrap().insert(label, rects);
            ok
        }
        "pet:drag" => { drag(&app, &payload); ok }
        "pet:click" => { on_pet_click(&app); ok }
        "pet:menu" => {
            if let (Some(w), Ok(menu)) = (app.get_webview_window(PET), build_menu(&app)) {
                let _ = w.popup_menu(&menu);
            }
            ok
        }
        "pet:content" => {
            let mut pet = a.pet.lock().unwrap();
            if pet["key"] == payload["key"] && pet["content"].is_null() && payload["bottom"].as_f64() >= payload["top"].as_f64() {
                pet["content"] = json!({ "top": payload["top"], "bottom": payload["bottom"] });
                drop(pet);
                replace_keeping_feet(&app);
            }
            ok
        }
        "session:focus" => open_session(&app, payload.as_str().unwrap_or("")),
        "session:seen" => { a.store.lock().unwrap().mark_seen(payload.as_str().unwrap_or("")); ok }
        "session:dismiss" => json!(a.store.lock().unwrap().dismiss(payload.as_str().unwrap_or(""))),
        "sessions:clear-finished" => { a.store.lock().unwrap().clear_finished(); ok }
        "question:answer" => json!(a.store.lock().unwrap().answer_question(payload["id"].as_str().unwrap_or(""), payload["answer"].clone())),
        "question:release" => json!(a.store.lock().unwrap().resolve_question(payload.as_str().unwrap_or(""), None, "released")),
        "hooks:connect" => { connect_hooks(&app, payload.as_bool().unwrap_or(true)); ok }
        "studio:open" => { open_app_window(&app, Some("editor"), payload.as_str()); ok }
        "whip:hit" => { on_whip_hit(&app, &payload); ok }
        "control:get" => {
            let mut s = app_state(&app);
            s["recentEvents"] = json!(a.events.lock().unwrap().iter().rev().take(150).rev().cloned().collect::<Vec<_>>());
            s
        }
        "control:act" => act(&app, payload["name"].as_str().unwrap_or(""), payload["arg"].clone()),
        "studio:list" => json!(pets::list_pets()),
        "studio:read" => {
            let key = payload.as_str().unwrap_or("");
            let (pet, j) = pets::read_pet_json(key)?;
            json!({ "pet": pet, "json": j, "normalized": pets::load_pet(key)? })
        }
        "studio:normalize" => pets::load_pet(payload.as_str().unwrap_or(""))?,
        "studio:save" => {
            let key = pets::save_pet_json(payload)?;
            if a.pet.lock().unwrap()["key"] == key {
                load_current_pet(&app);
                replace_keeping_feet(&app);
                send_init(&app, PET);
            }
            rebuild_menus(&app);
            json!({ "key": key, "normalized": pets::load_pet(&key)? })
        }
        "studio:use" => { set_pet(&app, payload.as_str().unwrap_or("")); ok }
        "studio:reveal" => {
            if let Some(p) = pets::find_pet(payload.as_str().unwrap_or("")) {
                let _ = std::process::Command::new("open").arg(p["dir"].as_str().unwrap_or("")).status();
            }
            ok
        }
        other => return Err(format!("unknown channel {other}")),
    })
}

fn drag(app: &AppHandle, msg: &Value) {
    let a = st(app);
    let (sx, sy) = (msg["screenX"].as_f64().unwrap_or(0.0), msg["screenY"].as_f64().unwrap_or(0.0));
    let Some(w) = app.get_webview_window(PET) else { return };
    let scale = w.scale_factor().unwrap_or(2.0);
    match msg["phase"].as_str().unwrap_or("") {
        "start" => {
            let pos = w.outer_position().map(|p| (p.x as f64 / scale, p.y as f64 / scale)).unwrap_or((0.0, 0.0));
            a.ui.lock().unwrap().drag = Some((pos.0, pos.1, sx, sy, Instant::now()));
            set_open(app, false);
            emit(app, PANEL, "hide-toast", json!({}));
        }
        "move" => {
            let mut ui = a.ui.lock().unwrap();
            if ui.drag.is_none() {
                // picked back up after the watchdog dropped it: carry on from here
                let pos = w.outer_position().map(|p| (p.x as f64 / scale, p.y as f64 / scale)).unwrap_or((0.0, 0.0));
                ui.drag = Some((pos.0, pos.1, sx, sy, Instant::now()));
            }
            let (x0, y0, sx0, sy0, _) = ui.drag.unwrap();
            ui.drag = Some((x0, y0, sx0, sy0, Instant::now()));
            drop(ui);
            let size = w.outer_size().map(|z| (z.width as f64 / scale, z.height as f64 / scale)).unwrap_or((100.0, 100.0));
            let d = display_at(app, sx, sy);
            // Follow the cursor, but never leave the screen it's on.
            let r = clamp_rect(Rect { x: x0 + sx - sx0, y: y0 + sy - sy0, width: size.0, height: size.1 }, &d);
            let _ = w.set_position(LogicalPosition::new(r.x, r.y));
        }
        "end" => finish_drag(app),
        _ => {}
    }
}

/// Drop the pet where it is: re-measure for the screen it landed on, then remember the spot.
fn finish_drag(app: &AppHandle) {
    let a = st(app);
    if a.ui.lock().unwrap().drag.take().is_none() {
        return;
    }
    let Some(w) = app.get_webview_window(PET) else { return };
    let scale = w.scale_factor().unwrap_or(2.0);
    let pos = w.outer_position().map(|p| (p.x as f64 / scale, p.y as f64 / scale)).unwrap_or((0.0, 0.0));
    let size = w.outer_size().map(|z| (z.width as f64 / scale, z.height as f64 / scale)).unwrap_or((100.0, 100.0));
    let m = a.geo.lock().unwrap().as_ref().map(|g| g.m);
    if let Some(m) = m {
        place_at(app, feet_of(&Rect { x: pos.0, y: pos.1, width: size.0, height: size.1 }, &m), None);
        remember_place(app);
    }
}

// -------------------------------------------------------------- loops
fn describe_event(evt: &Value, title: &str) -> Value {
    let s = |k: &str| evt[k].as_str().unwrap_or("").to_string();
    let name = s("hook_event_name");
    let (what, kind) = match name.as_str() {
        "PreToolUse" => (format!("{}{}", s("tool_name"), if evt["agent_id"].is_string() { " (subagent)" } else { "" }), ""),
        "PostToolUse" | "PostToolUseFailure" => (s("tool_name"), ""),
        "UserPromptSubmit" => (util::clip(&s("prompt"), 80), ""),
        "SubagentStart" | "SubagentStop" => (s("agent_type"), ""),
        "Notification" => (if s("notification_type").is_empty() { s("message") } else { s("notification_type") }, "attn"),
        "PermissionRequest" => (s("tool_name"), "attn"),
        "Stop" => (util::clip(&s("last_assistant_message"), 80), "done"),
        "SessionStart" => (s("source"), ""),
        "SessionEnd" => (s("reason"), ""),
        _ => (String::new(), ""),
    };
    json!({ "at": now_ms(), "name": name, "who": util::clip(title, 40), "what": what, "kind": kind })
}

/// Store events → windows: sessions (coalesced), attention, the live event log.
fn dispatcher(app: AppHandle, rx: std::sync::mpsc::Receiver<StoreEvent>) {
    let mut dirty = false;
    let mut last_flush = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(60)) {
            Ok(StoreEvent::Change) => dirty = true,
            Ok(StoreEvent::Attention(v)) => {
                emit(&app, PET, "attention", v.clone());
                emit(&app, PANEL, "attention", v.clone());
                let a = st(&app);
                if a.hidden.load(Ordering::Relaxed) && v["state"] == "question" {
                    show_pet(&app);
                }
            }
            Ok(StoreEvent::Hook { evt, title }) => {
                let a = st(&app);
                let d = describe_event(&evt, &title);
                let mut ev = a.events.lock().unwrap();
                ev.push_back(d.clone());
                if ev.len() > 300 { ev.pop_front(); }
                a.event_count.fetch_add(1, Ordering::Relaxed);
                a.pending.lock().unwrap().push(d);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return,
        }
        if dirty && last_flush.elapsed() > Duration::from_millis(50) {
            dirty = false;
            last_flush = Instant::now();
            let snap = st(&app).store.lock().unwrap().snapshot();
            emit(&app, PET, "sessions", json!(snap));
            emit(&app, PANEL, "sessions", json!(snap));
            *LAST_PUSH.lock().unwrap() = None;
            push_app_state(&app);
        }
        let batch: Vec<Value> = std::mem::take(&mut *st(&app).pending.lock().unwrap());
        if !batch.is_empty() && app.get_webview_window(APP).is_some() {
            emit(&app, APP, "control:events", json!(batch));
        }
    }
}

/// Click-through: a window takes the mouse only while the cursor is over its pet / cards.
/// Also drives hover-to-open, the close delay, and the drag watchdog.
fn pointer_loop(app: AppHandle) {
    // WebKit only tracks mouse movement in the key window, and these panels never become key
    // (so they don't steal your typing). Hover inside them is fed from here instead: bridge.js
    // turns `ptr` into mouseenter/mouseleave and :hover styles.
    let mut last_ptr: [Option<(i32, i32)>; 2] = [None, None];
    loop {
        std::thread::sleep(Duration::from_millis(30));
        let a = st(&app);
        let Ok(p) = app.cursor_position() else { continue };
        let hidden = a.hidden.load(Ordering::Relaxed);
        let mut hovers = [false, false];
        for (i, label) in [PET, PANEL].iter().enumerate() {
            let Some(w) = app.get_webview_window(label) else { continue };
            let (Ok(pos), Ok(scale)) = (w.outer_position(), w.scale_factor()) else { continue };
            let rects = a.hot.lock().unwrap().get(*label).cloned().unwrap_or_default();
            let inside = !hidden && rects.iter().any(|r| {
                let (x, y) = (pos.x as f64 + r[0] * scale, pos.y as f64 + r[1] * scale);
                p.x >= x && p.x <= x + r[2] * scale && p.y >= y && p.y <= y + r[3] * scale
            });
            hovers[i] = inside;
            let local = inside.then(|| (((p.x - pos.x as f64) / scale).round() as i32, ((p.y - pos.y as f64) / scale).round() as i32));
            if local != last_ptr[i] {
                last_ptr[i] = local;
                emit(&app, label, "ptr", local.map(|(x, y)| json!({ "x": x, "y": y })).unwrap_or(Value::Null));
            }
            let mut ign = a.ignoring.lock().unwrap();
            if ign.get(*label).copied() != Some(!inside) {
                debug(format!("{label}: {}", if inside { "takes the mouse" } else { "click-through" }));
                ign.insert(label.to_string(), !inside);
                let _ = w.set_ignore_cursor_events(!inside);
            }
        }
        let (changed, close_due, stale_drag) = {
            let mut ui = a.ui.lock().unwrap();
            let changed = ui.hover_pet != hovers[0] || ui.hover_panel != hovers[1];
            ui.hover_pet = hovers[0];
            ui.hover_panel = hovers[1];
            let close_due = ui.close_at.map(|t| Instant::now() >= t).unwrap_or(false);
            let stale = ui.drag.map(|d| d.4.elapsed() > Duration::from_secs(4)).unwrap_or(false);
            (changed, close_due, stale)
        };
        if changed {
            sync_open(&app);
        }
        if close_due {
            set_open(&app, false);
        }
        if stale_drag {
            finish_drag(&app);
        }
    }
}

/// While a prompt is held, switching to the session's app hands it back to Claude.
fn frontmost_loop(app: AppHandle) {
    loop {
        std::thread::sleep(Duration::from_millis(700));
        let a = st(&app);
        if !a.store.lock().unwrap().has_held_questions() {
            continue;
        }
        if let Some(b) = focus::frontmost_bundle_id() {
            a.store.lock().unwrap().release_for_app(&b);
        }
    }
}

fn whip_loop(app: AppHandle) {
    loop {
        std::thread::sleep(Duration::from_millis(16));
        if st(&app).whip.lock().unwrap().is_some() {
            whip_tick(&app);
        }
    }
}

// ------------------------------------------------------------------ boot
fn resources_dir(app: &AppHandle) -> PathBuf {
    let candidates = [app.path().resource_dir().ok(), Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))];
    candidates.into_iter().flatten().find(|d| d.join("pets").join("pip").join("pet.json").exists()).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
}

fn macos_version() -> String {
    let v = std::process::Command::new("sw_vers").arg("-productVersion").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    format!("macOS {v} · {}", std::env::consts::ARCH)
}

fn mime(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "png" | "apng" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

pub fn run() {
    let (tx, rx) = std::sync::mpsc::channel();
    let config = Arc::new(Mutex::new(Config::default()));
    let hidden = Arc::new(AtomicBool::new(false));
    let store: Store = Arc::new(Mutex::new(SessionStore::new()));
    store.lock().unwrap().set_events(tx);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_pet(app);
            open_app_window(app, None, None);
        }))
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--background"])))
        .register_uri_scheme_protocol("pet", |_ctx, req| {
            let url = req.uri().to_string();
            match pets::resolve_asset_url(&url).and_then(|f| std::fs::read(&f).ok().map(|b| (f, b))) {
                Some((f, bytes)) => tauri::http::Response::builder()
                    .header("Content-Type", mime(&f))
                    .header("Access-Control-Allow-Origin", "*")
                    .body(bytes)
                    .unwrap(),
                None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
            }
        })
        .invoke_handler(tauri::generate_handler![ipc])
        .setup(move |app| {
            let handle = app.handle().clone();
            paths::init(resources_dir(&handle));
            let p = paths::get();
            *config.lock().unwrap() = Config::load(&p.config_file());
            let _ = hooks::write_hook_script(&p.hook_src, &p.hook_script());

            // Hold policy: only while answering is on, the pet is out, and you're in another app.
            {
                let (config, hidden) = (config.clone(), hidden.clone());
                let mut s = store.lock().unwrap();
                s.hold_policy = Box::new(move |sess, meta| {
                    let c = config.lock().unwrap();
                    if !c.answer_from_pet || hidden.load(Ordering::Relaxed) {
                        return (false, 0);
                    }
                    if sess.host.bundle_id.is_some() && meta.frontmost.is_some() && sess.host.bundle_id == meta.frontmost {
                        return (false, 0);
                    }
                    (true, c.hold_seconds.max(5))
                });
                s.turn_ending = Box::new(|sess| transcripts::last_turn_ask(&paths::get().claude_projects(), &sess.session_id, sess.cwd.as_deref()));
            }

            app.manage(Arc::new(App {
                store: store.clone(),
                config: config.clone(),
                hidden: hidden.clone(),
                ui: Mutex::new(Ui::default()),
                geo: Mutex::new(None),
                pet: Mutex::new(Value::Null),
                hot: Mutex::new(HashMap::new()),
                ignoring: Mutex::new(HashMap::new()),
                events: Mutex::new(VecDeque::new()),
                pending: Mutex::new(Vec::new()),
                event_count: AtomicU64::new(0),
                server: Mutex::new(json!({ "ok": false })),
                sim: Mutex::new(None),
                whip: Mutex::new(None),
                hustle_until: AtomicI64::new(0),
                shortcut_ok: Mutex::new(HashMap::new()),
                os: macos_version(),
            }));

            #[cfg(target_os = "macos")]
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let info = match server::start(store.clone(), focus::frontmost_bundle_id) {
                Ok(port) => json!({ "ok": true, "port": port, "error": null }),
                Err(e) => json!({ "ok": false, "port": null, "error": e }),
            };
            *st(&handle).server.lock().unwrap() = info;

            load_current_pet(&handle);
            for m in handle.available_monitors().unwrap_or_default() {
                debug(format!("monitor {:?}: pos {:?} size {:?} work {:?} scale {}", m.name(), m.position(), m.size(), m.work_area(), m.scale_factor()));
            }
            overlay_panel(&handle, PANEL, "panel.html", PanelLevel::Custom(1000))?;
            overlay_panel(&handle, PET, "pet.html", PanelLevel::Custom(1001))?;
            place_from_config(&handle);
            for label in [PANEL, PET] {
                if let Some(w) = handle.get_webview_window(label) {
                    let _ = w.show();
                }
            }

            let menu = build_menu(&handle)?;
            TrayIconBuilder::with_id("tray")
                .icon(tray_icon())
                .icon_as_template(true)
                .tooltip("Promptagotchi")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .build(app)?;
            app.on_menu_event(|app, e| on_menu(app, e.id().as_ref()));
            register_shortcuts(&handle);

            session_files::spawn(store.clone());
            {
                let h = handle.clone();
                std::thread::spawn(move || dispatcher(h, rx));
            }
            for f in [pointer_loop as fn(AppHandle), frontmost_loop, whip_loop] {
                let h = handle.clone();
                std::thread::spawn(move || f(h));
            }

            // Open at login starts on in the installed app; once it's set up, it's yours to turn off.
            if !cfg!(debug_assertions) && !cfg(&handle).login_chosen {
                let _ = handle.autolaunch().enable();
                set_cfg(&handle, |c| c.login_chosen = true);
            }

            // Opening the app shows the window too, unless it was started quietly (at login).
            let quiet = std::env::args().any(|a| a == "--background");
            if !quiet {
                open_app_window(&handle, Some("home"), None);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Promptagotchi")
        .run(|app, event| match event {
            // Closing windows never quits; the pet lives in the menu bar until you choose Quit.
            RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => {
                show_pet(app);
                open_app_window(app, None, None);
            }
            RunEvent::Exit => stop_simulator(app),
            _ => {}
        });
}
