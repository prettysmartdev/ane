#[derive(Debug)]
pub struct AgentConfig {
    pub name: &'static str,
    pub skill_dir: &'static str,
    pub skill_filename: &'static str,
    /// Printed after writing if the agent requires manual integration steps
    /// (e.g. no auto-load convention, or an extra reference must be added).
    pub manual_note: Option<&'static str>,
}

pub const AGENTS: &[AgentConfig] = &[
    AgentConfig {
        name: "claude",
        skill_dir: ".claude/skills/ane",
        skill_filename: "SKILL.md",
        manual_note: None,
    },
    AgentConfig {
        name: "codex",
        skill_dir: ".agents/skills/ane",
        skill_filename: "SKILL.md",
        manual_note: None,
    },
    AgentConfig {
        name: "gemini",
        skill_dir: ".gemini/skills/ane",
        skill_filename: "SKILL.md",
        manual_note: None,
    },
    AgentConfig {
        name: "opencode",
        skill_dir: ".opencode/skills/ane",
        skill_filename: "SKILL.md",
        manual_note: None,
    },
    AgentConfig {
        name: "cline",
        skill_dir: ".cline/skills/ane",
        skill_filename: "SKILL.md",
        manual_note: None,
    },
    AgentConfig {
        name: "maki",
        skill_dir: ".agents/skills/ane",
        skill_filename: "SKILL.md",
        manual_note: Some(
            "Maki's per-project skill auto-loading is not publicly documented. This file \
             follows the Agent Skills open standard at .agents/skills/. If Maki does not \
             pick it up automatically, reference it from your AGENTS.md.",
        ),
    },
    AgentConfig {
        name: "crush",
        skill_dir: ".crush/skills/ane",
        skill_filename: "SKILL.md",
        manual_note: None,
    },
];

pub fn find_agent(name: &str) -> Option<&'static AgentConfig> {
    AGENTS.iter().find(|a| a.name.eq_ignore_ascii_case(name))
}

pub fn agent_names() -> Vec<&'static str> {
    AGENTS.iter().map(|a| a.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_agent_returns_config_for_all_supported_names() {
        for name in [
            "claude", "codex", "gemini", "opencode", "cline", "maki", "crush",
        ] {
            assert!(
                find_agent(name).is_some(),
                "find_agent({name:?}) returned None"
            );
        }
    }

    #[test]
    fn find_agent_is_case_insensitive() {
        assert!(find_agent("Claude").is_some());
        assert!(find_agent("CLAUDE").is_some());
    }

    #[test]
    fn find_agent_returns_none_for_unknown_agent() {
        assert!(find_agent("vim").is_none());
    }

    #[test]
    fn find_agent_returns_none_for_old_charm_name() {
        // 'charm' is the company; the agent is named 'crush'.
        assert!(find_agent("charm").is_none());
    }
}
