//! Web display: connects to the hub that served this page and renders the same
//! dashboards as the desktop app. Only meaningful on wasm32.
#![cfg(target_arch = "wasm32")]

use cluster_core::wire::{self, ClientMsg, ServerMsg};
use cluster_core::{Snapshot, UnitPrefs};
use cluster_layout::Compiled;
use cluster_ui::{DashboardView, MainWindow};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, MessageEvent, WebSocket};

#[derive(Default)]
struct WebState {
    view: Option<DashboardView>,
    snap: Snapshot,
    last_frame_ms: Option<f64>,
    units: UnitPrefs,
    dashboards: Vec<String>,
    current: String,
    hub_status: (String, String, String),
    ws: Option<WebSocket>,
    connected: bool,
    toast_until: f64,
}

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

fn send(st: &WebState, msg: &ClientMsg) {
    if let Some(ws) = &st.ws {
        let _ = ws.send_with_str(&serde_json::to_string(msg).unwrap_or_default());
    }
}

fn bind_view(ui: &MainWindow, view: &mut DashboardView) {
    let (w, h) = view.canvas();
    ui.set_canvas_w(w);
    ui.set_canvas_h(h);
    ui.set_statics(view.statics_model());
    ui.set_frames(view.frames_model());
    let (c0, c1, angle, image) = view.background();
    ui.set_bg(c0);
    ui.set_bg2(c1);
    ui.set_bg_angle(angle);
    ui.set_bg_image(image);
    ui.set_dashboard_name(view.name().into());
}

fn on_text(ui: &MainWindow, st: &mut WebState, text: &str) {
    match serde_json::from_str::<ServerMsg>(text) {
        Ok(ServerMsg::Hello { dashboards, current, units, .. }) => {
            st.dashboards = dashboards;
            if st.current.is_empty() {
                st.current = current;
            }
            st.units = if units == "imperial" { UnitPrefs::imperial() } else { UnitPrefs::default() };
            if let Some(v) = &mut st.view {
                v.set_units(st.units, &st.snap);
            }
        }
        Ok(ServerMsg::Layout { name, source }) => match Compiled::from_json5(&source) {
            Ok(c) => {
                let mut view = DashboardView::new(c, st.units, None);
                bind_view(ui, &mut view);
                st.view = Some(view);
                st.current = name;
                ui.set_error_text("".into());
            }
            Err(e) => ui.set_error_text(format!("{name}: {e}").into()),
        },
        Ok(ServerMsg::Status { source, state, detail }) => st.hub_status = (source, state, detail),
        Err(e) => log_console(&format!("bad message: {e}")),
    }
}

fn log_console(s: &str) {
    web_sys::console::log_1(&JsValue::from_str(s));
}

fn connect(ui: slint::Weak<MainWindow>, state: Rc<RefCell<WebState>>) {
    let Some(window) = web_sys::window() else { return };
    let host = window.location().host().unwrap_or_default();
    let proto = if window.location().protocol().unwrap_or_default() == "https:" { "wss" } else { "ws" };
    let Ok(ws) = WebSocket::new(&format!("{proto}://{host}/ws")) else { return };
    ws.set_binary_type(BinaryType::Arraybuffer);

    let (ui2, st2) = (ui.clone(), state.clone());
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let data = e.data();
        if let Ok(buf) = data.dyn_into::<js_sys::ArrayBuffer>() {
            let bytes = js_sys::Uint8Array::new(&buf).to_vec();
            if let Some(s) = wire::decode_frame(&bytes) {
                let mut st = st2.borrow_mut();
                st.snap = s;
                st.last_frame_ms = Some(now_ms());
            }
        } else if let Some(text) = e.data().as_string()
            && let Some(ui) = ui2.upgrade()
        {
            on_text(&ui, &mut st2.borrow_mut(), &text);
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let st3 = state.clone();
    let onopen = Closure::<dyn FnMut()>::new(move || {
        let mut st = st3.borrow_mut();
        st.connected = true;
        // Re-request this display's dashboard after a reconnect.
        if !st.current.is_empty() {
            let msg = ClientMsg::Select { name: st.current.clone() };
            send(&st, &msg);
        }
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let (ui4, st4) = (ui.clone(), state.clone());
    let onclose = Closure::<dyn FnMut()>::new(move || {
        st4.borrow_mut().connected = false;
        let (ui5, st5) = (ui4.clone(), st4.clone());
        slint::Timer::single_shot(std::time::Duration::from_secs(1), move || connect(ui5, st5));
    });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    state.borrow_mut().ws = Some(ws);
}

fn step_dashboard(state: &Rc<RefCell<WebState>>, delta: isize) {
    let st = state.borrow();
    if st.dashboards.is_empty() {
        return;
    }
    let n = st.dashboards.len() as isize;
    let i = st.dashboards.iter().position(|d| *d == st.current).unwrap_or(0) as isize;
    let name = st.dashboards[((i + delta).rem_euclid(n)) as usize].clone();
    send(&st, &ClientMsg::Select { name });
}

/// Sizes the Slint window to the browser viewport (Slint would otherwise use the
/// window's preferred size), and keeps it in sync on rotation and resizes.
fn fit_to_viewport(ui: &MainWindow) {
    let fit = |ui: &MainWindow| {
        if let Some(w) = web_sys::window() {
            let width = w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(800.0) as f32;
            let height = w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(600.0) as f32;
            ui.window().set_size(slint::LogicalSize::new(width, height));
        }
    };
    fit(ui);
    let weak = ui.as_weak();
    let on_resize = Closure::<dyn FnMut()>::new(move || {
        if let Some(ui) = weak.upgrade() {
            fit(&ui);
        }
    });
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
    }
    on_resize.forget();
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let ui = MainWindow::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
    ui.set_show_status(false);
    ui.set_touch_nav(true);
    fit_to_viewport(&ui);
    let state = Rc::new(RefCell::new(WebState::default()));
    connect(ui.as_weak(), state.clone());

    {
        let s = state.clone();
        ui.on_next_dashboard(move || step_dashboard(&s, 1));
        let s = state.clone();
        ui.on_previous_dashboard(move || step_dashboard(&s, -1));
        let (s, weak) = (state.clone(), ui.as_weak());
        ui.on_toggle_units(move || {
            let mut st = s.borrow_mut();
            st.units = if st.units == UnitPrefs::default() { UnitPrefs::imperial() } else { UnitPrefs::default() };
            let (units, snap) = (st.units, st.snap.clone());
            if let Some(v) = &mut st.view {
                v.set_units(units, &snap);
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_toast_text(
                    if units == UnitPrefs::default() { "Units: metric" } else { "Units: imperial" }.into(),
                );
                st.toast_until = now_ms() + 1500.0;
            }
        });
        let weak = ui.as_weak();
        ui.on_toggle_fullscreen(move || {
            if let Some(ui) = weak.upgrade() {
                let w = ui.window();
                w.set_fullscreen(!w.is_fullscreen());
            }
        });
    }

    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    let mut last = now_ms();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), move || {
        let Some(ui) = weak.upgrade() else { return };
        let now = now_ms();
        let dt = ((now - last) / 1000.0) as f32;
        last = now;
        let mut st = state.borrow_mut();
        let snap = st.snap.clone();
        if let Some(v) = &mut st.view {
            v.update(&snap, dt);
        }
        let stale = st.last_frame_ms.is_none_or(|t| now - t > 500.0);
        let paused = snap.get(cluster_core::SignalId::RaceOn) == Some(0.0);
        let (title, detail) = if !st.connected {
            (
                "Connecting to DashCluster…".to_owned(),
                "Make sure the desktop app is running and on the same Wi-Fi.".to_owned(),
            )
        } else if stale {
            let (src, _, detail) = &st.hub_status;
            (
                "Waiting for telemetry…".to_owned(),
                if src.is_empty() { String::new() } else { format!("{src}: {detail}") },
            )
        } else if paused {
            ("Game paused or in a menu".to_owned(), String::new())
        } else {
            (String::new(), String::new())
        };
        ui.set_overlay_title(title.into());
        ui.set_overlay_detail(detail.into());
        ui.set_stale(stale || paused || !st.connected);
        if st.toast_until > 0.0 && now > st.toast_until {
            st.toast_until = 0.0;
            ui.set_toast_text("".into());
        }
    });
    std::mem::forget(timer);
    ui.run().map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(())
}
