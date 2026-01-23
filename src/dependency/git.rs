use crate::error::Result;
use git2::Repository;
use std::path::{Path, PathBuf};

/// Git operations for external dependencies
pub struct GitOperations;

impl GitOperations {
    /// Clone or update a repository
    pub fn clone_or_update(
        repo_url: &str,
        branch: Option<&str>,
        target_path: &Path,
    ) -> Result<PathBuf> {
        if target_path.exists() {
            // Repository already exists, try to update it
            Self::update_repository(target_path, branch)?;
        } else {
            // Clone the repository
            Self::clone_repository(repo_url, branch, target_path)?;
        }

        Ok(target_path.to_path_buf())
    }

    fn clone_repository(repo_url: &str, branch: Option<&str>, target_path: &Path) -> Result<()> {
        let mut builder = git2::build::RepoBuilder::new();

        if let Some(branch_name) = branch {
            builder.branch(branch_name);
        }

        builder.clone(repo_url, target_path)?;

        Ok(())
    }

    fn update_repository(repo_path: &Path, branch: Option<&str>) -> Result<()> {
        let repo = Repository::open(repo_path)?;

        // Fetch from remote
        let mut remote = repo.find_remote("origin")?;
        remote.fetch(&["refs/heads/*:refs/heads/*"], None, None)?;

        // Checkout branch if specified
        if let Some(branch_name) = branch {
            let branch_ref = format!("refs/heads/{}", branch_name);
            let obj = repo.revparse_single(&branch_ref)?;
            repo.checkout_tree(&obj, None)?;
            repo.set_head(&branch_ref)?;
        }

        Ok(())
    }

    /// Check if a path is a git repository
    pub fn is_git_repo(path: &Path) -> bool {
        Repository::open(path).is_ok()
    }

    /// Get current branch name
    pub fn get_current_branch(path: &Path) -> Result<String> {
        let repo = Repository::open(path)?;
        let head = repo.head()?;

        if let Some(branch_name) = head.shorthand() {
            Ok(branch_name.to_string())
        } else {
            Ok("HEAD".to_string())
        }
    }
}
