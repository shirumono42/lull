#![cfg_attr(not(test), windows_subsystem = "windows")]

mod audio;
mod settings;
mod ui;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId, WindowLevel};

use audio::Engine;
use settings::Settings;
use ui::{Painter, Response, Ui};

#[cfg(target_os = "macos")]
const HINT: &str = "⌃⌥N  play / stop      ⌃⌥M  panel";
#[cfg(not(target_os = "macos"))]
const HINT: &str = "Ctrl+Alt+N  play / stop      Ctrl+Alt+M  panel";

enum UserEvent {
    Hotkey(GlobalHotKeyEvent),
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

/// The popup. It only exists while visible; hiding drops it entirely.
struct Panel {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    painter: Painter,
    ui: Ui,
    cursor: (f32, f32),
    had_focus: bool,
}

struct TrayParts {
    icon: TrayIcon,
    play_item: MenuItem,
    show_id: tray_icon::menu::MenuId,
    play_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
}

struct App {
    settings: Settings,
    dirty: bool,
    engine: Engine,
    panel: Option<Panel>,
    tray: Option<TrayParts>,
    _hotkeys: Option<GlobalHotKeyManager>,
    hk_play: u32,
    hk_panel: u32,
    last_hide: Option<Instant>,
}

impl App {
    fn new() -> Self {
        let settings = Settings::load();
        let mods = Some(Modifiers::CONTROL | Modifiers::ALT);
        let play = HotKey::new(mods, Code::KeyN);
        let panel = HotKey::new(mods, Code::KeyM);
        let hotkeys = GlobalHotKeyManager::new().ok();
        if let Some(m) = &hotkeys {
            for hk in [play, panel] {
                if let Err(e) = m.register(hk) {
                    eprintln!("lull: could not register hotkey: {e}");
                }
            }
        }
        App {
            engine: Engine::new(&settings),
            settings,
            dirty: false,
            panel: None,
            tray: None,
            _hotkeys: hotkeys,
            hk_play: play.id(),
            hk_panel: panel.id(),
            last_hide: None,
        }
    }

    fn create_tray(&mut self) {
        let show = MenuItem::new("Show panel", true, None);
        let play = MenuItem::new("Play", true, None);
        let quit = MenuItem::new("Quit Lull", true, None);
        let menu = Menu::new();
        let _ = menu.append_items(&[&show, &play, &PredefinedMenuItem::separator(), &quit]);
        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Lull");
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(tray_icon(false));
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(tray_icon(false));
        let icon = builder.build();
        match icon {
            Ok(icon) => {
                self.tray = Some(TrayParts {
                    icon,
                    show_id: show.id().clone(),
                    play_id: play.id().clone(),
                    quit_id: quit.id().clone(),
                    play_item: play,
                })
            }
            Err(e) => eprintln!("lull: tray icon unavailable: {e}"),
        }
    }

    fn toggle_play(&mut self) {
        self.engine.toggle();
        self.refresh_state();
    }

    /// Syncs tray + panel with the playing state.
    fn refresh_state(&mut self) {
        let playing = self.engine.is_playing();
        if let Some(t) = &self.tray {
            t.play_item.set_text(if playing { "Stop" } else { "Play" });
            let _ = t.icon.set_tooltip(Some(if playing { "Lull — playing" } else { "Lull" }));
            #[cfg(target_os = "macos")]
            let _ = t.icon.set_icon_templated(Some(tray_icon(playing)));
            #[cfg(not(target_os = "macos"))]
            let _ = t.icon.set_icon(Some(tray_icon(playing)));
        }
        if let Some(p) = &self.panel {
            p.window.request_redraw();
        }
    }

    fn settings_changed(&mut self) {
        self.engine.apply(&self.settings);
        self.dirty = true;
        if let Some(p) = &self.panel {
            p.window.request_redraw();
        }
    }

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
        if self.panel.take().is_some() {
            self.last_hide = Some(Instant::now());
        }
        if self.dirty {
            self.settings.save();
            self.dirty = false;
        }
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
            let x = x.clamp(mp.x + gap, mp.x + mw - pw - gap);
            let y = y.clamp(mp.y + gap, mp.y + mh - ph - gap);
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
            attrs = attrs
                .with_skip_taskbar(true)
                .with_corner_preference(CornerPreference::Round);
        }

        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => return eprintln!("lull: window error: {e}"),
        };
        let surface = softbuffer::Context::new(window.clone())
            .and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone()));
        let surface = match surface {
            Ok(s) => s,
            Err(e) => return eprintln!("lull: surface error: {e}"),
        };
        let painter = Painter::new(window.scale_factor() as f32);
        window.set_visible(true);
        window.focus_window();
        window.request_redraw();
        self.panel = Some(Panel {
            window,
            surface,
            painter,
            ui: Ui::default(),
            cursor: (-1.0, -1.0),
            had_focus: false,
        });
    }

    fn redraw(&mut self) {
        let playing = self.engine.is_playing();
        let Some(p) = &mut self.panel else { return };
        let size = p.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if p.surface.resize(w, h).is_err() {
            return;
        }
        p.painter.resize(size.width, size.height, p.window.scale_factor() as f32);
        p.ui.draw(&mut p.painter, &self.settings, playing, HINT);
        if let Ok(mut buf) = p.surface.buffer_mut() {
            p.painter.blit(&mut buf);
            let _ = buf.present();
        }
    }

    fn handle_response(&mut self, r: Response) {
        match r {
            Response::None => {}
            Response::Changed => self.settings_changed(),
            Response::TogglePlay => self.toggle_play(),
        }
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        self.hide_panel();
        el.exit();
    }
}

fn tray_icon(playing: bool) -> tray_icon::Icon {
    let size = 32;
    let rgba = ui::tray_icon_rgba(size, playing, cfg!(target_os = "macos"));
    tray_icon::Icon::from_rgba(rgba, size, size).expect("valid icon")
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _el: &ActiveEventLoop) {
        // Tray icons must be created once the event loop is running (macOS).
        if self.tray.is_none() {
            self.create_tray();
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Hotkey(e) if e.state == HotKeyState::Pressed => {
                if e.id == self.hk_play {
                    self.toggle_play();
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
                } else if e.id == t.quit_id {
                    self.quit(el);
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        let Some(p) = &mut self.panel else { return };
        let scale = p.window.scale_factor() as f32;
        match ev {
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Focused(true) => p.had_focus = true,
            WindowEvent::Focused(false) if p.had_focus => self.hide_panel(),
            WindowEvent::CloseRequested => self.hide_panel(),
            WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Resized(_) => p.window.request_redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                p.cursor = (position.x as f32 / scale, position.y as f32 / scale);
                let r = p.ui.moved(&mut self.settings, p.cursor.0, p.cursor.1);
                if r == Response::Changed && p.ui.dragging() {
                    self.settings_changed();
                } else if r == Response::Changed {
                    p.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                p.cursor = (-1.0, -1.0);
                p.ui.moved(&mut self.settings, -1.0, -1.0);
                p.window.request_redraw();
            }
            WindowEvent::MouseInput { state, button: winit::event::MouseButton::Left, .. } => {
                if state == ElementState::Pressed {
                    let r = p.ui.press(&mut self.settings, p.cursor.0, p.cursor.1);
                    self.handle_response(r);
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
                self.handle_response(r);
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Escape) => self.hide_panel(),
                Key::Named(NamedKey::Space) => self.toggle_play(),
                Key::Character(c) if c.eq_ignore_ascii_case("q") => self.quit(el),
                _ => {}
            },
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let was_playing = self.engine.is_playing();
        match self.engine.poll() {
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
        if was_playing != self.engine.is_playing() {
            self.refresh_state();
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        if self.dirty {
            self.settings.save();
        }
    }
}

fn forward<T: 'static>(proxy: &EventLoopProxy<UserEvent>, wrap: fn(T) -> UserEvent) -> impl Fn(T) + Send + Sync + 'static {
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
