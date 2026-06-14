//! Reusable Wayland layer-shell overlay shell shared by the overlay apps.

mod paths;
mod settings;
mod trace;
pub use paths::{cache_dir, config_dir};
pub use settings::{FontSize, Position, SavedConfig, load_config, save};
pub use trace::{debug, debug_enabled};

use iced::Element;
use iced::Subscription;
use iced::Task;
use iced_layershell::actions::LayerShellCustomActionWithId;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::LayerShellSettings;
use std::os::unix::io::AsRawFd;

/// Exits the process if another instance of `app` is already running.
/// Uses a per-app `flock(2)` lock under `$XDG_RUNTIME_DIR` (fallback `/tmp`).
pub fn ensure_single_instance(app: &str) {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = std::path::Path::new(&dir).join(format!("{app}.lock"));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap_or_else(|e| {
            eprintln!("[overlay] não foi possível abrir lock file: {e}");
            std::process::exit(1);
        });
    // LOCK_EX | LOCK_NB — exclusive, non-blocking
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        eprintln!("[overlay] {app} já está em execução");
        std::process::exit(0);
    }
    // Keep fd open for the process lifetime; kernel releases the lock on exit.
    std::mem::forget(file);
}

/// The interface every overlay app must implement.
///
/// `run::<A>()` wires the app into `iced_layershell` with the shared defaults
/// (transparent background, bottom-anchored, Layer::Top, 80px tall, etc.).
pub trait OverlayApp: Default + Sized + 'static {
    /// The app-specific message type.  Must carry the `TryInto` conversion
    /// that `#[to_layer_message]` generates.
    type Message: Clone
        + std::fmt::Debug
        + Send
        + 'static
        + TryInto<LayerShellCustomActionWithId, Error = Self::Message>;

    /// The Wayland namespace / app-id for the layer shell surface.
    fn namespace() -> &'static str;

    /// Handle a message and (optionally) return a follow-up `Task`.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message>;

    /// Render the current state into a widget tree.
    fn view(&self) -> Element<'_, Self::Message>;

    /// Subscriptions to run while the app is alive.
    fn subscription(&self) -> Subscription<Self::Message>;
}

/// Wires an [`OverlayApp`] into `iced_layershell` and blocks until exit.
///
/// Calls [`ensure_single_instance`], then builds the standard layer-shell
/// application with a transparent background, white text, bottom-anchored
/// 80px panel.
pub fn run<A: OverlayApp>() -> iced_layershell::Result {
    ensure_single_instance(A::namespace());

    iced_layershell::application(
        A::default,
        A::namespace(),
        update_wrapper::<A>,
        view_wrapper::<A>,
    )
    .subscription(|state: &A| state.subscription())
    .style(|_state, _theme| iced::theme::Style {
        background_color: iced::Color::TRANSPARENT,
        text_color: iced::Color::WHITE,
    })
    .layer_settings(LayerShellSettings {
        anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
        layer: Layer::Top,
        exclusive_zone: 0,
        size: Some((0, 80)),
        margin: (0, 0, 40, 0),
        keyboard_interactivity: KeyboardInteractivity::None,
        events_transparent: true,
        ..Default::default()
    })
    .run()
}

// Free functions with the exact signature iced_layershell::application expects,
// forwarding to the trait methods.

fn update_wrapper<A: OverlayApp>(state: &mut A, message: A::Message) -> Task<A::Message> {
    state.update(message)
}

fn view_wrapper<A: OverlayApp>(state: &A) -> Element<'_, A::Message> {
    state.view()
}
