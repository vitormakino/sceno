//! Reusable Wayland layer-shell overlay shell shared by the overlay apps.

mod paths;
mod settings;
mod stack;
mod trace;
pub use paths::{cache_dir, config_dir, data_dir};
pub use settings::{FontSize, load_config, save};
pub use stack::{Margin, margin_for_slot};
pub use trace::{debug, debug_enabled};

use futures::stream::{self, BoxStream, StreamExt};
use iced::Element;
use iced::Subscription;
use iced::Task;
use iced_layershell::actions::LayerShellCustomActionWithId;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::LayerShellSettings;
use std::os::unix::io::AsRawFd;
use std::sync::{Mutex, OnceLock};

/// Hands the slot claimed synchronously in [`run`] to the reflow subscription, which takes it
/// exactly once. Safe as a process-global because `ensure_single_instance` guarantees one
/// process per app id.
static STACK_GUARD: OnceLock<Mutex<Option<stack::SlotGuard>>> = OnceLock::new();

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

    /// Construct the app's layer-shell margin-change message. Each app implements this as
    /// `Message::MarginChange(margin)` (the variant `#[to_layer_message]` generates), letting
    /// the shared auto-stacker reposition the surface generically.
    fn margin_changed(margin: Margin) -> Self::Message;

    /// Handle a message and (optionally) return a follow-up `Task`.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message>;

    /// Render the current state into a widget tree.
    fn view(&self) -> Element<'_, Self::Message>;

    /// Subscriptions to run while the app is alive.
    fn subscription(&self) -> Subscription<Self::Message>;

    // ── Surface geometry (defaults preserve the legacy 80px bottom strip) ──────

    /// Surface height in px. Default 80 (the thin caption/meter strip).
    fn surface_height() -> u32 {
        stack::SURFACE_HEIGHT as u32
    }

    /// Layer-shell anchor edges. Default bottom + left + right (full-width strip).
    fn anchor() -> Anchor {
        Anchor::Bottom | Anchor::Left | Anchor::Right
    }

    /// Whether pointer events pass through the surface. Default `true` (click-through).
    fn events_transparent() -> bool {
        true
    }

    /// Whether this app joins the shared bottom-edge auto-stacking. Default `true`.
    /// Apps that own a large, fixed-geometry surface (e.g. a karaoke panel) return
    /// `false` so they don't claim a strip slot or reflow with the strips.
    fn stacks() -> bool {
        true
    }

    /// Fixed surface margin, used only when [`Self::stacks`] is `false`.
    /// Defaults to the bottom-most slot's margin.
    fn initial_margin() -> Margin {
        margin_for_slot(0)
    }
}

/// Wires an [`OverlayApp`] into `iced_layershell` and blocks until exit.
///
/// Calls [`ensure_single_instance`], then builds the standard layer-shell
/// application with a transparent background, white text, bottom-anchored
/// 80px panel.
pub fn run<A: OverlayApp>() -> iced_layershell::Result {
    ensure_single_instance(A::namespace());

    // Stacking apps claim a slot synchronously so the surface is born at the right margin
    // (no reposition flash); the guard is handed to the reflow subscription for live
    // compaction. Non-stacking apps own a fixed geometry and skip the slot pool entirely.
    let stacks = A::stacks();
    let initial_margin = if stacks {
        let guard = stack::claim_lowest();
        let margin = guard.margin();
        debug(
            "stack",
            format_args!(
                "{} claimed slot {} margin {:?}",
                A::namespace(),
                guard.index(),
                margin
            ),
        );
        let _ = STACK_GUARD.set(Mutex::new(Some(guard)));
        margin
    } else {
        A::initial_margin()
    };

    iced_layershell::application(
        A::default,
        A::namespace(),
        update_wrapper::<A>,
        view_wrapper::<A>,
    )
    .subscription(move |state: &A| {
        if stacks {
            Subscription::batch([state.subscription(), stacking_subscription::<A>()])
        } else {
            state.subscription()
        }
    })
    .style(|_state, _theme| iced::theme::Style {
        background_color: iced::Color::TRANSPARENT,
        text_color: iced::Color::WHITE,
    })
    .layer_settings(LayerShellSettings {
        anchor: A::anchor(),
        layer: Layer::Top,
        exclusive_zone: 0,
        size: Some((0, A::surface_height())),
        margin: initial_margin,
        keyboard_interactivity: KeyboardInteractivity::None,
        events_transparent: A::events_transparent(),
        ..Default::default()
    })
    .run()
}

/// Subscription that repositions the surface as sibling overlays open/close.
fn stacking_subscription<A: OverlayApp>() -> Subscription<A::Message> {
    Subscription::run(stack_reflow_recipe::<A>)
}

/// Recipe for [`stacking_subscription`]: takes the claimed slot guard once and maps the
/// reflow margin stream into the app's own `MarginChange` message. Monomorphized per app, so
/// its `Subscription::run` identity is stable for the process.
fn stack_reflow_recipe<A: OverlayApp>() -> BoxStream<'static, A::Message> {
    let guard = STACK_GUARD
        .get()
        .and_then(|m| m.lock().ok().and_then(|mut g| g.take()));
    match guard {
        Some(g) => Box::pin(stack::reflow_stream(A::namespace(), g).map(A::margin_changed)),
        None => Box::pin(stream::pending()),
    }
}

// Free functions with the exact signature iced_layershell::application expects,
// forwarding to the trait methods.

fn update_wrapper<A: OverlayApp>(state: &mut A, message: A::Message) -> Task<A::Message> {
    state.update(message)
}

fn view_wrapper<A: OverlayApp>(state: &A) -> Element<'_, A::Message> {
    state.view()
}
