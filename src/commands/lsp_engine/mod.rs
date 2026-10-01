mod detector;
mod engine;
mod health;
mod installer;
mod transport;

pub use engine::{LspClient, LspEngine, LspEngineConfig, LspProvider, NoLsp, ShutdownControl};
pub use installer::InstallProgress;

#[cfg(test)]
pub(crate) use engine::SemanticTestGate;
