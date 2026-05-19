pub const SKILL_CONTENT: &str = include_str!("ane-skill.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_content_is_non_empty() {
        assert!(!SKILL_CONTENT.is_empty());
    }

    #[test]
    fn skill_content_contains_key_markers() {
        assert!(SKILL_CONTENT.contains("ane exec"), "missing 'ane exec'");
        assert!(SKILL_CONTENT.contains("Action"), "missing 'Action'");
        assert!(SKILL_CONTENT.contains("Scope"), "missing 'Scope'");
    }

    #[test]
    fn skill_content_under_800_words() {
        let word_count = SKILL_CONTENT.split_whitespace().count();
        assert!(
            word_count <= 800,
            "SKILL_CONTENT has {word_count} words, expected <= 800"
        );
    }
}
