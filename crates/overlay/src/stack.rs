//! Event-driven vertical stacking of sceno overlays at the bottom edge.
//!
//! Two cooperating layers, both crash-safe:
//! - **Slot ownership** via `flock(2)` marker files (`sceno-stack-<k>.lock`), mirroring
//!   [`crate::ensure_single_instance`]. A newly-started app claims the lowest free slot and
//!   keeps it; the kernel releases the lock when the process exits (even on SIGKILL).
//! - **The event** via D-Bus `NameOwnerChanged`: each app owns a `dev.sceno.<app>` bus name,
//!   so any sibling appearing/disappearing wakes the others. On each event an app tries to
//!   migrate down to the lowest free slot and emits its new margin.
//!
//! Slot correctness rests entirely on `flock` (atomic), so D-Bus timing only affects *when* a
//! reflow happens, never *who* owns which slot.

//! The slot-ownership (`flock`) and reflow (D-Bus) machinery below is Linux-only
//! and gated as such; the geometry math (margins, constants) is cross-platform so
//! the off-Linux window backend can reuse it.

#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use futures::channel::mpsc;
#[cfg(target_os = "linux")]
use futures::stream::BoxStream;

/// Margin tuple `(top, right, bottom, left)`, matching `LayerShellSettings.margin`.
pub type Margin = (i32, i32, i32, i32);

/// Offset of the bottom-most slot from the screen edge (the legacy single-app margin).
pub const BASE_MARGIN: i32 = 40;
/// Layer-shell surface height (matches `LayerShellSettings.size = (0, 80)`).
pub const SURFACE_HEIGHT: i32 = 80;
/// Visual gap between stacked surfaces.
pub const GAP: i32 = 8;
/// Vertical pitch between adjacent slots.
pub const PITCH: i32 = SURFACE_HEIGHT + GAP;
/// Defensive bound on how many slots to probe (real usage is 2).
#[cfg(target_os = "linux")]
const MAX_SLOTS: usize = 32;
/// D-Bus dispatch timeout; bounds how long a quit takes to notice a stuck bus, not latency.
#[cfg(target_os = "linux")]
const PROCESS_TIMEOUT: Duration = Duration::from_secs(1);
/// Shared bus-name prefix; one well-known name per app (`dev.sceno.<app>`).
#[cfg(target_os = "linux")]
const BUS_PREFIX: &str = "dev.sceno.";

/// Margin for a given stack slot at the bottom edge (slot 0 = bottom-most).
pub fn margin_for_slot(slot: usize) -> Margin {
    (0, 0, BASE_MARGIN + (slot as i32) * PITCH, 0)
}

#[cfg(target_os = "linux")]
fn slot_path(index: usize) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    std::path::Path::new(&dir).join(format!("sceno-stack-{index}.lock"))
}

/// RAII holder of a claimed stack slot. Dropping it closes the fd, releasing the `flock`.
#[cfg(target_os = "linux")]
pub struct SlotGuard {
    index: usize,
    _file: File, // held open => lock held; closed on drop => released
}

#[cfg(target_os = "linux")]
impl SlotGuard {
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn margin(&self) -> Margin {
        margin_for_slot(self.index)
    }
}

/// Try to exclusively claim slot `index`. `None` if another process holds it or on I/O error.
#[cfg(target_os = "linux")]
pub fn try_claim_slot(index: usize) -> Option<SlotGuard> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(slot_path(index))
        .ok()?;
    // LOCK_EX | LOCK_NB — exclusive, non-blocking; atomic per open file description.
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    (ret == 0).then_some(SlotGuard { index, _file: file })
}

/// Claim the lowest currently-free slot (probes 0, 1, 2, …).
#[cfg(target_os = "linux")]
pub fn claim_lowest() -> SlotGuard {
    (0..MAX_SLOTS)
        .find_map(try_claim_slot)
        .unwrap_or_else(|| panic!("no free stack slot in 0..{MAX_SLOTS}"))
}

/// Attempt to move `guard` down to the lowest free slot below its current index.
/// Acquire-before-release: the new slot is locked before the old one is dropped, so the
/// process never holds zero slots. Returns the new margin if it moved.
#[cfg(target_os = "linux")]
fn migrate_down(guard: &mut SlotGuard) -> Option<Margin> {
    for lower in 0..guard.index() {
        if let Some(new_guard) = try_claim_slot(lower) {
            let old = std::mem::replace(guard, new_guard);
            crate::debug(
                "stack",
                format_args!("migrated slot {} -> {}", old.index(), lower),
            );
            drop(old); // releases the old slot's flock
            return Some(guard.margin());
        }
    }
    None
}

/// Spawn the D-Bus presence + reflow loop, taking ownership of the already-claimed slot.
/// Emits a new margin whenever a sibling appearing/disappearing frees a lower slot.
#[cfg(target_os = "linux")]
pub fn reflow_stream(app: &str, guard: SlotGuard) -> BoxStream<'static, Margin> {
    let (tx, rx) = mpsc::unbounded::<Margin>();
    let bus_name = format!("{BUS_PREFIX}{app}");
    std::thread::spawn(move || {
        use dbus::blocking::Connection;
        use dbus::message::MatchRule;

        let Ok(conn) = Connection::new_session() else {
            crate::debug("stack", format_args!("no session bus; reflow disabled"));
            return;
        };
        // Announce presence so siblings get a NameOwnerChanged for us.
        let _ = conn.request_name(&bus_name, false, true, false);

        let mut guard = guard;
        let rule = MatchRule::new_signal("org.freedesktop.DBus", "NameOwnerChanged");
        let added = conn.add_match(rule, move |_: (), _conn, msg| {
            // Args: (name, old_owner, new_owner). Any dev.sceno.* change may free a slot.
            let (name, _old, _new) = msg.get3::<String, String, String>();
            if name.as_deref().is_some_and(|n| n.starts_with(BUS_PREFIX))
                && let Some(margin) = migrate_down(&mut guard)
            {
                let _ = tx.unbounded_send(margin);
            }
            true // keep the match alive
        });
        if added.is_err() {
            crate::debug("stack", format_args!("add_match failed; reflow disabled"));
            return;
        }

        loop {
            if conn.process(PROCESS_TIMEOUT).is_err() {
                break;
            }
        }
    });
    Box::pin(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margin_for_slot_steps_by_pitch() {
        assert_eq!(margin_for_slot(0), (0, 0, 40, 0));
        assert_eq!(margin_for_slot(1), (0, 0, 40 + 88, 0));
        assert_eq!(margin_for_slot(2), (0, 0, 40 + 176, 0));
    }
}
