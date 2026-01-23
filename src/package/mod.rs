mod loader;
mod merger;
mod resolver;
pub mod source;
mod types;

// Re-export from config
// Allow deprecated PackageAuth - kept for backward compatibility
#[allow(deprecated)]
pub use crate::config::{PackageAuth, PackageReference};

// Export package-specific types
pub use loader::PackageLoader;
pub use merger::ServiceMerger;
pub use resolver::PackageResolver;
pub use types::{
    Package, PackageCache, PackageCacheEntry, PackageMetadata, PackageSource, PackageVersion,
};
