//! #453: in-session last-check display after an update-channel switch.
//!
//! State files stay isolated (`update-state.json` vs `update-state-preview.json`).
//! This module only selects which file the UI should read, and whether a
//! check that started on one channel may drive the UI after the user has
//! already switched to the other. It does not change check/download/install
//! semantics or state-file format.

use std::path::Path;

use super::{paths, UpdateChannel, UpdateState};

/// Last successful check time the UI should show for `channel`.
/// Reads only that channel's state file.
pub fn last_check_unix_for_channel(channel: UpdateChannel) -> Option<i64> {
    last_check_unix_from(&paths::state_file_for(channel))
}

pub fn last_check_unix_from(path: &Path) -> Option<i64> {
    UpdateState::load_from(path).last_check_unix
}

/// A check started on `started` must not rewrite the in-session UI after
/// the user is already on `current`. The worker already writes only to
/// `started`'s file.
pub fn check_result_belongs_to_current(started: UpdateChannel, current: UpdateChannel) -> bool {
    started == current
}

/// Channel state file inside `dir`, matching production file names.
pub fn state_file_in(dir: &Path, channel: UpdateChannel) -> std::path::PathBuf {
    dir.join(match channel {
        UpdateChannel::Stable => "update-state.json",
        UpdateChannel::Preview => "update-state-preview.json",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Distinct last-check instants (Asia/Shanghai wall times used in the issue / GUI fixture).
    const STABLE_TS: i64 = 1_795_244_400; // 2026-11-21 15:00 CST
    const PREVIEW_TS: i64 = 1_795_311_000; // 2026-11-22 09:30 CST

    fn write_state(path: &Path, last_check_unix: Option<i64>, client_id: &str) {
        let mut st = UpdateState::default();
        st.last_check_unix = last_check_unix;
        st.client_id = Some(client_id.to_string());
        st.save_to(path).unwrap();
    }

    fn load(path: &Path) -> UpdateState {
        UpdateState::load_from(path)
    }

    #[test]
    fn stable_preview_stable_uses_each_channel_file_including_no_history() {
        let dir = tempdir().unwrap();
        let stable = state_file_in(dir.path(), UpdateChannel::Stable);
        let preview = state_file_in(dir.path(), UpdateChannel::Preview);
        write_state(&stable, Some(STABLE_TS), "cid-stable");
        // Preview has never been checked: no file (FirstRun / no history).
        assert!(!preview.exists());

        // Display follows the current channel only.
        assert_eq!(
            last_check_unix_from(&state_file_in(dir.path(), UpdateChannel::Stable)),
            Some(STABLE_TS)
        );
        assert_eq!(
            last_check_unix_from(&state_file_in(dir.path(), UpdateChannel::Preview)),
            None,
            "preview with no history must show 还没有检查过更新, not the stable timestamp"
        );

        // Switching back to Stable still reads the stable file (restart-equivalent reload).
        assert_eq!(last_check_unix_from(&stable), Some(STABLE_TS));
        // Isolation: preview file still absent; stable client_id / time unchanged.
        assert!(!preview.exists());
        let st = load(&stable);
        assert_eq!(st.last_check_unix, Some(STABLE_TS));
        assert_eq!(st.client_id.as_deref(), Some("cid-stable"));
    }

    #[test]
    fn distinct_last_check_states_do_not_cross_write() {
        let dir = tempdir().unwrap();
        let stable = state_file_in(dir.path(), UpdateChannel::Stable);
        let preview = state_file_in(dir.path(), UpdateChannel::Preview);
        write_state(&stable, Some(STABLE_TS), "cid-stable");
        write_state(&preview, Some(PREVIEW_TS), "cid-preview");

        // Stable → Preview → Stable display
        assert_eq!(last_check_unix_from(&stable), Some(STABLE_TS));
        assert_eq!(last_check_unix_from(&preview), Some(PREVIEW_TS));
        assert_eq!(last_check_unix_from(&stable), Some(STABLE_TS));

        let st = load(&stable);
        let pv = load(&preview);
        assert_eq!(st.last_check_unix, Some(STABLE_TS));
        assert_eq!(pv.last_check_unix, Some(PREVIEW_TS));
        assert_eq!(st.client_id.as_deref(), Some("cid-stable"));
        assert_eq!(pv.client_id.as_deref(), Some("cid-preview"));
        assert_ne!(st.client_id, pv.client_id);
    }

    #[test]
    fn delayed_old_channel_result_cannot_drive_new_channel_display() {
        let dir = tempdir().unwrap();
        let stable = state_file_in(dir.path(), UpdateChannel::Stable);
        let preview = state_file_in(dir.path(), UpdateChannel::Preview);
        write_state(&stable, Some(STABLE_TS), "cid-stable");
        // User switches to Preview (no history) while a Stable check is in flight.
        assert!(!preview.exists());
        assert!(!check_result_belongs_to_current(
            UpdateChannel::Stable,
            UpdateChannel::Preview
        ));
        // The finishing worker would write only the started (Stable) file.
        let later = STABLE_TS + 60;
        write_state(&stable, Some(later), "cid-stable");
        // UI for Preview still has no history; Stable file is not copied over.
        assert_eq!(last_check_unix_from(&preview), None);
        assert_eq!(last_check_unix_from(&stable), Some(later));
        assert!(!preview.exists());

        // Switch back: a late Preview check must not overwrite Stable's display.
        write_state(&preview, Some(PREVIEW_TS), "cid-preview");
        assert!(!check_result_belongs_to_current(
            UpdateChannel::Preview,
            UpdateChannel::Stable
        ));
        assert_eq!(last_check_unix_from(&stable), Some(later));
        assert_eq!(last_check_unix_from(&preview), Some(PREVIEW_TS));
        assert!(check_result_belongs_to_current(
            UpdateChannel::Stable,
            UpdateChannel::Stable
        ));
    }
}
