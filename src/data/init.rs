use std::path::Path;

use anyhow::{Context, Result};

use crate::data::agents::{self, AgentConfig};
use crate::data::skill::SKILL_CONTENT;

pub fn init_agent(agent_name: &str, base_dir: &Path) -> Result<&'static AgentConfig> {
    let config = agents::find_agent(agent_name).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown agent '{}'. Supported: {}",
            agent_name,
            agents::agent_names().join(", ")
        )
    })?;

    let skill_dir = base_dir.join(config.skill_dir);
    std::fs::create_dir_all(&skill_dir)
        .with_context(|| format!("failed to create skill directory {}", skill_dir.display()))?;

    let skill_path = skill_dir.join(config.skill_filename);
    std::fs::write(&skill_path, SKILL_CONTENT)
        .with_context(|| format!("failed to write skill file {}", skill_path.display()))?;

    Ok(config)
}
