use crate::format::{executable_images, Arch};
use anyhow::{bail, ensure, Context, Result};
use regex::bytes::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Cli,
    App,
    Ide,
}
impl Kind {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "cli" => Ok(Self::Cli),
            "app" => Ok(Self::App),
            "ide" => Ok(Self::Ide),
            _ => bail!("Unknown target: {value}"),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::App => "app",
            Self::Ide => "ide",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Original,
    Patched,
}
#[derive(Clone, Debug)]
pub struct Edit {
    pub offset: usize,
    pub length: usize,
    pub bytes: Vec<u8>,
}
#[derive(Debug)]
pub struct Inspection {
    pub state: State,
    pub edits: Vec<Edit>,
}
struct Gate {
    arch: Arch,
    original: &'static str,
    patched: &'static str,
    replacement: &'static [u8],
    offset: usize,
    context: bool,
}

fn gates(kind: Kind) -> Vec<Gate> {
    if kind == Kind::Cli {
        return vec![
            Gate {
                arch: Arch::X64,
                original: r"\x48\x85\xc0\x0f\x84....\x80\x78\x08\x00\x0f\x85....\xe8....\x48\x89\x84\x24\x80\x00\x00\x00\x48\x89\x5c\x24\x50\x48\x89\x4c\x24\x70",
                patched: r"\x48\x85\xc0\x0f\x84....\x48\x85\xc0\x90\x0f\x85....\xe8....\x48\x89\x84\x24\x80\x00\x00\x00\x48\x89\x5c\x24\x50\x48\x89\x4c\x24\x70",
                replacement: b"\x48\x85\xc0\x90",
                offset: 9,
                context: false,
            },
            Gate {
                arch: Arch::Arm64,
                original: r"\x01\x20\x40\x39[\x01\x21\x41\x61\x81\xa1\xc1\xe1].[\x00-\x07]\x37...[\x94-\x97]\xe0\x4b\x00\xf9\xe1\x33\x00\xf9\xe2\x43\x00\xf9",
                patched: r"\x21\x00\x80\x52[\x01\x21\x41\x61\x81\xa1\xc1\xe1].[\x00-\x07]\x37...[\x94-\x97]\xe0\x4b\x00\xf9\xe1\x33\x00\xf9\xe2\x43\x00\xf9",
                replacement: b"\x21\x00\x80\x52",
                offset: 0,
                context: true,
            },
        ];
    }
    vec![
        Gate {
            arch: Arch::X64,
            original: r"\x80\x78\x08\x00\x74.\x48\x8b.\x24.\x48\x89.\x60",
            patched: r"\xc6\x40\x08\x01\x90\x90\x48\x8b.\x24.\x48\x89.\x60",
            replacement: b"\xc6\x40\x08\x01\x90\x90",
            offset: 0,
            context: false,
        },
        Gate {
            arch: Arch::X64,
            original: r"\x80\x78\x08\x00\x74\x3e\x48\x8b\x4c\x24\x78\x48\x89\x48\x40\x48\x8b\x8c\x24\x80\x00\x00\x00\x48\x89\x48\x48",
            patched: r"\xc6\x40\x08\x01\x90\x90\x48\x8b\x4c\x24\x78\x48\x89\x48\x40\x48\x8b\x8c\x24\x80\x00\x00\x00\x48\x89\x48\x48",
            replacement: b"\xc6\x40\x08\x01\x90\x90",
            offset: 0,
            context: false,
        },
        Gate {
            arch: Arch::Arm64,
            original: r"\x03\x20\x40\x39[\x03\x23\x43\x63\x83\xa3\xc3\xe3].[\x00-\x07]\x36(?:....){1,2}\x03\x10\x06\xa9",
            patched: r"\x23\x00\x80\x52\x03\x20\x00\x39(?:....){1,2}\x03\x10\x06\xa9",
            replacement: b"\x23\x00\x80\x52\x03\x20\x00\x39",
            offset: 0,
            context: false,
        },
    ]
}
fn regex(pattern: &str) -> Result<Regex> {
    Ok(RegexBuilder::new(pattern)
        .unicode(false)
        .dot_matches_new_line(true)
        .build()?)
}

pub fn inspect(data: &[u8], kind: Kind) -> Result<Inspection> {
    if kind == Kind::Ide {
        let original = regex(r"(resetIsTierGCPTos\(\),)this\.[A-Za-z_$0-9]+\.isGoogleInternal")?;
        let patched = regex(r"resetIsTierGCPTos\(\),true")?;
        let orig: Vec<_> = original.captures_iter(data).collect();
        let done = patched.find_iter(data).count();
        ensure!(
            orig.len() + done == 1,
            "IDE signature is missing, ambiguous or mixed"
        );
        if done == 1 {
            return Ok(Inspection {
                state: State::Patched,
                edits: vec![],
            });
        }
        let capture = &orig[0];
        let prefix = capture.get(1).unwrap();
        let full = capture.get(0).unwrap();
        return Ok(Inspection {
            state: State::Original,
            edits: vec![Edit {
                offset: prefix.end(),
                length: full.end() - prefix.end(),
                bytes: b"true".to_vec(),
            }],
        });
    }
    let images = executable_images(data)?;
    let mut state = None;
    let mut edits = Vec::new();
    let gates = gates(kind);
    for image in images {
        let mut matches = Vec::new();
        for gate in gates.iter().filter(|g| g.arch == image.arch) {
            for (pattern, candidate) in [
                (gate.original, State::Original),
                (gate.patched, State::Patched),
            ] {
                let re = regex(pattern)?;
                for range in &image.ranges {
                    for m in re.find_iter(&data[range.clone()]) {
                        let start = range.start + m.start();
                        if gate.context {
                            if m.start() < 8 {
                                continue;
                            }
                            let a = u32::from_le_bytes(data[start - 8..start - 4].try_into()?);
                            let b = u32::from_le_bytes(data[start - 4..start].try_into()?);
                            if a & 0xff00001f != 0xb5000001 || b & 0xff00001f != 0xb4000000 {
                                continue;
                            }
                        }
                        matches.push((
                            candidate,
                            Edit {
                                offset: start + gate.offset,
                                length: gate.replacement.len(),
                                bytes: gate.replacement.to_vec(),
                            },
                        ));
                    }
                }
            }
        }
        ensure!(
            matches.len() == 1,
            "Executable signature is missing, ambiguous or mixed ({} matches)",
            matches.len()
        );
        let (candidate, edit) = matches.remove(0);
        ensure!(
            state.is_none() || state == Some(candidate),
            "Universal executable is partially patched"
        );
        state = Some(candidate);
        edits.push(edit);
    }
    Ok(Inspection {
        state: state.context("No executable images")?,
        edits,
    })
}

pub fn patched_bytes(data: &[u8], kind: Kind) -> Result<Vec<u8>> {
    let mut inspection = inspect(data, kind)?;
    ensure!(
        inspection.state == State::Original,
        "Expected an unpatched original"
    );
    let mut output = data.to_vec();
    inspection
        .edits
        .sort_by_key(|edit| std::cmp::Reverse(edit.offset));
    for edit in inspection.edits {
        output.splice(edit.offset..edit.offset + edit.length, edit.bytes);
    }
    ensure!(
        inspect(&output, kind)?.state == State::Patched,
        "Patch verification failed"
    );
    Ok(output)
}
pub fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
pub fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub fn atomic_write(path: &Path, bytes: &[u8], permissions: Option<fs::Permissions>) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("No parent directory")?)?;
    temporary.write_all(bytes)?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    ensure!(
        fs::read(temporary.path())? == bytes,
        "Staged write verification failed"
    );
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    original: String,
    patched: String,
}

pub fn change(path: &Path, kind: Kind, restore: bool) -> Result<bool> {
    let current = fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?;
    let inspection = inspect(&current, kind)?;
    if inspection.state
        == if restore {
            State::Original
        } else {
            State::Patched
        }
    {
        return Ok(false);
    }
    crate::runtime::ensure_stopped(path, kind)?;
    let backup = sidecar(path, ".agybak");
    let record_path = sidecar(path, ".pagy.json");
    let replacement;
    if restore {
        replacement = fs::read(&backup).context("Original .agybak backup is missing")?;
        ensure!(
            inspect(&replacement, kind)?.state == State::Original,
            "Backup is not a recognized original"
        );
        if record_path.exists() {
            let record: Record = serde_json::from_slice(&fs::read(&record_path)?)?;
            ensure!(
                record.version == 1
                    && record.original == hash(&replacement)
                    && record.patched == hash(&current),
                "Backup or target changed since patching; refusing to restore"
            );
        } else {
            ensure!(
                patched_bytes(&replacement, kind)? == current,
                "Existing backup does not reconstruct the current patch"
            );
        }
    } else {
        replacement = patched_bytes(&current, kind)?;
        atomic_write(&backup, &current, Some(fs::metadata(path)?.permissions()))?;
    }
    let signing = crate::macos::prepare(path, restore)?;
    let permissions = fs::metadata(path)?.permissions();
    ensure!(
        fs::read(path)? == current,
        "Target changed during preparation"
    );
    crate::runtime::ensure_stopped(path, kind)?;
    atomic_write(path, &replacement, Some(permissions.clone()))?;
    let result = (|| -> Result<()> {
        crate::macos::finish(path, kind, restore, signing.as_ref())?;
        let final_bytes = fs::read(path)?;
        let desired = if restore {
            State::Original
        } else {
            State::Patched
        };
        ensure!(
            inspect(&final_bytes, kind)?.state == desired,
            "Post-write verification failed"
        );
        if !restore {
            let record = Record {
                version: 1,
                original: hash(&current),
                patched: hash(&final_bytes),
            };
            atomic_write(&record_path, &serde_json::to_vec(&record)?, None)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let binary_rollback = atomic_write(path, &current, Some(permissions));
        let signing_rollback = crate::macos::rollback(signing.as_ref());
        ensure!(
            binary_rollback.is_ok() && signing_rollback.is_ok(),
            "Patch failed: {error:#}; rollback failed, restore backup manually"
        );
        bail!("Operation failed and was rolled back: {error:#}");
    }
    if restore {
        let _ = fs::remove_file(record_path);
    }
    if kind == Kind::Ide {
        crate::runtime::clear_ide_cache();
    }
    Ok(true)
}
