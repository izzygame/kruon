use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kruon_desktop_lib::core::{AdapterHost, AdapterKind, RuntimeCore, StartRunRequest};
use sha2::{Digest, Sha256};

fn main() {
    if let Err(error) = execute() {
        eprintln!("w1_probe: {error}");
        std::process::exit(2);
    }
}

fn execute() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.iter().any(|arg| arg == "--allow-model-call") {
        return synthetic_probe();
    }

    let fixture = argument(&args, "--fixture-dir")
        .ok_or("real probe requires --fixture-dir inside the system temporary directory")?;
    let adapter = match argument(&args, "--adapter").as_deref() {
        Some("codex") => AdapterKind::Codex,
        Some("claude") => AdapterKind::Claude,
        _ => return Err("real probe requires --adapter codex|claude".into()),
    };
    let fixture = validate_probe_fixture(Path::new(&fixture))?;
    let readme = fixture.join("README.txt");
    let metadata = std::fs::symlink_metadata(&readme)
        .map_err(|_| "fixture must contain a regular README.txt".to_owned())?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("README.txt must be a regular non-symlink file".into());
    }

    let before = directory_fingerprint(&fixture)?;
    let database = std::env::temp_dir().join(format!("kruon-w1-{}.sqlite3", uuid::Uuid::new_v4()));
    let runtime = RuntimeCore::open(&database).map_err(|error| error.to_string())?;
    let snapshot = runtime
        .start(StartRunRequest {
            adapter,
            workspace_root: fixture.to_string_lossy().into_owned(),
            working_directory: fixture.to_string_lossy().into_owned(),
            prompt: "Read README.txt and return its verification marker. Do not create, edit, rename, or delete any file. Do not run commands.".into(),
            timeout_ms: Some(60_000),
            policy_id: Some("w1-read-only-probe".into()),
        })
        .map_err(|error| error.to_string())?;

    let deadline = Instant::now() + Duration::from_secs(70);
    let final_snapshot = loop {
        let current = runtime
            .get_run(&snapshot.run_id)
            .map_err(|error| error.to_string())?;
        if current.terminal_state.is_some() {
            break current;
        }
        if Instant::now() >= deadline {
            return Err("probe did not become terminal within 70 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let after = directory_fingerprint(&fixture)?;
    if before != after {
        return Err("fixture directory changed during the read-only probe".into());
    }
    let replay = runtime
        .replay_run(&snapshot.run_id)
        .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "run": final_snapshot,
            "events": replay.events,
            "fixture_unchanged": true,
            "database": database,
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn synthetic_probe() -> Result<(), String> {
    let host = AdapterHost;
    let codex = host.normalize_line(
        AdapterKind::Codex,
        "synthetic-codex",
        1,
        r#"{"type":"turn.completed","usage":{"input_tokens":1}}"#,
    );
    let claude = host.normalize_line(
        AdapterKind::Claude,
        "synthetic-claude",
        1,
        r#"{"type":"result","is_error":false,"api_key":"must-redact"}"#,
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "mode": "synthetic",
            "real_model_call": false,
            "events": [codex, claude],
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn argument(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn validate_probe_fixture(path: &Path) -> Result<PathBuf, String> {
    let fixture = path
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize fixture: {error}"))?;
    let temp = std::env::temp_dir()
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize temp directory: {error}"))?;
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repository = manifest
        .ancestors()
        .nth(3)
        .unwrap_or(manifest)
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize repository: {error}"))?;
    if !fixture.starts_with(&temp) {
        return Err(format!(
            "fixture {} is not inside system temp {}",
            fixture.display(),
            temp.display()
        ));
    }
    if fixture.starts_with(&repository) {
        return Err("repository paths are forbidden for real probes".into());
    }
    Ok(fixture)
}

fn directory_fingerprint(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    fingerprint_recursive(root, root, &mut result)?;
    Ok(result)
}

fn fingerprint_recursive(
    root: &Path,
    current: &Path,
    result: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(current).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned();
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "probe fixture contains forbidden symlink {relative}"
            ));
        }
        if metadata.is_dir() {
            result.insert(relative.clone(), "directory".into());
            fingerprint_recursive(root, &path, result)?;
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            result.insert(relative, format!("{:x}", Sha256::digest(bytes)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_path_is_rejected() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        assert!(validate_probe_fixture(repository).is_err());
    }

    #[test]
    fn temporary_fixture_is_accepted() {
        let fixture = tempfile::tempdir().unwrap();
        assert!(validate_probe_fixture(fixture.path()).is_ok());
    }
}
