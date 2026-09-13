use crate::engine::{self, Kind, State};
use anyhow::{bail, ensure, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
    time::UNIX_EPOCH,
};

pub fn state_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("PAGY_STATE_DIR") {
        return Ok(path.into());
    }
    Ok(dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .context("No user state directory")?
        .join("antigravity-region-fix"))
}
pub fn lock() -> Result<File> {
    let directory = state_dir()?;
    fs::create_dir_all(&directory)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("patch.lock"))?;
    FileExt::try_lock_exclusive(&file).context("Another patch/restore operation is running")?;
    Ok(file)
}

pub fn resolve(kind: Kind, explicit: Option<PathBuf>) -> Result<PathBuf> {
    let key = match kind {
        Kind::Cli => "PAGY_AGY",
        Kind::App => "PAGY_APP",
        Kind::Ide => "PAGY_IDE",
    };
    if let Some(path) = explicit.or_else(|| env::var_os(key).map(PathBuf::from)) {
        ensure!(path.is_file(), "Not a file: {}", path.display());
        return Ok(path.canonicalize()?);
    }
    if kind == Kind::Cli {
        if let Some(paths) = env::var_os("PATH") {
            for directory in env::split_paths(&paths) {
                let path = directory.join(if cfg!(windows) { "agy.exe" } else { "agy" });
                if path.is_file() {
                    return Ok(path.canonicalize()?);
                }
            }
        }
    }
    let home = dirs::home_dir().context("No home directory")?;
    if cfg!(target_os = "linux") && kind != Kind::Cli {
        let path = home
            .join(".local/share/antigravity-patched")
            .join(if kind == Kind::App {
                "Antigravity/resources/bin/language_server"
            } else {
                "Antigravity IDE/resources/app/out/main.js"
            });
        if path.is_file() {
            return Ok(path.canonicalize()?);
        }
    }
    let mut roots = vec![
        home.join(".local/share"),
        home.join("Applications"),
        PathBuf::from("/Applications"),
        PathBuf::from("/opt"),
        PathBuf::from("/usr/share"),
        PathBuf::from("/usr/local/share"),
    ];
    if cfg!(windows) {
        for variable in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = env::var_os(variable) {
                roots.push(root.into());
            }
        }
        roots.push(home.join("scoop/apps"));
    }
    let suffixes: Vec<&str> = match kind {
        Kind::Cli => {
            if cfg!(windows) {
                vec!["agy.exe"]
            } else {
                vec!["agy"]
            }
        }
        Kind::App => {
            if cfg!(windows) {
                vec!["resources/bin/language_server.exe"]
            } else if cfg!(target_os = "macos") {
                vec!["Contents/Resources/bin/language_server"]
            } else {
                vec!["resources/bin/language_server"]
            }
        }
        Kind::Ide => {
            if cfg!(target_os = "macos") {
                vec!["Contents/Resources/app/out/main.js"]
            } else {
                vec!["resources/app/out/main.js"]
            }
        }
    };
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let mut candidates = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.contains("antigravity") || name == "agy" {
                candidates.push(entry.path());
            }
            if cfg!(windows) && name == "programs" {
                for child in fs::read_dir(entry.path())?.flatten() {
                    let name = child.file_name().to_string_lossy().to_lowercase();
                    if name.contains("antigravity") || name == "agy" {
                        candidates.push(child.path());
                    }
                }
            }
        }
        for candidate in candidates {
            for entry in walkdir::WalkDir::new(candidate)
                .max_depth(8)
                .into_iter()
                .filter_map(Result::ok)
            {
                if entry.file_type().is_file()
                    && suffixes.iter().any(|suffix| entry.path().ends_with(suffix))
                {
                    return Ok(entry.path().canonicalize()?);
                }
            }
        }
    }
    bail!("{} not found. Set {key} to the target file", kind.name())
}

pub fn process_root(path: &Path, kind: Kind) -> PathBuf {
    if cfg!(target_os = "macos") {
        if let Some(app) = path
            .ancestors()
            .find(|p| p.extension().is_some_and(|x| x == "app"))
        {
            return app.into();
        }
    }
    let levels = match kind {
        Kind::Cli => 0,
        Kind::App => 3,
        Kind::Ide => 4,
    };
    path.ancestors().nth(levels).unwrap_or(path).into()
}
pub fn ensure_stopped(path: &Path, kind: Kind) -> Result<()> {
    let root = process_root(path, kind);
    let matches = |exe: &Path| exe == root || (kind != Kind::Cli && exe.starts_with(&root));
    #[cfg(target_os = "linux")]
    {
        for entry in fs::read_dir("/proc")?.flatten() {
            if let Ok(exe) = fs::read_link(entry.path().join("exe")) {
                ensure!(
                    !matches(&exe),
                    "Close all running {} processes before modifying files",
                    kind.name()
                );
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let result = Command::new("ps").args(["-axo", "comm="]).output()?;
        ensure!(
            result.status.success(),
            "Could not inspect running applications"
        );
        for line in String::from_utf8_lossy(&result.stdout).lines() {
            ensure!(
                !matches(Path::new(line.trim())),
                "Close {} first",
                kind.name()
            );
        }
    }
    #[cfg(windows)]
    {
        let result = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-CimInstance Win32_Process | Select-Object -ExpandProperty ExecutablePath",
            ])
            .output()?;
        ensure!(
            result.status.success(),
            "Could not inspect running applications"
        );
        let root = root
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_lowercase();
        for line in String::from_utf8_lossy(&result.stdout).lines() {
            let exe = line.trim().to_lowercase();
            ensure!(
                exe != root && !(kind != Kind::Cli && exe.starts_with(&(root.clone() + "\\"))),
                "Close {} first",
                kind.name()
            );
        }
        let _ = matches;
    }
    Ok(())
}
pub fn clear_ide_cache() {
    if let Some(config) = dirs::config_dir() {
        for relative in ["CachedData", "Code Cache/js"] {
            let _ = fs::remove_dir_all(config.join("Antigravity IDE").join(relative));
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Stamp {
    version: String,
    len: u64,
    modified: u128,
    identity: Vec<u64>,
}
fn stamp(path: &Path) -> Result<Stamp> {
    let meta = fs::metadata(path)?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        vec![
            meta.dev(),
            meta.ino(),
            meta.ctime() as u64,
            meta.ctime_nsec() as u64,
        ]
    };
    #[cfg(windows)]
    let identity = {
        use std::os::windows::fs::MetadataExt;
        vec![
            meta.creation_time(),
            meta.last_write_time(),
            meta.file_attributes() as u64,
        ]
    };
    Ok(Stamp {
        version: env!("CARGO_PKG_VERSION").into(),
        len: meta.len(),
        modified: meta.modified()?.duration_since(UNIX_EPOCH)?.as_nanos(),
        identity,
    })
}
fn cache_path(path: &Path) -> Result<PathBuf> {
    Ok(state_dir()?.join(format!(
        "cli-{}.json",
        engine::hash(path.as_os_str().as_encoded_bytes())
    )))
}
pub fn prepare_cli(path: &Path) -> Result<()> {
    let _guard = lock()?;
    let initial = stamp(path)?;
    let cached = cache_path(path)?;
    if env::var_os("PAGY_NO_CACHE").is_none() {
        if let Ok(bytes) = fs::read(&cached) {
            if serde_json::from_slice::<Stamp>(&bytes).ok().as_ref() == Some(&initial) {
                return Ok(());
            }
        }
    }
    let changed = engine::change(path, Kind::Cli, false)?;
    if changed {
        eprintln!("pagy: compatible CLI patch applied; original saved as .agybak");
    }
    let final_stamp = stamp(path)?;
    if !changed {
        ensure!(
            initial == final_stamp,
            "CLI changed during inspection; retry pagy"
        );
    }
    engine::atomic_write(&cached, &serde_json::to_vec(&final_stamp)?, None)?;
    Ok(())
}
pub fn launch(args: Vec<OsString>) -> Result<()> {
    let path = resolve(Kind::Cli, None)?;
    prepare_cli(&path)?;
    let mut command = Command::new(path);
    command.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec().into())
    }
    #[cfg(windows)]
    {
        let code = command.status()?.code().unwrap_or(1);
        std::process::exit(code);
    }
}

pub fn manage(args: Vec<OsString>) -> Result<()> {
    if args
        .first()
        .is_some_and(|a| a == "install" || a == "uninstall")
    {
        let uninstall = args[0] == "uninstall";
        return crate::install::run(args.into_iter().skip(1).collect(), uninstall);
    }
    let mut explicit = std::collections::HashMap::new();
    let mut words = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!("antigravity-region-fix [--path-cli FILE] [--path-app FILE] [--path-ide FILE] <status|patch|restore> [all|cli|app|ide]\nUse pagy to patch and launch agy with all original arguments.\nExperimental software; MIT; no warranty.");
            return Ok(());
        }
        if arg == "--version" {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        if let Some(kind) = [
            ("--path-cli", Kind::Cli),
            ("--path-app", Kind::App),
            ("--path-ide", Kind::Ide),
        ]
        .iter()
        .find(|(name, _)| arg == *name)
        .map(|x| x.1)
        {
            explicit.insert(
                kind.name(),
                PathBuf::from(args.next().context("Missing path value")?),
            );
        } else {
            words.push(arg);
        }
    }
    ensure!(
        !words.is_empty() && words.len() <= 2,
        "Expected <status|patch|restore> [all|cli|app|ide]; use --help"
    );
    let action = words[0].to_str().context("Invalid action")?;
    ensure!(
        ["status", "patch", "restore"].contains(&action),
        "Unknown action: {action}"
    );
    let target = words
        .get(1)
        .map(|v| v.to_str().context("Invalid target"))
        .transpose()?
        .unwrap_or("all");
    let kinds = if target == "all" {
        vec![Kind::Cli, Kind::App, Kind::Ide]
    } else {
        vec![Kind::parse(target)?]
    };
    let _guard = lock()?;
    let mut errors = Vec::new();
    for kind in kinds {
        let result = (|| -> Result<()> {
            let path = resolve(kind, explicit.remove(kind.name()))?;
            if action != "status" {
                engine::change(&path, kind, action == "restore")?;
                if kind == Kind::Cli {
                    let _ = fs::remove_file(cache_path(&path)?);
                }
            }
            let state = engine::inspect(&fs::read(&path)?, kind)?.state;
            println!(
                "{}: {} — {}",
                kind.name(),
                if state == State::Patched {
                    "patched"
                } else {
                    "unpatched"
                },
                path.display()
            );
            Ok(())
        })();
        if let Err(error) = result {
            errors.push(format!("{}: {error:#}", kind.name()));
        }
    }
    ensure!(errors.is_empty(), "{}", errors.join("\n"));
    Ok(())
}
