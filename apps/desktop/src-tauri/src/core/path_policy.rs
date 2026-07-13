use std::path::{Component, Path, PathBuf};

use super::error::{KruonError, KruonResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPaths {
    pub workspace_root: PathBuf,
    pub working_directory: PathBuf,
}

pub struct PathPolicy;

impl PathPolicy {
    pub fn validate(
        workspace_root: impl AsRef<Path>,
        working_directory: impl AsRef<Path>,
    ) -> KruonResult<ValidatedPaths> {
        let workspace_root = workspace_root.as_ref();
        let requested = working_directory.as_ref();
        if requested
            .components()
            .any(|part| part == Component::ParentDir)
        {
            return Err(KruonError::PathPolicy(
                "working directory must not contain '..' components".into(),
            ));
        }

        let canonical_workspace = workspace_root.canonicalize().map_err(|error| {
            KruonError::PathPolicy(format!(
                "workspace {} cannot be canonicalized: {error}",
                workspace_root.display()
            ))
        })?;
        if !canonical_workspace.is_dir() {
            return Err(KruonError::PathPolicy(format!(
                "workspace {} is not a directory",
                canonical_workspace.display()
            )));
        }

        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            canonical_workspace.join(requested)
        };
        let canonical_working = candidate.canonicalize().map_err(|error| {
            KruonError::PathPolicy(format!(
                "working directory {} cannot be canonicalized: {error}",
                candidate.display()
            ))
        })?;
        if !canonical_working.is_dir() {
            return Err(KruonError::PathPolicy(format!(
                "working directory {} is not a directory",
                canonical_working.display()
            )));
        }
        if !canonical_working.starts_with(&canonical_workspace) {
            return Err(KruonError::PathPolicy(format!(
                "working directory {} escapes workspace {}",
                canonical_working.display(),
                canonical_workspace.display()
            )));
        }

        Ok(ValidatedPaths {
            workspace_root: canonical_workspace,
            working_directory: canonical_working,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_child_directory() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let paths = PathPolicy::validate(root.path(), "child").unwrap();
        assert_eq!(paths.working_directory, child.canonicalize().unwrap());
    }

    #[test]
    fn rejects_parent_traversal_and_absolute_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(PathPolicy::validate(root.path(), "../outside").is_err());
        assert!(PathPolicy::validate(root.path(), outside.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("escape")).unwrap();
        assert!(PathPolicy::validate(root.path(), "escape").is_err());
    }
}
