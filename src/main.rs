mod catalog;
mod config;

use iced::{
    Border, Color, Element, Fill, Task, Theme, keyboard,
    widget::{button, column, container, image, operation, row, text, text_input},
};
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use window_vibrancy::{
    LiquidGlassOptions, NSGlassEffectViewStyle, NSVisualEffectMaterial, NSVisualEffectState,
};

fn main() -> iced::Result {
    iced::application(Launcher::new, Launcher::update, Launcher::view)
        .title("Launcher")
        .decorations(false)
        .transparent(true)
        .style(|_, theme| iced::theme::Style {
            background_color: iced::Color::TRANSPARENT,
            text_color: theme.palette().text,
        })
        .theme(|state: &Launcher| state.config.theme(state.appearance))
        .window_size((680.0, 560.0))
        .resizable(false)
        .centered()
        .subscription(|state| {
            iced::Subscription::batch([
                if state.animating() {
                    iced::window::frames().map(Message::Tick)
                } else {
                    iced::Subscription::none()
                },
                iced::system::theme_changes().map(Message::Appearance),
                iced::event::listen_with(|event, status, _| match event {
                    iced::Event::Keyboard(event) if forward_key(&event, status) => {
                        Some(Message::Key(event))
                    }
                    _ => None,
                }),
            ])
        })
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    Appearance(iced::theme::Mode),
    Glass(bool),
    Query(String),
    Icons(Vec<Option<image::Handle>>),
    Key(keyboard::Event),
    Launch(usize),
    Tick(Instant),
}

fn forward_key(event: &keyboard::Event, status: iced::event::Status) -> bool {
    matches!(
        event,
        keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }
    ) || status == iced::event::Status::Ignored
}

// Liquid Glass on macOS 26+, classic blur before that; false means no native backdrop.
fn apply_glass(window: &dyn iced::window::Window, radius: f64) -> bool {
    use raw_window_handle::{AppKitWindowHandle, RawWindowHandle, WindowHandle};
    let Ok(RawWindowHandle::AppKit(handle)) = window.window_handle().map(|handle| handle.as_raw())
    else {
        return false;
    };
    // The glass must sit behind iced's Metal layer, i.e. beside winit's view rather than inside
    // it. winit casts contentView back to its own type, so leave that view where it is and put
    // the glass into the window frame view underneath it.
    let view = unsafe { handle.ns_view.cast::<objc2::runtime::AnyObject>().as_ref() };
    // The system shadow and outline use the square window shape and show as dark corners
    // outside the rounded glass; the glass draws its own edge.
    unsafe {
        let ns_window: *mut objc2::runtime::AnyObject = objc2::msg_send![view, window];
        let _: () = objc2::msg_send![ns_window, setHasShadow: false];
        // Hidden until the fade-in starts, so the window never flashes opaque.
        let _: () = objc2::msg_send![ns_window, setAlphaValue: 0.0_f64];
    }
    let frame: *mut objc2::runtime::AnyObject = unsafe { objc2::msg_send![view, superview] };
    let Some(frame) = std::ptr::NonNull::new(frame) else {
        return false;
    };
    let frame = unsafe {
        WindowHandle::borrow_raw(RawWindowHandle::AppKit(AppKitWindowHandle::new(
            frame.cast(),
        )))
    };
    window_vibrancy::apply_liquid_glass(
        frame,
        LiquidGlassOptions::new(NSGlassEffectViewStyle::Regular).radius(radius),
    )
    .or_else(|_| {
        window_vibrancy::apply_vibrancy(
            frame,
            NSVisualEffectMaterial::HudWindow,
            Some(NSVisualEffectState::Active),
            Some(radius),
        )
    })
    .inspect_err(|error| eprintln!("window glass unavailable: {error}"))
    .is_ok()
}

fn set_alpha(window: &dyn iced::window::Window, alpha: f64) {
    use raw_window_handle::RawWindowHandle;
    if let Ok(RawWindowHandle::AppKit(handle)) =
        window.window_handle().map(|handle| handle.as_raw())
    {
        unsafe {
            let view = handle.ns_view.cast::<objc2::runtime::AnyObject>().as_ref();
            let ns_window: *mut objc2::runtime::AnyObject = objc2::msg_send![view, window];
            let _: () = objc2::msg_send![ns_window, setAlphaValue: alpha];
        }
    }
}

const SELECT_FADE: Duration = Duration::from_millis(150);

fn dim(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

struct Launcher {
    appearance: iced::theme::Mode,
    glass: bool,
    config: config::Config,
    apps: Vec<catalog::App>,
    icons: Vec<Option<image::Handle>>,
    matches: Vec<usize>,
    query: String,
    selected: usize,
    error: Option<String>,
    fade: iced::animation::Animation<bool>,
    closing: bool,
    now: Instant,
    selected_at: Instant,
}

impl Launcher {
    fn new() -> (Self, Task<Message>) {
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        // shortcut: scan once at startup; add background refresh when persistent activation is introduced.
        let (apps, errors) = catalog::discover(roots);
        for error in &errors {
            eprintln!("{error}");
        }
        let matches = catalog::search(&apps, "");
        let paths: Vec<_> = apps.iter().map(|app| app.path.clone()).collect();
        let cache = config::xdg_dir(
            std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
            ".cache",
        )
        .unwrap_or_else(std::env::temp_dir)
        .join("launcher/icons");
        let icons = vec![None; apps.len()];
        let load_icons = Task::perform(
            async move {
                paths
                    .iter()
                    .map(|path| catalog::load_icon(path, &cache))
                    .collect()
            },
            Message::Icons,
        );
        let config = config::Config::load();
        let radius = config.radius() as f64;
        let glass = iced::window::oldest()
            .and_then(move |id| iced::window::run(id, move |window| apply_glass(window, radius)))
            .map(Message::Glass);
        (
            Self {
                config,
                appearance: iced::theme::Mode::Dark,
                glass: false,
                apps,
                icons,
                matches,
                query: String::new(),
                selected: 0,
                error: (!errors.is_empty())
                    .then(|| "Some folders could not be read; see terminal output.".into()),
                fade: iced::animation::Animation::new(false).quick(),
                closing: false,
                now: Instant::now(),
                selected_at: Instant::now(),
            },
            Task::batch([
                operation::focus("search"),
                load_icons,
                glass,
                iced::system::theme().map(Message::Appearance),
            ]),
        )
    }

    fn animating(&self) -> bool {
        self.fade.is_animating(self.now) || self.now < self.selected_at + SELECT_FADE
    }

    fn select(&mut self, index: usize) {
        if index != self.selected {
            self.selected = index;
            self.selected_at = Instant::now();
            self.now = self.selected_at;
        }
    }

    fn close(&mut self) -> Task<Message> {
        if !self.closing {
            self.closing = true;
            self.now = Instant::now();
            self.fade.go_mut(false, self.now);
        }
        Task::none()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Appearance(mode) => self.appearance = mode,
            Message::Glass(glass) => {
                self.glass = glass;
                self.fade.go_mut(true, Instant::now());
            }
            Message::Tick(now) => {
                self.now = now;
                if self.closing && !self.fade.is_animating(now) {
                    return iced::exit();
                }
                let alpha = self.fade.interpolate(0.0, 1.0, now) as f64;
                return iced::window::oldest()
                    .and_then(move |id| iced::window::run(id, move |w| set_alpha(w, alpha)))
                    .discard();
            }
            Message::Icons(icons) => self.icons = icons,
            Message::Query(query) => {
                self.query = query;
                self.matches = catalog::search(&self.apps, &self.query);
                self.selected = 0;
            }
            Message::Key(keyboard::Event::KeyPressed { key, .. }) => match key.as_ref() {
                keyboard::Key::Named(keyboard::key::Named::Escape) => return self.close(),
                keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                    self.select((self.selected + 1).min(self.matches.len().saturating_sub(1)));
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                    self.select(self.selected.saturating_sub(1));
                }
                keyboard::Key::Named(keyboard::key::Named::Enter) => {
                    if let Some(&index) = self.matches.get(self.selected) {
                        return self.update(Message::Launch(index));
                    }
                }
                _ => {}
            },
            Message::Launch(index) => {
                if let Some(app) = self.apps.get(index) {
                    match Command::new("/usr/bin/open").arg(&app.path).output() {
                        Ok(output) if output.status.success() => return self.close(),
                        Ok(output) => {
                            self.error = Some(format!(
                                "Could not open {}: {}",
                                app.name,
                                String::from_utf8_lossy(&output.stderr).trim()
                            ))
                        }
                        Err(error) => {
                            self.error = Some(format!("Could not open {}: {error}", app.name))
                        }
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let mut content = column![
            text_input("Search applications…", &self.query)
                .id("search")
                .on_input(Message::Query)
                .padding(14)
                .size(24)
                .style(|theme: &Theme, status| {
                    let palette = theme.palette();
                    let focused = matches!(status, text_input::Status::Focused { .. });
                    text_input::Style {
                        background: dim(palette.text, 0.06).into(),
                        border: Border {
                            radius: 12.0.into(),
                            width: 1.0,
                            color: if focused {
                                dim(palette.primary, 0.7)
                            } else {
                                dim(palette.text, 0.12)
                            },
                        },
                        placeholder: dim(palette.text, 0.4),
                        selection: dim(palette.primary, 0.35),
                        ..text_input::default(theme, status)
                    }
                }),
            text(format!(
                "{} applications · ↑ ↓ select · Enter open · Esc quit",
                self.matches.len()
            ))
            .size(13)
            .style(|theme: &Theme| text::Style {
                color: Some(dim(theme.palette().text, 0.5)),
            }),
        ]
        .spacing(10);
        let start = self.selected.saturating_sub(5);
        for (position, &index) in self.matches.iter().enumerate().skip(start).take(6) {
            let app = &self.apps[index];
            let icon: Element<'_, Message> = match &self.icons[index] {
                Some(handle) => image(handle.clone()).width(36).height(36).into(),
                None => container(text("▧").size(28))
                    .center_x(36)
                    .center_y(36)
                    .into(),
            };
            content = content.push(
                button(
                    row![
                        icon,
                        column![
                            text(&app.name).size(20),
                            text(app.path.to_string_lossy().into_owned())
                                .size(11)
                                .style(|theme: &Theme| text::Style {
                                    color: Some(dim(theme.palette().text, 0.5)),
                                })
                        ]
                    ]
                    .spacing(12)
                    .align_y(iced::Center),
                )
                .padding(10)
                .width(Fill)
                .style(move |theme: &Theme, status| {
                    let palette = theme.palette();
                    let (background, border) = if position == self.selected {
                        let t = (self
                            .now
                            .saturating_duration_since(self.selected_at)
                            .as_secs_f32()
                            / SELECT_FADE.as_secs_f32())
                        .min(1.0);
                        (
                            dim(palette.primary, 0.28 * t),
                            dim(palette.primary, 0.55 * t),
                        )
                    } else if status == button::Status::Hovered {
                        (dim(palette.text, 0.06), Color::TRANSPARENT)
                    } else {
                        (Color::TRANSPARENT, Color::TRANSPARENT)
                    };
                    button::Style {
                        background: Some(background.into()),
                        text_color: palette.text,
                        border: Border {
                            radius: 12.0.into(),
                            width: 1.0,
                            color: border,
                        },
                        ..Default::default()
                    }
                })
                .on_press(Message::Launch(index)),
            );
        }
        if self.matches.is_empty() {
            content = content.push(text("No applications found"));
        }
        if let Some(error) = &self.error {
            content = content.push(text(error).size(12));
        }
        container(content)
            .padding(20)
            .width(Fill)
            .height(Fill)
            .style(|theme: &Theme| {
                let palette = theme.palette();
                container::Style {
                    // Tint over the native glass; opaque when there is no backdrop to show.
                    background: Some(
                        dim(palette.background, if self.glass { 0.35 } else { 1.0 }).into(),
                    ),
                    border: Border {
                        radius: self.config.radius().into(),
                        width: 1.0,
                        color: dim(palette.text, 0.12),
                    },
                    ..Default::default()
                }
            })
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_is_forwarded_even_when_input_captures_it() {
        let event = keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        };
        assert!(forward_key(&event, iced::event::Status::Captured));
        assert!(forward_key(&event, iced::event::Status::Ignored));
        let other = keyboard::Event::ModifiersChanged(keyboard::Modifiers::empty());
        assert!(!forward_key(&other, iced::event::Status::Captured));
    }
}
