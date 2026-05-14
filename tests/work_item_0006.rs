use ane::data::init::init_agent;
use ane::data::skill::SKILL_CONTENT;

// --- Skill token count ---

#[test]
fn skill_file_under_400_tokens() {
    let word_count = SKILL_CONTENT.split_whitespace().count();
    assert!(
        word_count < 400,
        "ane-skill.md has {word_count} whitespace-delimited words, expected < 400"
    );
}

// --- run_init integration tests ---

#[test]
fn run_init_creates_directory_and_file() {
    let dir = tempfile::TempDir::new().unwrap();
    init_agent("claude", dir.path()).unwrap();

    let skill_path = dir.path().join(".claude/skills/ane/SKILL.md");
    assert!(
        skill_path.exists(),
        "skill file should exist at {}",
        skill_path.display()
    );
    assert_eq!(std::fs::read_to_string(&skill_path).unwrap(), SKILL_CONTENT);
}

#[test]
fn run_init_overwrites_existing_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let skill_dir = dir.path().join(".claude/skills/ane");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "dummy content").unwrap();

    init_agent("claude", dir.path()).unwrap();

    let contents = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert_eq!(contents, SKILL_CONTENT);
}

#[test]
fn run_init_unknown_agent_returns_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let err = init_agent("vim", dir.path()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unknown agent"),
        "expected 'unknown agent' in: {msg}"
    );
    assert!(msg.contains("claude"), "expected agent list in: {msg}");
}
