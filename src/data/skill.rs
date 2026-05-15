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
}
