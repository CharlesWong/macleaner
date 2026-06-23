//! Pure builder for the menu-bar title.

/// Menu-bar title: free space with a warning glyph when below the threshold.
pub fn title(free_gb: u64, min_free_gb: u64) -> String {
    let glyph = if free_gb < min_free_gb { "⚠️" } else { "🧹" };
    format!("{glyph} {free_gb}G")
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
}
