use crate::{
    engine::{atomic_write, hash},
    runtime,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

const WARNING: &str = "Experimental software for educational and diagnostic use. Provided AS IS, without warranty.\nTo the extent permitted by law, authors disclaim liability. You are responsible for service terms and applicable law.";
#[derive(Serialize, Deserialize)]
struct Installed {
    path: PathBuf,
    digest: Option<String>,
    original: Option<PathBuf>,
    original_hash: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    bin_dir: PathBuf,
    files: Vec<Installed>,
}

fn default_bin() -> Result<PathBuf> {
    if cfg!(windows) {
        Ok(dirs::data_local_dir()
            .context("No local data directory")?
            .join("Programs/AntigravityRegionFix/bin"))
    } else {
        Ok(dirs::home_dir()
            .context("No home directory")?
            .join(".local/bin"))
    }
}
fn owned_legacy(path: &Path) -> bool {
    fs::read(path)
        .ok()
        .filter(|data| data.len() < 8192)
        .is_some_and(|data| {
            String::from_utf8_lossy(&data).contains("# antigravity-region-fix launcher")
        })
}
fn check_file(entry: &Installed) -> Result<()> {
    match &entry.digest {
        Some(digest) => ensure!(
            entry.path.is_file() && hash(&fs::read(&entry.path)?) == *digest,
            "Installed command changed: {}",
            entry.path.display()
        ),
        None => ensure!(
            !entry.path.exists(),
            "Unexpected command appeared: {}",
            entry.path.display()
        ),
    }
    Ok(())
}
pub fn run(args: Vec<OsString>, uninstall: bool) -> Result<()> {
    println!("{WARNING}");
    let mut args = args.into_iter();
    let mut bin = None;
    while let Some(arg) = args.next() {
        ensure!(arg == "--bin-dir", "Expected --bin-dir PATH");
        bin = Some(PathBuf::from(
            args.next().context("Missing --bin-dir value")?,
        ));
    }
    let _lock = runtime::lock()?;
    let manifest_path = runtime::state_dir()?.join("installation-rust.json");
    if uninstall {
        ensure!(
            bin.is_none(),
            "Uninstall uses the recorded installation directory"
        );
        let manifest: Manifest = serde_json::from_slice(
            &fs::read(&manifest_path).context("Rust installation manifest not found")?,
        )?;
        ensure!(manifest.version == 1, "Unsupported installation manifest");
        for entry in &manifest.files {
            check_file(entry)?;
            if let Some(original) = &entry.original {
                ensure!(
                    Some(hash(&fs::read(original)?)) == entry.original_hash,
                    "Installer backup changed: {}",
                    original.display()
                );
            }
        }
        let previous = manifest
            .files
            .iter()
            .map(|entry| -> Result<_> {
                Ok(if entry.path.exists() {
                    Some((
                        fs::read(&entry.path)?,
                        fs::metadata(&entry.path)?.permissions(),
                    ))
                } else {
                    None
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut changed = 0;
        let result = (|| -> Result<()> {
            for entry in &manifest.files {
                if let Some(original) = &entry.original {
                    atomic_write(
                        &entry.path,
                        &fs::read(original)?,
                        Some(fs::metadata(original)?.permissions()),
                    )?;
                } else if entry.path.exists() {
                    fs::remove_file(&entry.path)?;
                }
                changed += 1;
            }
            fs::remove_file(&manifest_path)?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut failed = false;
            for (entry, previous) in manifest.files[..changed]
                .iter()
                .zip(&previous[..changed])
                .rev()
            {
                let result = match previous {
                    Some((bytes, permissions)) => {
                        atomic_write(&entry.path, bytes, Some(permissions.clone()))
                    }
                    None => fs::remove_file(&entry.path).map_err(Into::into),
                };
                failed |= result.is_err();
            }
            ensure!(
                !failed,
                "Uninstall and rollback failed; installation backups are retained"
            );
            return Err(error.context("Uninstall rolled back"));
        }
        println!("Rust commands removed; previous launchers restored where present. Antigravity patches and backups were retained.");
        return Ok(());
    }
    let bin = bin.unwrap_or(default_bin()?);
    fs::create_dir_all(&bin)?;
    let bin = bin.canonicalize()?;
    let source = env::current_exe()?
        .parent()
        .context("No source executable directory")?
        .to_owned();
    let old: Option<Manifest> = if manifest_path.exists() {
        Some(serde_json::from_slice(&fs::read(&manifest_path)?)?)
    } else {
        None
    };
    if let Some(old) = &old {
        ensure!(
            old.version == 1 && old.bin_dir == bin,
            "Uninstall before changing the command directory"
        );
        for entry in &old.files {
            check_file(entry)?;
        }
    }
    let mut replacements: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    for name in ["pagy", "antigravity-region-fix"] {
        let name = format!("{name}{}", env::consts::EXE_SUFFIX);
        let from = source.join(&name);
        ensure!(
            from.is_file(),
            "Build both binaries first; missing {}",
            from.display()
        );
        replacements.push((bin.join(name), Some(from)));
    }
    if cfg!(windows) {
        for name in [
            "pagy.ps1",
            "pagy.cmd",
            "antigravity-region-fix.ps1",
            "antigravity-region-fix.cmd",
        ] {
            let path = bin.join(name);
            if path.exists() {
                ensure!(
                    owned_legacy(&path),
                    "Refusing to remove an unrelated shim: {}",
                    path.display()
                );
                replacements.push((path, None));
            }
        }
    }
    let backups = runtime::state_dir()?.join("installer-originals");
    fs::create_dir_all(&backups)?;
    let mut manifest = Manifest {
        version: 1,
        bin_dir: bin.clone(),
        files: vec![],
    };
    let mut staged = Vec::new();
    for (path, source) in &replacements {
        ensure!(
            !path.is_symlink(),
            "Refusing to replace a symbolic link: {}",
            path.display()
        );
        let prior = old
            .as_ref()
            .and_then(|m| m.files.iter().find(|entry| entry.path == *path));
        if path.exists() && prior.is_none() {
            ensure!(
                owned_legacy(path),
                "Refusing to overwrite an unrelated command: {}",
                path.display()
            );
        }
        let current = if path.exists() {
            Some((fs::read(path)?, fs::metadata(path)?.permissions()))
        } else {
            None
        };
        let (original, original_hash) = if let Some(prior) = prior {
            (prior.original.clone(), prior.original_hash.clone())
        } else if let Some((data, permissions)) = &current {
            let saved = backups.join(format!(
                "{}-{}",
                hash(path.as_os_str().as_encoded_bytes()),
                hash(data)
            ));
            atomic_write(&saved, data, Some(permissions.clone()))?;
            (Some(saved), Some(hash(data)))
        } else {
            (None, None)
        };
        let replacement = source
            .as_ref()
            .map(|p| -> Result<_> { Ok((fs::read(p)?, fs::metadata(p)?.permissions())) })
            .transpose()?;
        manifest.files.push(Installed {
            path: path.clone(),
            digest: replacement.as_ref().map(|(bytes, _)| hash(bytes)),
            original,
            original_hash,
        });
        staged.push((path.clone(), current, replacement));
    }
    if let Some(old) = &old {
        for entry in &old.files {
            if !manifest.files.iter().any(|new| new.path == entry.path) {
                manifest.files.push(Installed {
                    path: entry.path.clone(),
                    digest: entry.digest.clone(),
                    original: entry.original.clone(),
                    original_hash: entry.original_hash.clone(),
                });
            }
        }
    }
    let mut committed = 0;
    let result = (|| -> Result<()> {
        for (path, _, replacement) in &staged {
            match replacement {
                Some((bytes, permissions)) => atomic_write(path, bytes, Some(permissions.clone()))?,
                None => fs::remove_file(path)?,
            }
            committed += 1;
        }
        atomic_write(&manifest_path, &serde_json::to_vec_pretty(&manifest)?, None)?;
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_failed = false;
        for (path, previous, _) in staged[..committed].iter().rev() {
            let result = match previous {
                Some((bytes, permissions)) => atomic_write(path, bytes, Some(permissions.clone())),
                None => fs::remove_file(path).map_err(Into::into),
            };
            rollback_failed |= result.is_err();
        }
        ensure!(
            !rollback_failed,
            "Installation and rollback failed; originals are in {}",
            backups.display()
        );
        return Err(error.context("Installation rolled back"));
    }
    println!("Installed native commands in {}", bin.display());
    if !env::var_os("PATH").is_some_and(|paths| env::split_paths(&paths).any(|p| p == bin)) {
        println!(
            "Add this directory to your user PATH and open a new terminal: {}",
            bin.display()
        );
    }
    println!("Try: pagy --help");
    Ok(())
}
