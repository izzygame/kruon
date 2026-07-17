use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleasePlatform {
    Windows,
    MacOs,
    Other,
}

pub struct DataRoots<'a> {
    pub local_app_data: Option<&'a Path>,
    pub home: Option<&'a Path>,
    pub xdg_data_home: Option<&'a Path>,
    pub temporary: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataLifecyclePolicy {
    pub schema_version: u32,
    pub location_class: &'static str,
    pub upgrade: &'static str,
    pub application_downgrade: &'static str,
    pub uninstall: &'static str,
    pub removal: &'static str,
}

pub fn data_lifecycle_policy() -> DataLifecyclePolicy {
    DataLifecyclePolicy {
        schema_version: 1,
        location_class: "os_app_data",
        upgrade: "retain_and_migrate_transactionally",
        application_downgrade: "fail_closed_on_newer_schema",
        uninstall: "retain_by_default",
        removal: "explicit_user_action_required",
    }
}

pub fn database_path_for(platform: ReleasePlatform, roots: DataRoots<'_>) -> PathBuf {
    let base = match platform {
        ReleasePlatform::Windows => roots
            .local_app_data
            .unwrap_or(roots.temporary)
            .to_path_buf(),
        ReleasePlatform::MacOs => roots
            .home
            .unwrap_or(roots.temporary)
            .join("Library")
            .join("Application Support"),
        ReleasePlatform::Other => roots
            .xdg_data_home
            .map(Path::to_path_buf)
            .or_else(|| roots.home.map(|home| home.join(".local").join("share")))
            .unwrap_or_else(|| roots.temporary.to_path_buf()),
    };
    base.join("kruon").join("kruon.sqlite3")
}

pub fn default_database_path() -> PathBuf {
    let temporary = std::env::temp_dir();
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let platform = if cfg!(target_os = "windows") {
        ReleasePlatform::Windows
    } else if cfg!(target_os = "macos") {
        ReleasePlatform::MacOs
    } else {
        ReleasePlatform::Other
    };
    database_path_for(
        platform,
        DataRoots {
            local_app_data: local_app_data.as_deref(),
            home: home.as_deref(),
            xdg_data_home: xdg_data_home.as_deref(),
            temporary: &temporary,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_database_path_is_stable_and_outside_the_app_bundle() {
        let home = Path::new("/Users/alpha");
        let temporary = Path::new("/tmp");
        let database = database_path_for(
            ReleasePlatform::MacOs,
            DataRoots {
                local_app_data: None,
                home: Some(home),
                xdg_data_home: None,
                temporary,
            },
        );
        assert_eq!(
            database,
            Path::new("/Users/alpha/Library/Application Support/kruon/kruon.sqlite3")
        );
        assert!(!database.to_string_lossy().contains("kruon.app"));
        assert!(!database
            .to_string_lossy()
            .contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn lifecycle_policy_retains_data_and_rejects_unsafe_downgrades() {
        let policy = data_lifecycle_policy();
        assert_eq!(policy.location_class, "os_app_data");
        assert_eq!(policy.upgrade, "retain_and_migrate_transactionally");
        assert_eq!(policy.application_downgrade, "fail_closed_on_newer_schema");
        assert_eq!(policy.uninstall, "retain_by_default");
        assert_eq!(policy.removal, "explicit_user_action_required");
    }
}
