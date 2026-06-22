//! Pure label builders for the menu-bar title and menu rows.

use crate::state::ago_label;

/// Menu-bar title: free space with a warning glyph when below the threshold.
pub fn title(free_gb: u64, min_free_gb: u64) -> String {
    let glyph = if free_gb < min_free_gb { "⚠️" } else { "🧹" };
    format!("{glyph} {free_gb}G")
}

/// "Free: 35 GB" status row.
pub fn free_label(free_gb: u64) -> String {
    format!("Free: {free_gb} GB")
}

/// "Last clean: 2h ago" / "Last clean: never" status row.
pub fn last_clean_label(last: Option<u64>, now: u64) -> String {
    match last {
        None => "Last clean: never".to_string(),
        Some(t) => format!("Last clean: {}", ago_label(now.saturating_sub(t))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_warns_below_threshold_ac1() {
        assert_eq!(title(35, 25), "🧹 35G");
        assert_eq!(title(18, 25), "⚠️ 18G");
        // exactly at the threshold is not "below"
        assert_eq!(title(25, 25), "🧹 25G");
    }

    #[test]
    fn labels_ac3() {
        assert_eq!(last_clean_label(None, 1000), "Last clean: never");
        let now = 1_000_000;
        let two_h_ago = now - 2 * 3600;
        assert!(last_clean_label(Some(two_h_ago), now).contains("2h ago"));
        assert_eq!(free_label(35), "Free: 35 GB");
    }
}
