//! Cross-platform system-tray menu.
//!
//! Each app describes its tray menu **once** as a declarative [`Menu`] tree whose
//! actionable leaves carry the app's own `Message` to emit on click. Two backends
//! render the same tree:
//!
//! - **Linux** — `ksni` (StatusNotifierItem over D-Bus). [`spawn`] starts the tray
//!   on its own thread (called from the app's `event_stream`), exactly like the
//!   per-app `ksni::Tray` impls it replaces.
//! - **macOS** — `tray-icon` (`NSStatusItem`). The `NSStatusItem` must be created
//!   on the main thread with the event loop already running, so the app drives it
//!   from `update`: a periodic [`tick_stream`] message calls [`pump`], which lazily
//!   builds the tray on first tick and then drains menu clicks into app messages.
//!
//! Live-state note: the tray reflects its *own* clicks (checkmarks/radios flip
//! locally). External config changes (reset / live edit) don't reach the ksni
//! tray thread on Linux (parity with the old per-app trays), but on macOS the app
//! can call [`refresh`] to rebuild it so those changes show there too.

use std::sync::Arc;

/// A tray menu: its title/icon plus the entry tree. `M` is the app message type.
pub struct Menu<M> {
    /// Tooltip (both platforms) and the ksni tray title (Linux).
    pub title: String,
    /// Freedesktop icon name shown in the Linux system tray.
    pub icon_name: String,
    /// Short text shown in the macOS menu bar (an emoji/letter — there's no
    /// freedesktop icon there). Ignored on Linux.
    pub mac_label: String,
    /// The top-level menu entries.
    pub items: Vec<Item<M>>,
}

/// One menu entry. Actionable leaves carry the message to emit when activated.
pub enum Item<M> {
    /// A plain action: emits `message` on click.
    Button { label: String, message: M },
    /// A checkbox. On click it flips and emits `toggle(new_checked)`.
    Check {
        label: String,
        checked: bool,
        toggle: Arc<dyn Fn(bool) -> M + Send + Sync>,
    },
    /// A mutually-exclusive group. Clicking option `i` selects it and emits
    /// `select(i)`. (On macOS, rendered as a run of checkmarks — muda has no
    /// native radio; on Linux it's a real `RadioGroup`.)
    Radio {
        selected: usize,
        options: Vec<String>,
        select: Arc<dyn Fn(usize) -> M + Send + Sync>,
    },
    /// A nested submenu.
    Sub { label: String, items: Vec<Item<M>> },
    /// A horizontal separator.
    Separator,
}

impl<M> Item<M> {
    /// A clickable action emitting `message`.
    pub fn button(label: impl Into<String>, message: M) -> Self {
        Item::Button {
            label: label.into(),
            message,
        }
    }

    /// A checkbox; `toggle(new)` maps the flipped state to a message.
    pub fn check(
        label: impl Into<String>,
        checked: bool,
        toggle: impl Fn(bool) -> M + Send + Sync + 'static,
    ) -> Self {
        Item::Check {
            label: label.into(),
            checked,
            toggle: Arc::new(toggle),
        }
    }

    /// A radio group; `select(idx)` maps the chosen option to a message.
    pub fn radio(
        selected: usize,
        options: Vec<String>,
        select: impl Fn(usize) -> M + Send + Sync + 'static,
    ) -> Self {
        Item::Radio {
            selected,
            options,
            select: Arc::new(select),
        }
    }

    /// A nested submenu.
    pub fn sub(label: impl Into<String>, items: Vec<Item<M>>) -> Self {
        Item::Sub {
            label: label.into(),
            items,
        }
    }
}

// ───────────────────────────── Linux: ksni ──────────────────────────────────

#[cfg(target_os = "linux")]
pub use linux::spawn;

#[cfg(target_os = "linux")]
mod linux {
    use super::{Item, Menu};
    use futures::channel::mpsc::UnboundedSender;

    /// Spawn the ksni tray on its own thread. Call from the app's `event_stream`.
    /// Clicks are forwarded to `tx` as app messages.
    pub fn spawn<M: Clone + Send + 'static>(menu: Menu<M>, tx: UnboundedSender<M>) {
        ksni::TrayService::new(KsniTray { menu, tx }).spawn();
    }

    struct KsniTray<M> {
        menu: Menu<M>,
        tx: UnboundedSender<M>,
    }

    impl<M: Clone + Send + 'static> ksni::Tray for KsniTray<M> {
        fn icon_name(&self) -> String {
            self.menu.icon_name.clone()
        }
        fn title(&self) -> String {
            self.menu.title.clone()
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            render(&self.menu.items, &[])
        }
    }

    /// Walk to the live `Item` at `path` (each step indexes the current level;
    /// descends through `Sub`). Used by click closures to flip check/radio state
    /// so ksni's post-activation re-render shows the new state.
    fn item_at_mut<'a, M>(items: &'a mut [Item<M>], path: &[usize]) -> Option<&'a mut Item<M>> {
        let (first, rest) = path.split_first()?;
        let it = items.get_mut(*first)?;
        if rest.is_empty() {
            Some(it)
        } else if let Item::Sub { items, .. } = it {
            item_at_mut(items, rest)
        } else {
            None
        }
    }

    fn render<M: Clone + Send + 'static>(
        items: &[Item<M>],
        prefix: &[usize],
    ) -> Vec<ksni::MenuItem<KsniTray<M>>> {
        use ksni::menu::*;
        items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let mut path = prefix.to_vec();
                path.push(i);
                match item {
                    Item::Separator => ksni::MenuItem::Separator,
                    Item::Button { label, message } => {
                        let msg = message.clone();
                        StandardItem {
                            label: label.clone(),
                            activate: Box::new(move |t: &mut KsniTray<M>| {
                                let _ = t.tx.unbounded_send(msg.clone());
                            }),
                            ..Default::default()
                        }
                        .into()
                    }
                    Item::Check { label, checked, .. } => CheckmarkItem {
                        label: label.clone(),
                        checked: *checked,
                        activate: Box::new(move |t: &mut KsniTray<M>| {
                            if let Some(Item::Check {
                                checked, toggle, ..
                            }) = item_at_mut(&mut t.menu.items, &path)
                            {
                                *checked = !*checked;
                                let _ = t.tx.unbounded_send(toggle(*checked));
                            }
                        }),
                        ..Default::default()
                    }
                    .into(),
                    Item::Radio {
                        selected, options, ..
                    } => RadioGroup {
                        selected: *selected,
                        select: Box::new(move |t: &mut KsniTray<M>, idx| {
                            if let Some(Item::Radio {
                                selected, select, ..
                            }) = item_at_mut(&mut t.menu.items, &path)
                            {
                                *selected = idx;
                                let _ = t.tx.unbounded_send(select(idx));
                            }
                        }),
                        options: options
                            .iter()
                            .map(|o| RadioItem {
                                label: o.clone(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                    Item::Sub { label, items } => SubMenu {
                        label: label.clone(),
                        submenu: render(items, &path),
                        ..Default::default()
                    }
                    .into(),
                }
            })
            .collect()
    }
}

// ───────────────────────────── macOS: tray-icon ─────────────────────────────

#[cfg(not(target_os = "linux"))]
pub use mac::{pump, refresh, tick_stream};

#[cfg(not(target_os = "linux"))]
mod mac {
    use super::{Item, Menu};
    use std::any::Any;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;

    use futures::channel::mpsc;
    use futures::stream::BoxStream;
    use tray_icon::menu::{
        CheckMenuItem, IsMenuItem, Menu as MudaMenu, MenuEvent, MenuId, MenuItem as MudaItem,
        PredefinedMenuItem, Submenu,
    };
    use tray_icon::{TrayIcon, TrayIconBuilder};

    /// The live macOS tray: the `NSStatusItem` plus a click → action map. `!Send`
    /// (the muda/tray handles aren't), so it stays in a main-thread `thread_local`.
    struct MacTray<M> {
        _tray: TrayIcon,
        actions: Vec<Action<M>>,
        by_id: HashMap<MenuId, usize>,
    }

    enum Action<M> {
        Button(M),
        Check {
            handle: CheckMenuItem,
            checked: bool,
            toggle: Arc<dyn Fn(bool) -> M + Send + Sync>,
        },
        /// One option of a radio run; `group` holds *all* sibling checks so the
        /// chosen one can be lit and the rest cleared.
        RadioOption {
            group: Vec<CheckMenuItem>,
            choose: usize,
            select: Arc<dyn Fn(usize) -> M + Send + Sync>,
        },
    }

    thread_local! {
        // Type-erased so the `thread_local` needn't be generic over `M`; `pump`
        // downcasts back to `MacTray<M>` (sound — only one app/`M` per process).
        static TRAY: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    }

    /// Container we can append menu items to (root menu or a submenu).
    trait Parent {
        fn add(&self, item: &dyn IsMenuItem);
    }
    impl Parent for MudaMenu {
        fn add(&self, item: &dyn IsMenuItem) {
            let _ = self.append(item);
        }
    }
    impl Parent for Submenu {
        fn add(&self, item: &dyn IsMenuItem) {
            let _ = self.append(item);
        }
    }

    fn fill<M: Clone + 'static>(
        parent: &dyn Parent,
        items: &[Item<M>],
        actions: &mut Vec<Action<M>>,
        by_id: &mut HashMap<MenuId, usize>,
    ) {
        for item in items {
            match item {
                Item::Separator => parent.add(&PredefinedMenuItem::separator()),
                Item::Button { label, message } => {
                    let mi = MudaItem::new(label, true, None);
                    by_id.insert(mi.id().clone(), actions.len());
                    actions.push(Action::Button(message.clone()));
                    parent.add(&mi);
                }
                Item::Check {
                    label,
                    checked,
                    toggle,
                } => {
                    let mi = CheckMenuItem::new(label, true, *checked, None);
                    by_id.insert(mi.id().clone(), actions.len());
                    actions.push(Action::Check {
                        handle: mi.clone(),
                        checked: *checked,
                        toggle: toggle.clone(),
                    });
                    parent.add(&mi);
                }
                Item::Radio {
                    selected,
                    options,
                    select,
                } => {
                    let group: Vec<CheckMenuItem> = options
                        .iter()
                        .enumerate()
                        .map(|(k, o)| CheckMenuItem::new(o, true, k == *selected, None))
                        .collect();
                    for (k, mi) in group.iter().enumerate() {
                        by_id.insert(mi.id().clone(), actions.len());
                        actions.push(Action::RadioOption {
                            group: group.clone(),
                            choose: k,
                            select: select.clone(),
                        });
                    }
                    for mi in &group {
                        parent.add(mi);
                    }
                }
                Item::Sub { label, items } => {
                    let sm = Submenu::new(label, true);
                    fill(&sm, items, actions, by_id);
                    parent.add(&sm);
                }
            }
        }
    }

    impl<M: Clone + 'static> MacTray<M> {
        fn build(desc: Menu<M>) -> Self {
            let menu = MudaMenu::new();
            let mut actions = Vec::new();
            let mut by_id = HashMap::new();
            fill(&menu, &desc.items, &mut actions, &mut by_id);
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip(&desc.title)
                .with_title(&desc.mac_label)
                .build()
                .expect("build NSStatusItem tray");
            MacTray {
                _tray: tray,
                actions,
                by_id,
            }
        }

        fn drain(&mut self) -> Vec<M> {
            let mut out = Vec::new();
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                let Some(&ai) = self.by_id.get(&ev.id) else {
                    continue;
                };
                match &mut self.actions[ai] {
                    Action::Button(m) => out.push(m.clone()),
                    Action::Check {
                        handle,
                        checked,
                        toggle,
                    } => {
                        // Drive the displayed state from our own model (don't rely
                        // on muda's auto-toggle) so it can't desync.
                        *checked = !*checked;
                        handle.set_checked(*checked);
                        out.push(toggle(*checked));
                    }
                    Action::RadioOption {
                        group,
                        choose,
                        select,
                    } => {
                        for (k, h) in group.iter().enumerate() {
                            h.set_checked(k == *choose);
                        }
                        out.push(select(*choose));
                    }
                }
            }
            out
        }
    }

    /// Drive the macOS tray from the app's `update`. On the first call it creates
    /// the `NSStatusItem` from `desc()` (must be on the main thread with the event
    /// loop live — i.e. inside iced's `update`); every call drains pending menu
    /// clicks into app messages for the app to dispatch.
    pub fn pump<M: Clone + 'static>(desc: impl FnOnce() -> Menu<M>) -> Vec<M> {
        TRAY.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                *slot = Some(Box::new(MacTray::build(desc())));
            }
            slot.as_mut()
                .unwrap()
                .downcast_mut::<MacTray<M>>()
                .expect("tray message type")
                .drain()
        })
    }

    /// Rebuild the tray from `desc` — used after the app changes config behind the
    /// tray's back (reset/external edit) so its checkmarks/radios reflect the new
    /// state (clicks alone keep them in sync). Must run on the main thread (call
    /// from `update`). No-op until the tray exists (the first [`pump`] builds it
    /// fresh anyway). Replacing the handle drops the old `NSStatusItem` and adds a
    /// new one.
    pub fn refresh<M: Clone + 'static>(desc: impl FnOnce() -> Menu<M>) {
        TRAY.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_some() {
                *slot = Some(Box::new(MacTray::build(desc())));
            }
        });
    }

    /// A ~100 ms tick stream (thread + mpsc, the repo's `BoxStream` pattern) whose
    /// message — built by `make` — should route to [`pump`] in the app's `update`.
    pub fn tick_stream<Msg: Send + 'static>(
        make: impl Fn() -> Msg + Send + 'static,
    ) -> BoxStream<'static, Msg> {
        let (tx, rx) = mpsc::unbounded::<Msg>();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if tx.unbounded_send(make()).is_err() {
                    break;
                }
            }
        });
        Box::pin(rx)
    }
}
