use crate::engine::Kind;
use anyhow::Result;
use std::path::Path;

#[cfg(not(target_os = "macos"))]
pub struct Session;
#[cfg(not(target_os = "macos"))]
pub fn prepare(_: &Path, _: bool) -> Result<Option<Session>> {
    Ok(None)
}
#[cfg(not(target_os = "macos"))]
pub fn finish(_: &Path, _: Kind, _: bool, _: Option<&Session>) -> Result<()> {
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn rollback(_: Option<&Session>) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub use native::*;

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use crate::engine::{atomic_write, hash, inspect, sidecar, State};
    use anyhow::{bail, ensure, Context};
    use serde::{Deserialize, Serialize};
    use std::{fs, path::PathBuf, process::Command};
    use tempfile::TempDir;

    pub struct Session {
        app: Option<PathBuf>,
        before: TempDir,
        original: Option<PathBuf>,
    }
    #[derive(Serialize, Deserialize)]
    struct Entry {
        relative: PathBuf,
        kind: String,
        digest: String,
    }

    fn checked(command: &mut Command) -> Result<()> {
        let output = command.output()?;
        ensure!(
            output.status.success(),
            "macOS command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }
    fn bundle(path: &Path) -> Option<PathBuf> {
        path.ancestors()
            .find(|p| p.extension().is_some_and(|x| x == "app") && p.join("Contents").is_dir())
            .map(Path::to_path_buf)
    }
    fn signature_paths(app: &Path) -> Result<Vec<PathBuf>> {
        let plist = plist::Value::from_file(app.join("Contents/Info.plist"))?;
        let name = plist
            .as_dictionary()
            .and_then(|d| d.get("CFBundleExecutable"))
            .and_then(|v| v.as_string())
            .context("Missing bundle executable")?;
        ensure!(
            Path::new(name).components().count() == 1
                && !name.contains(['/', '\\'])
                && name != "."
                && name != "..",
            "Invalid bundle executable"
        );
        Ok(vec![
            PathBuf::from("Contents/MacOS").join(name),
            PathBuf::from("Contents/_CodeSignature"),
            PathBuf::from("Contents/CodeResources"),
        ])
    }
    fn kind(path: &Path) -> &'static str {
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => "symlink",
            Ok(m) if m.is_dir() => "dir",
            Ok(_) => "file",
            Err(_) => "missing",
        }
    }
    fn tree_hash(path: &Path) -> Result<String> {
        let mut bytes = kind(path).as_bytes().to_vec();
        match kind(path) {
            "symlink" => {
                use std::os::unix::ffi::OsStrExt;
                bytes.extend_from_slice(fs::read_link(path)?.as_os_str().as_bytes());
            }
            "file" => {
                use std::os::unix::fs::PermissionsExt;
                bytes.extend_from_slice(&fs::metadata(path)?.permissions().mode().to_le_bytes());
                bytes.extend(fs::read(path)?);
            }
            "dir" => {
                let mut paths = fs::read_dir(path)?
                    .map(|e| e.map(|e| e.path()))
                    .collect::<std::io::Result<Vec<_>>>()?;
                paths.sort();
                for child in paths {
                    use std::os::unix::ffi::OsStrExt;
                    bytes.extend_from_slice(child.file_name().unwrap().as_bytes());
                    bytes.push(0);
                    bytes.extend_from_slice(tree_hash(&child)?.as_bytes());
                }
            }
            _ => (),
        }
        Ok(hash(&bytes))
    }
    fn remove(path: &Path) -> Result<()> {
        match kind(path) {
            "dir" => fs::remove_dir_all(path)?,
            "missing" => (),
            _ => fs::remove_file(path)?,
        }
        Ok(())
    }
    fn copy(source: &Path, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        match kind(source) {
            "symlink" => std::os::unix::fs::symlink(fs::read_link(source)?, dest)?,
            "dir" => {
                fs::create_dir_all(dest)?;
                for entry in fs::read_dir(source)? {
                    let entry = entry?;
                    copy(&entry.path(), &dest.join(entry.file_name()))?;
                }
            }
            "file" => {
                fs::copy(source, dest)?;
                fs::set_permissions(dest, fs::metadata(source)?.permissions())?;
            }
            _ => (),
        }
        Ok(())
    }
    fn validate(snapshot: &Path, app: &Path) -> Result<Vec<Entry>> {
        let entries: Vec<Entry> =
            serde_json::from_slice(&fs::read(snapshot.join("manifest.json"))?)?;
        let allowed = signature_paths(app)?;
        ensure!(entries.len() == allowed.len(), "Invalid signature snapshot");
        for (entry, relative) in entries.iter().zip(allowed) {
            ensure!(entry.relative == relative, "Unsafe signature snapshot path");
            let saved = snapshot.join("payload").join(&entry.relative);
            ensure!(
                kind(&saved) == entry.kind && tree_hash(&saved)? == entry.digest,
                "Signature backup verification failed"
            );
        }
        Ok(entries)
    }
    fn snapshot(app: &Path, dest: &Path) -> Result<()> {
        fs::create_dir_all(dest)?;
        let mut entries = Vec::new();
        for relative in signature_paths(app)? {
            let source = app.join(&relative);
            copy(&source, &dest.join("payload").join(&relative))?;
            entries.push(Entry {
                relative,
                kind: kind(&source).into(),
                digest: tree_hash(&source)?,
            });
        }
        atomic_write(
            &dest.join("manifest.json"),
            &serde_json::to_vec(&entries)?,
            None,
        )?;
        validate(dest, app)?;
        Ok(())
    }
    fn restore_snapshot(saved: &Path, app: &Path) -> Result<()> {
        for entry in validate(saved, app)? {
            let dest = app.join(&entry.relative);
            remove(&dest)?;
            copy(&saved.join("payload").join(&entry.relative), &dest)?;
            ensure!(
                tree_hash(&dest)? == entry.digest,
                "Signature restoration failed"
            );
        }
        Ok(())
    }
    pub fn prepare(path: &Path, restore: bool) -> Result<Option<Session>> {
        let before = tempfile::tempdir()?;
        let app = bundle(path);
        let mut original = None;
        if let Some(app) = &app {
            snapshot(app, before.path())?;
            let backup = sidecar(app, ".pagy-signature-backup");
            if restore {
                ensure!(backup.exists(), "Rust signature backup missing; restore a previous Python patch with the old tool first");
                validate(&backup, app)?;
            } else {
                let display = Command::new("/usr/bin/codesign")
                    .args(["-d", "--verbose=2"])
                    .arg(app)
                    .output()?;
                let valid = Command::new("/usr/bin/codesign")
                    .args(["--verify", "--deep", "--strict"])
                    .arg(app)
                    .output()?
                    .status
                    .success();
                let vendor_signed = valid
                    && display.status.success()
                    && !String::from_utf8_lossy(&display.stderr)
                        .to_lowercase()
                        .contains("signature=adhoc");
                if !backup.exists() || vendor_signed {
                    ensure!(vendor_signed, "Bundle is already modified; restore its original signing state before first Rust patch");
                    let stage = tempfile::tempdir_in(app.parent().context("No bundle parent")?)?;
                    snapshot(app, &stage.path().join("new"))?;
                    if backup.exists() {
                        fs::rename(&backup, stage.path().join("old"))?;
                    }
                    if let Err(error) = fs::rename(stage.path().join("new"), &backup) {
                        if stage.path().join("old").exists() {
                            fs::rename(stage.path().join("old"), &backup)?;
                        }
                        return Err(error.into());
                    }
                } else {
                    validate(&backup, app)?;
                }
            }
            original = Some(backup);
        }
        Ok(Some(Session {
            app,
            before,
            original,
        }))
    }
    fn sign(path: &Path, bundle: bool) -> Result<()> {
        let existing = Command::new("/usr/bin/codesign")
            .args(["-d", "--verbose=2"])
            .arg(path)
            .output()?
            .status
            .success();
        let mut cmd = Command::new("/usr/bin/codesign");
        cmd.args(["--force", "--sign", "-"]);
        if existing {
            cmd.arg("--preserve-metadata=identifier,entitlements,flags,runtime");
        }
        checked(cmd.arg(path))?;
        let mut verify = Command::new("/usr/bin/codesign");
        verify.arg("--verify");
        if bundle {
            verify.arg("--deep");
        }
        checked(verify.args(["--strict", "--verbose=2"]).arg(path))
    }
    pub fn finish(
        path: &Path,
        target: Kind,
        restore: bool,
        session: Option<&Session>,
    ) -> Result<()> {
        let session = session.context("Missing signing transaction")?;
        if !restore && target != Kind::Ide {
            sign(path, false)?;
        }
        if let Some(app) = &session.app {
            let mut other_patched = false;
            for (relative, kind) in [
                ("Contents/Resources/bin/language_server", Kind::App),
                ("Contents/Resources/app/out/main.js", Kind::Ide),
            ] {
                let candidate = app.join(relative);
                if candidate != path && candidate.is_file() {
                    let status = inspect(&fs::read(&candidate)?, kind)
                        .context("Cannot validate sibling bundle target")?;
                    other_patched |= status.state == State::Patched;
                }
            }
            if restore && !other_patched {
                restore_snapshot(
                    session
                        .original
                        .as_ref()
                        .context("Missing original signing snapshot")?,
                    app,
                )?;
                checked(
                    Command::new("/usr/bin/codesign")
                        .args(["--verify", "--deep", "--strict"])
                        .arg(app),
                )?;
            } else {
                sign(app, true)?;
            }
        }
        if !restore {
            let target = session.app.as_deref().unwrap_or(path);
            let result = Command::new("/usr/bin/xattr")
                .args(["-dr", "com.apple.quarantine"])
                .arg(target)
                .output()?;
            if !result.status.success()
                && !String::from_utf8_lossy(&result.stderr).contains("No such xattr")
            {
                bail!(
                    "Could not remove quarantine: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
        }
        Ok(())
    }
    pub fn rollback(session: Option<&Session>) -> Result<()> {
        if let Some(session) = session {
            if let Some(app) = &session.app {
                restore_snapshot(session.before.path(), app)?;
            }
        }
        Ok(())
    }
}
