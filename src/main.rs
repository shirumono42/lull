#![cfg_attr(not(test), windows_subsystem = "windows")]

mod audio;
mod platform;
mod pomodoro;
mod settings;
mod ui;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

use audio::Engine;
use pomodoro::{Event, Phase, Timer, clock};
use settings::{OVERLAY_SIZE_RANGE, Settings};
use ui::{Painter, Response, Tab, TimerCmd, TimerView, Ui};

/// Settings are written this long after the last change made outside the panel.
const SAVE_DELAY: Duration = Duration::from_secs(2);

enum UserEvent {
    Hotkey(GlobalHotKeyEvent),
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

/// A borderless window with a software-rendered surface.
struct Canvas {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    painter: Painter,
}

impl Canvas {
    fn new(el: &ActiveEventLoop, attrs: WindowAttributes) -> Option<Self> {
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("lull: window error: {e}");
                return None;
            }
        };
        let surface = softbuffer::Context::new(window.clone())
            .and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone()));
        match surface {
            Ok(surface) => {
                let painter = Painter::new(window.scale_factor() as f32);
                Some(Canvas { window, surface, painter })
            }
            Err(e) => {
                eprintln!("lull: surface error: {e}");
                None
            }
        }
    }

    /// Draws at `zoom` × the window's DPI scale and presents the frame.
    fn present(&mut self, zoom: f32, draw: impl FnOnce(&mut Painter)) {
        let size = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if self.surface.resize(w, h).is_err() {
            return;
        }
        let scale = self.window.scale_factor() as f32 * zoom;
        self.painter.resize(size.width, size.height, scale);
        draw(&mut self.painter);
        if let Ok(mut buf) = self.surface.buffer_mut() {
            self.painter.blit(&mut buf);
            let _ = buf.present();
        }
    }
}

/// The popup. It only exists while visible; hiding drops it entirely.
struct Panel {
    canvas: Canvas,
    ui: Ui,
    cursor: (f32, f32),
    had_focus: bool,
}

/// The floating on-screen timer. Exists only while it is shown.
struct Overlay {
    canvas: Canvas,
    last_press: Option<Instant>,
}

#[derive(Clone, PartialEq)]
enum IconKey {
    Bars(bool),
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Timer(String, Phase, bool),
}

struct TrayParts {
    icon: TrayIcon,
    play_item: MenuItem,
    timer_item: MenuItem,
    show_id: MenuId,
    play_id: MenuId,
    timer_id: MenuId,
    reset_id: MenuId,
    quit_id: MenuId,
    icon_key: Option<IconKey>,
    shown: String,
}

struct App {
    settings: Settings,
    /// When unsaved changes were last made.
    dirty: Option<Instant>,
    engine: Engine,
    timer: Timer,
    panel: Option<Panel>,
    overlay: Option<Overlay>,
    tab: Tab,
    tray: Option<TrayParts>,
    _hotkeys: Option<GlobalHotKeyManager>,
    hk_play: u32,
    hk_panel: u32,
    hk_timer: u32,
    /// Footer text listing the shortcuts (and any that failed to register).
    hint: String,
    last_hide: Option<Instant>,
    /// Windows: next attempt to pin the tray icon, while still trying.
    pin_retry: Option<(Instant, u32)>,
}

impl App {
    fn new() -> Self {
        let settings = Settings::load();
        let defaults = Settings::default();
        let hotkeys = GlobalHotKeyManager::new().ok();
        let mut ids = [0u32; 3];
        let mut hint = Vec::new();
        let specs = [
            (&settings.hotkey_play, &defaults.hotkey_play, "sound"),
            (&settings.hotkey_timer, &defaults.hotkey_timer, "timer"),
            (&settings.hotkey_panel, &defaults.hotkey_panel, "panel"),
        ];
        for (i, (spec, fallback, what)) in specs.into_iter().enumerate() {
            let (hk, spec) = spec
                .parse::<HotKey>()
                .map(|hk| (hk, spec.as_str()))
                .unwrap_or_else(|_| (fallback.parse().unwrap(), fallback.as_str()));
            ids[i] = hk.id();
            let ok = hotkeys.as_ref().is_some_and(|m| m.register(hk).is_ok());
            let busy = if ok { "" } else { " (in use)" };
            hint.push(format!("{}{busy}  {what}", settings::pretty_hotkey(spec)));
        }
        App {
            engine: Engine::new(&settings),
            settings,
            dirty: None,
            timer: Timer::default(),
            panel: None,
            overlay: None,
            tab: Tab::Sound,
            tray: None,
            _hotkeys: hotkeys,
            hk_play: ids[0],
            hk_timer: ids[1],
            hk_panel: ids[2],
            hint: hint.join("      "),
            last_hide: None,
            pin_retry: None,
        }
    }

    /// Windows: retries pinning the tray icon to the taskbar for ~30 s after
    /// the first launch (Explorer registers new icons lazily).
    fn try_pin_tray(&mut self, now: Instant) {
        let Some((at, tries)) = self.pin_retry else { return };
        if now < at {
            return;
        }
        #[cfg(windows)]
        let done = platform::promote_tray_icon();
        #[cfg(not(windows))]
        let done = true;
        if done || tries >= 10 {
            self.pin_retry = None;
            if done {
                self.settings.tray_pinned = true;
                self.dirty = Some(now);
            }
        } else {
            self.pin_retry = Some((now + Duration::from_secs(3), tries + 1));
        }
    }

    fn create_tray(&mut self) {
        let show = MenuItem::new("Show panel", true, None);
        let play = MenuItem::new("Play", true, None);
        let timer = MenuItem::new("Start timer", true, None);
        let reset = MenuItem::new("Reset timer", true, None);
        let quit = MenuItem::new("Quit Lull", true, None);
        let menu = Menu::new();
        let sep = PredefinedMenuItem::separator();
        let _ = menu.append_items(&[&show, &play, &timer, &reset, &sep, &quit]);
        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Lull");
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(bars_icon(false));
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(bars_icon(false));
        match builder.build() {
            Ok(icon) => {
                self.tray = Some(TrayParts {
                    icon,
                    show_id: show.id().clone(),
                    play_id: play.id().clone(),
                    timer_id: timer.id().clone(),
                    reset_id: reset.id().clone(),
                    quit_id: quit.id().clone(),
                    play_item: play,
                    timer_item: timer,
                    icon_key: Some(IconKey::Bars(false)),
                    shown: String::new(),
                })
            }
            Err(e) => eprintln!("lull: tray icon unavailable: {e}"),
        }
    }

    fn timer_view(&self) -> TimerView {
        let now = Instant::now();
        let plan = self.settings.plan();
        TimerView {
            active: self.timer.is_active(),
            running: self.timer.is_running(),
            phase: self.timer.phase,
            round: self.timer.round,
            rounds: plan.rounds,
            remaining: self.timer.remaining(&plan, now),
            progress: if self.timer.is_active() { self.timer.progress(&plan, now) } else { 0.0 },
        }
    }

    fn toggle_play(&mut self) {
        self.engine.toggle();
        self.refresh();
    }

    /// Brings tray, panel and overlay up to date. Cheap when nothing changed.
    fn refresh(&mut self) {
        let playing = self.engine.is_playing();
        let tv = self.timer_view();
        let timer_text = tv.active.then(|| {
            let paused = if tv.running { "" } else { " (paused)" };
            format!("{} {}{paused}", ui::phase_name(tv.phase), clock(tv.remaining))
        });
        if let Some(t) = &mut self.tray {
            let shown = format!("{playing}{timer_text:?}");
            if shown != t.shown {
                t.shown = shown;
                t.play_item.set_text(if playing { "Stop sound" } else { "Play sound" });
                let timer_label = match (tv.active, tv.running) {
                    (_, true) => "Pause timer",
                    (true, false) => "Resume timer",
                    _ => "Start timer",
                };
                t.timer_item.set_text(timer_label);
                let mut tip = String::from(if playing { "Lull — playing" } else { "Lull" });
                if let Some(tt) = &timer_text {
                    tip = format!("{tip} · {tt}");
                }
                let _ = t.icon.set_tooltip(Some(tip));
                // macOS shows the countdown as text next to the menu bar icon.
                #[cfg(target_os = "macos")]
                t.icon.set_title(tv.active.then(|| clock(tv.remaining)));
            }
            // Windows tray icons cannot carry text, so the icon becomes the
            // number of minutes left.
            #[cfg(not(target_os = "macos"))]
            let key = if tv.active {
                let mins = tv.remaining.as_secs().div_ceil(60).max(1);
                IconKey::Timer(mins.to_string(), tv.phase, tv.running)
            } else {
                IconKey::Bars(playing)
            };
            #[cfg(target_os = "macos")]
            let key = IconKey::Bars(playing);
            if t.icon_key.as_ref() != Some(&key) {
                let icon = match &key {
                    IconKey::Bars(p) => bars_icon(*p),
                    IconKey::Timer(text, phase, running) => {
                        let rgba = ui::tray_timer_rgba(32, text, *phase, *running);
                        tray_icon::Icon::from_rgba(rgba, 32, 32).expect("valid icon")
                    }
                };
                #[cfg(target_os = "macos")]
                let _ = t.icon.set_icon_templated(Some(icon));
                #[cfg(not(target_os = "macos"))]
                let _ = t.icon.set_icon(Some(icon));
                t.icon_key = Some(key);
            }
        }
        if let Some(p) = &self.panel {
            p.canvas.window.request_redraw();
        }
        if let Some(o) = &self.overlay {
            o.canvas.window.request_redraw();
        }
    }

    fn settings_changed(&mut self, el: &ActiveEventLoop) {
        self.engine.apply(&self.settings);
        self.dirty = Some(Instant::now());
        self.sync_overlay(el);
        self.refresh();
    }

    fn save_now(&mut self) {
        if self.dirty.take().is_some() {
            self.settings.save();
        }
    }

    // ---- Pomodoro ---------------------------------------------------------

    fn timer_cmd(&mut self, el: &ActiveEventLoop, cmd: TimerCmd) {
        let now = Instant::now();
        let plan = self.settings.plan();
        let follow = self.settings.sound_on_focus;
        match cmd {
            TimerCmd::StartPause if self.timer.is_running() => {
                self.timer.pause(now);
                if follow {
                    self.engine.stop();
                }
            }
            TimerCmd::StartPause => match self.timer.start(now) {
                Some(ev) => self.on_timer_event(ev, false),
                None if follow && self.timer.phase == Phase::Focus => self.engine.start(),
                None => {}
            },
            TimerCmd::Skip => {
                if let Some(ev) = self.timer.skip(&plan, now) {
                    self.on_timer_event(ev, false);
                }
            }
            TimerCmd::Reset => {
                if self.timer.is_active() && follow {
                    self.engine.stop();
                }
                self.timer.reset();
            }
        }
        self.sync_overlay(el);
        self.refresh();
    }

    fn on_timer_event(&mut self, ev: Event, natural: bool) {
        let s = &self.settings;
        match ev {
            Event::Started(Phase::Focus) if s.sound_on_focus => self.engine.start(),
            Event::Started(Phase::Break | Phase::LongBreak) | Event::Finished if s.sound_off_at_end => {
                self.engine.stop()
            }
            _ => {}
        }
        if natural && s.chime {
            self.engine.chime();
        }
    }

    // ---- On-screen timer ----------------------------------------------------

    fn overlay_size(&self) -> LogicalSize<f64> {
        let k = self.settings.overlay_size as f64;
        LogicalSize::new(ui::OVERLAY_W as f64 * k, ui::OVERLAY_H as f64 * k)
    }

    /// Creates, updates or drops the overlay to match settings and timer state.
    fn sync_overlay(&mut self, el: &ActiveEventLoop) {
        let want = self.settings.overlay && self.timer.is_active();
        if !want {
            self.overlay = None;
            return;
        }
        let level = if self.settings.overlay_on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal };
        let size = self.overlay_size();
        if let Some(o) = &self.overlay {
            let w = &o.canvas.window;
            w.set_window_level(level);
            platform::set_opacity(w, self.settings.overlay_opacity);
            if w.inner_size().to_logical::<f64>(w.scale_factor()) != size {
                let _ = w.request_inner_size(size);
            }
            return;
        }

        let monitors: Vec<MonitorHandle> = el.available_monitors().collect();
        let fits = |(x, y): (i32, i32)| {
            monitors.iter().any(|m| {
                let (p, s) = (m.position(), m.size());
                x >= p.x - 20 && y >= p.y - 20 && x < p.x + s.width as i32 - 40 && y < p.y + s.height as i32 - 40
            })
        };
        let pos = self.settings.overlay_pos.filter(|&p| fits(p)).or_else(|| {
            let m = el.primary_monitor().or_else(|| monitors.first().cloned())?;
            let sf = m.scale_factor();
            let w = (size.width * sf) as i32;
            Some((m.position().x + (m.size().width as i32 - w) / 2, m.position().y + (40.0 * sf) as i32))
        });

        let mut attrs = Window::default_attributes()
            .with_title("Lull timer")
            .with_inner_size(size)
            .with_resizable(false)
            .with_decorations(false)
            .with_window_level(level)
            .with_active(false);
        if let Some((x, y)) = pos {
            attrs = attrs.with_position(PhysicalPosition::new(x, y));
        }
        #[cfg(windows)]
        {
            use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
            attrs = attrs.with_skip_taskbar(true).with_corner_preference(CornerPreference::Round);
        }
        if let Some(canvas) = Canvas::new(el, attrs) {
            platform::set_opacity(&canvas.window, self.settings.overlay_opacity);
            platform::show_on_all_spaces(&canvas.window);
            canvas.window.request_redraw();
            self.overlay = Some(Overlay { canvas, last_press: None });
        }
    }

    fn overlay_event(&mut self, el: &ActiveEventLoop, ev: WindowEvent) {
        let Some(o) = &mut self.overlay else { return };
        match ev {
            WindowEvent::RedrawRequested => {
                let tv = self.timer_view();
                let zoom = self.settings.overlay_size;
                if let Some(o) = &mut self.overlay {
                    o.canvas.present(zoom, |p| ui::draw_overlay(p, &tv));
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => o.canvas.window.request_redraw(),
            WindowEvent::MouseInput { state: ElementState::Pressed, button: winit::event::MouseButton::Left, .. } => {
                // Double-click pauses / resumes; a press-and-drag moves it.
                let now = Instant::now();
                if o.last_press.is_some_and(|t| now - t < Duration::from_millis(350)) {
                    o.last_press = None;
                    self.timer_cmd(el, TimerCmd::StartPause);
                } else {
                    o.last_press = Some(now);
                    let _ = o.canvas.window.drag_window();
                }
            }
            WindowEvent::Moved(pos) => {
                self.settings.overlay_pos = Some((pos.x, pos.y));
                self.dirty = Some(Instant::now());
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let step = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y.signum() * 0.1,
                    MouseScrollDelta::PixelDelta(d) => (d.y as f32 / 400.0).clamp(-0.1, 0.1),
                };
                let (lo, hi) = OVERLAY_SIZE_RANGE;
                let size = ((self.settings.overlay_size + step).clamp(lo, hi) * 100.0).round() / 100.0;
                if size != self.settings.overlay_size {
                    self.settings.overlay_size = size;
                    self.settings_changed(el);
                }
            }
            _ => {}
        }
    }

    // ---- Panel ------------------------------------------------------------

    fn toggle_panel(&mut self, el: &ActiveEventLoop, anchor: Option<tray_icon::Rect>) {
        if self.panel.is_some() {
            self.hide_panel();
        } else if self.last_hide.is_some_and(|t| t.elapsed() < Duration::from_millis(350)) {
            // The click that would reopen it is the same one that just closed it.
        } else {
            self.show_panel(el, anchor);
        }
    }

    fn hide_panel(&mut self) {
        if let Some(p) = self.panel.take() {
            self.tab = p.ui.tab;
            self.last_hide = Some(Instant::now());
        }
        self.save_now();
    }

    fn show_panel(&mut self, el: &ActiveEventLoop, anchor: Option<tray_icon::Rect>) {
        let monitor = anchor
            .and_then(|a| {
                el.available_monitors().find(|m| {
                    let (p, s) = (m.position(), m.size());
                    let (x, y) = (a.position.x as i32, a.position.y as i32);
                    x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
                })
            })
            .or_else(|| el.primary_monitor())
            .or_else(|| el.available_monitors().next());
        let scale = monitor.as_ref().map_or(1.0, |m| m.scale_factor());
        let (pw, ph) = ((ui::W as f64 * scale) as i32, (ui::H as f64 * scale) as i32);
        let gap = (8.0 * scale) as i32;

        let mut pos = None;
        if let Some(m) = &monitor {
            let (mp, ms) = (m.position(), m.size());
            let (mw, mh) = (ms.width as i32, ms.height as i32);
            let (x, y) = match anchor {
                Some(a) => {
                    let (ax, ay) = (a.position.x as i32, a.position.y as i32);
                    let (aw, ah) = (a.size.width as i32, a.size.height as i32);
                    let x = ax + aw / 2 - pw / 2;
                    let y = if ay - mp.y < mh / 2 { ay + ah + gap } else { ay - ph - gap };
                    (x, y)
                }
                // Hotkey: the corner where the tray lives on each OS.
                None if cfg!(target_os = "macos") => (mp.x + mw - pw - 2 * gap, mp.y + 4 * gap),
                None => (mp.x + mw - pw - 2 * gap, mp.y + mh - ph - 7 * gap),
            };
            let x = x.clamp(mp.x + gap, (mp.x + mw - pw - gap).max(mp.x + gap));
            let y = y.clamp(mp.y + gap, (mp.y + mh - ph - gap).max(mp.y + gap));
            pos = Some(PhysicalPosition::new(x, y));
        }

        let mut attrs = Window::default_attributes()
            .with_title("Lull")
            .with_inner_size(LogicalSize::new(ui::W as f64, ui::H as f64))
            .with_resizable(false)
            .with_decorations(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false);
        if let Some(p) = pos {
            attrs = attrs.with_position(p);
        }
        #[cfg(windows)]
        {
            use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
            attrs = attrs.with_skip_taskbar(true).with_corner_preference(CornerPreference::Round);
        }

        let Some(canvas) = Canvas::new(el, attrs) else { return };
        canvas.window.set_visible(true);
        canvas.window.focus_window();
        canvas.window.request_redraw();
        self.panel = Some(Panel { canvas, ui: Ui::new(self.tab), cursor: (-1.0, -1.0), had_focus: false });
    }

    fn handle_response(&mut self, el: &ActiveEventLoop, r: Response) {
        match r {
            Response::None => {}
            Response::Redraw => {
                if let Some(p) = &self.panel {
                    p.canvas.window.request_redraw();
                }
            }
            Response::Changed => self.settings_changed(el),
            Response::TogglePlay => self.toggle_play(),
            Response::Timer(cmd) => self.timer_cmd(el, cmd),
        }
    }

    fn panel_event(&mut self, el: &ActiveEventLoop, ev: WindowEvent) {
        let Some(p) = &mut self.panel else { return };
        let scale = p.canvas.window.scale_factor() as f32;
        match ev {
            WindowEvent::RedrawRequested => {
                let playing = self.engine.is_playing();
                let tv = self.timer_view();
                if let Some(p) = &mut self.panel {
                    let (ui, s, hint) = (&p.ui, &self.settings, &self.hint);
                    p.canvas.present(1.0, |pt| ui.draw(pt, s, playing, &tv, hint));
                }
            }
            WindowEvent::Focused(true) => p.had_focus = true,
            WindowEvent::Focused(false) if p.had_focus => self.hide_panel(),
            WindowEvent::CloseRequested => self.hide_panel(),
            WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Resized(_) => p.canvas.window.request_redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                p.cursor = (position.x as f32 / scale, position.y as f32 / scale);
                let r = p.ui.moved(&mut self.settings, p.cursor.0, p.cursor.1);
                self.handle_response(el, r);
            }
            WindowEvent::CursorLeft { .. } => {
                p.cursor = (-1.0, -1.0);
                let r = p.ui.moved(&mut self.settings, -1.0, -1.0);
                self.handle_response(el, r);
            }
            WindowEvent::MouseInput { state, button: winit::event::MouseButton::Left, .. } => {
                if state == ElementState::Pressed {
                    let r = p.ui.press(&mut self.settings, p.cursor.0, p.cursor.1);
                    self.handle_response(el, r);
                } else {
                    p.ui.release();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(d) => d.y as f32 / 40.0,
                };
                let r = p.ui.wheel(&mut self.settings, p.cursor.0, p.cursor.1, notches);
                self.handle_response(el, r);
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Escape) => self.hide_panel(),
                Key::Named(NamedKey::Space) => self.toggle_play(),
                Key::Named(NamedKey::Tab) => {
                    p.ui.tab = if p.ui.tab == Tab::Sound { Tab::Timer } else { Tab::Sound };
                    p.canvas.window.request_redraw();
                }
                Key::Character(c) if c.eq_ignore_ascii_case("q") => self.quit(el),
                _ => {}
            },
            _ => {}
        }
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        self.hide_panel();
        self.save_now();
        el.exit();
    }
}

fn bars_icon(playing: bool) -> tray_icon::Icon {
    let size = 32;
    let rgba = ui::tray_icon_rgba(size, playing, cfg!(target_os = "macos"));
    tray_icon::Icon::from_rgba(rgba, size, size).expect("valid icon")
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _el: &ActiveEventLoop) {
        // Tray icons must be created once the event loop is running (macOS).
        if self.tray.is_none() {
            self.create_tray();
            if cfg!(windows) && self.tray.is_some() && !self.settings.tray_pinned {
                self.pin_retry = Some((Instant::now() + Duration::from_millis(500), 0));
            }
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Hotkey(e) if e.state == HotKeyState::Pressed => {
                if e.id == self.hk_play {
                    self.toggle_play();
                } else if e.id == self.hk_timer {
                    self.timer_cmd(el, TimerCmd::StartPause);
                } else if e.id == self.hk_panel {
                    self.toggle_panel(el, None);
                }
            }
            UserEvent::Hotkey(_) => {}
            UserEvent::Tray(TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }) => self.toggle_panel(el, Some(rect)),
            UserEvent::Tray(_) => {}
            UserEvent::Menu(e) => {
                let Some(t) = &self.tray else { return };
                if e.id == t.show_id {
                    let rect = t.icon.rect();
                    if self.panel.is_none() {
                        self.show_panel(el, rect);
                    }
                } else if e.id == t.play_id {
                    self.toggle_play();
                } else if e.id == t.timer_id {
                    self.timer_cmd(el, TimerCmd::StartPause);
                } else if e.id == t.reset_id {
                    self.timer_cmd(el, TimerCmd::Reset);
                } else if e.id == t.quit_id {
                    self.quit(el);
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, ev: WindowEvent) {
        if self.panel.as_ref().is_some_and(|p| p.canvas.window.id() == id) {
            self.panel_event(el, ev);
        } else if self.overlay.as_ref().is_some_and(|o| o.canvas.window.id() == id) {
            self.overlay_event(el, ev);
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let plan = self.settings.plan();
        let mut transitioned = false;
        while let Some(ev) = self.timer.tick(&plan, now) {
            self.on_timer_event(ev, true);
            transitioned = true;
        }
        if transitioned {
            self.sync_overlay(el);
        }

        // Changes made outside the panel (dragging the overlay) are saved
        // shortly after they stop.
        let mut save_at = None;
        if let Some(t) = self.dirty.filter(|_| self.panel.is_none()) {
            if now >= t + SAVE_DELAY {
                self.save_now();
            } else {
                save_at = Some(t + SAVE_DELAY);
            }
        }

        self.try_pin_tray(now);
        let engine_at = self.engine.poll();
        self.refresh();
        let pin_at = self.pin_retry.map(|(t, _)| t);
        let wake = [engine_at, self.timer.next_tick(&plan, now), save_at, pin_at].into_iter().flatten().min();
        el.set_control_flow(match wake {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        self.save_now();
    }
}

fn forward<T: 'static>(
    proxy: &EventLoopProxy<UserEvent>,
    wrap: fn(T) -> UserEvent,
) -> impl Fn(T) + Send + Sync + 'static {
    let proxy = std::sync::Mutex::new(proxy.clone());
    move |e| {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(wrap(e));
        }
    }
}

fn main() {
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder.build().expect("event loop");
    let proxy = event_loop.create_proxy();
    GlobalHotKeyEvent::set_event_handler(Some(forward(&proxy, UserEvent::Hotkey)));
    TrayIconEvent::set_event_handler(Some(forward(&proxy, UserEvent::Tray)));
    MenuEvent::set_event_handler(Some(forward(&proxy, UserEvent::Menu)));

    let mut app = App::new();
    event_loop.set_control_flow(ControlFlow::Wait);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("lull: {e}");
    }
}
