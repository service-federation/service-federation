//! Script configuration types.
//!
//! This module contains the [`Script`] struct for configuring
//! runnable scripts in the federation config.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Script configuration for custom commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    /// Working directory for the script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Services that must be running before the script can execute
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,

    /// Environment variables for the script
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub environment: HashMap<String, String>,

    /// The script command to execute
    pub script: String,

    /// When true, run the script in complete isolation:
    /// - Allocate fresh random ports for all port-type parameters
    /// - Scope Docker volumes by session (myvolume → fed-{session}-myvolume)
    /// - Start dependencies in an isolated context
    /// - Clean up all resources after the script completes
    #[serde(default)]
    pub isolated: bool,
}
