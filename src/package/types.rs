use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Parsed package source
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageSource {
    Local {
        path: PathBuf,
    },
    GitHub {
        org: String,
        repo: String,
        version: PackageVersion,
    },
    Git {
        url: String,
        version: PackageVersion,
    },
}

impl PackageSource {
    /// Get the identity of this package source (without version).
    /// Used for detecting diamond dependencies where the same package
    /// is required at multiple versions.
    ///
    /// Returns None for Local sources since they don't have versioning.
    pub fn identity(&self) -> Option<String> {
        match self {
            PackageSource::Local { .. } => None,
            PackageSource::GitHub { org, repo, .. } => Some(format!("github:{}/{}", org, repo)),
            PackageSource::Git { url, .. } => {
                // Strip version from URL if present
                let base_url = url.split('@').next().unwrap_or(url);
                Some(format!("git:{}", base_url))
            }
        }
    }

    /// Get the version of this package source.
    pub fn version(&self) -> PackageVersion {
        match self {
            PackageSource::Local { .. } => PackageVersion::Latest,
            PackageSource::GitHub { version, .. } | PackageSource::Git { version, .. } => {
                version.clone()
            }
        }
    }
}

/// Package version specifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PackageVersion {
    Tag(String),
    Branch(String),
    Commit(String),
    Latest,
}

impl PackageVersion {
    pub fn as_string(&self) -> String {
        match self {
            PackageVersion::Tag(t) => t.clone(),
            PackageVersion::Branch(b) => b.clone(),
            PackageVersion::Commit(c) => c.clone(),
            PackageVersion::Latest => "latest".to_string(),
        }
    }
}

impl std::fmt::Display for PackageVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

/// Loaded package with metadata
#[derive(Debug, Clone)]
pub struct Package {
    /// Package reference/alias
    pub alias: String,

    /// Resolved source
    pub source: PackageSource,

    /// Package configuration
    pub config: Config,

    /// Resolved path on filesystem
    pub path: PathBuf,

    /// Metadata
    pub metadata: PackageMetadata,
}

/// Package metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// Package name
    pub name: Option<String>,

    /// Package description
    pub description: Option<String>,

    /// Package version
    pub version: Option<String>,

    /// Last updated timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,

    /// Checksum for integrity
    pub checksum: Option<String>,
}

/// Cache entry for a package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageCacheEntry {
    /// Package source
    pub source: String,

    /// Package version
    pub version: PackageVersion,

    /// Filesystem path
    pub path: PathBuf,

    /// Metadata
    pub metadata: PackageMetadata,

    /// Cached at timestamp
    pub cached_at: chrono::DateTime<chrono::Utc>,
}

/// Package cache structure
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PackageCache {
    /// Cache entries by source key
    pub entries: HashMap<String, PackageCacheEntry>,
}
