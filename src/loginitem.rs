//! Login Item registration via `SMAppService` (the modern macOS API).
//! Registration only takes effect when the process runs from a real `.app`
//! bundle; from a bare binary it is a harmless no-op/error.

use objc2_service_management::{SMAppService, SMAppServiceStatus};

/// Pure mapping: is this status the "enabled at login" state?
pub fn status_is_enabled(status: SMAppServiceStatus) -> bool {
    status.0 == SMAppServiceStatus::Enabled.0
}

/// Decide what a launch-time reconcile should do given the desired intent and
/// the current OS state. `Some(true)` = register, `Some(false)` = unregister,
/// `None` = already in sync. Bidirectional so an out-of-band config change is
/// honored on the next launch.
pub fn reconcile_action(want_enabled: bool, currently_enabled: bool) -> Option<bool> {
    match (want_enabled, currently_enabled) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// Current Login Item status for this app.
pub fn is_enabled() -> bool {
    // SAFETY: objc class/instance calls with no preconditions beyond a live
    // Objective-C runtime, which is always present on macOS.
    let status = unsafe { SMAppService::mainAppService().status() };
    status_is_enabled(status)
}

/// Register this app as a Login Item.
pub fn register() -> anyhow::Result<()> {
    // SAFETY: see `is_enabled`.
    unsafe { SMAppService::mainAppService().registerAndReturnError() }
        .map_err(|e| anyhow::anyhow!("SMAppService register failed: {e:?}"))
}

/// Unregister this app as a Login Item.
pub fn unregister() -> anyhow::Result<()> {
    // SAFETY: see `is_enabled`.
    unsafe { SMAppService::mainAppService().unregisterAndReturnError() }
        .map_err(|e| anyhow::anyhow!("SMAppService unregister failed: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_ac7() {
        assert!(status_is_enabled(SMAppServiceStatus::Enabled));
        assert!(!status_is_enabled(SMAppServiceStatus::NotRegistered));
        assert!(!status_is_enabled(SMAppServiceStatus::NotFound));
        assert!(!status_is_enabled(SMAppServiceStatus::RequiresApproval));
    }

    #[test]
    fn reconcile_is_bidirectional() {
        assert_eq!(reconcile_action(true, false), Some(true)); // want on, off → register
        assert_eq!(reconcile_action(false, true), Some(false)); // want off, on → unregister
        assert_eq!(reconcile_action(true, true), None); // already in sync
        assert_eq!(reconcile_action(false, false), None);
    }
}
