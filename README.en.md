> **Warning.** This software is provided for research and diagnostic purposes only, AS IS, without warranty of any kind. You are solely responsible for compliance with applicable laws and service terms.

# Antigravity Regional Fix

[Русский](README.ru.md) · [MIT](LICENSE) · [Credits](THIRD_PARTY_NOTICES.md) · [Validation](TESTING.md)

A tool to bypass client-side regional restrictions in Antigravity (CLI, Desktop App, and IDE).

## Components

- **`pagy`** — Transparent wrapper for the `agy` CLI. Verifies and applies patches on launch, forwarding all arguments to the original binary.
- **`antigravity-region-fix`** — Management utility to inspect, patch, and restore the CLI, desktop application, and IDE.

## Installation

### Release Bundle

Download the bundle for your platform and run the installer:

```sh
# Linux / macOS
sh install.sh
```

```powershell
# Windows
.\install.ps1
```

Default install locations:

- Linux / macOS: `~/.local/bin`
- Windows: `%LOCALAPPDATA%\Programs\AntigravityRegionFix\bin`

To install to a custom directory, use `--bin-dir`:

```sh
sh install.sh --bin-dir /path/to/bin
```

### Build from Source

Requires Rust 1.88+:

```sh
cargo build --release --locked --bins
./target/release/antigravity-region-fix install
```

On Windows:

```powershell
.\target\release\antigravity-region-fix.exe install
```

## Usage

### CLI Wrapper

Use `pagy` as a drop-in replacement for `agy`. All arguments and flags are forwarded directly:

```sh
pagy
pagy --help
pagy models
pagy --mode plan --print "Hello"
```

### Patch Management

Close target applications and active CLI sessions before patching or restoring.

```sh
# Check status
antigravity-region-fix status

# Apply patches
antigravity-region-fix patch cli
antigravity-region-fix patch app
antigravity-region-fix patch ide

# Restore originals
antigravity-region-fix restore all
# or individually: restore cli | app | ide

# Uninstall
antigravity-region-fix uninstall
```

## Paths & Environment Variables

Standard installation paths are detected automatically. For custom locations, use flags or environment variables:

| Environment Variable | CLI Flag     | Target                                        |
| -------------------- | ------------ | --------------------------------------------- |
| `PAGY_AGY`           | `--path-cli` | Path to `agy` executable                      |
| `PAGY_APP`           | `--path-app` | Path to `resources/bin/language_server` (app) |
| `PAGY_IDE`           | `--path-ide` | Path to `resources/app/out/main.js` (IDE)     |
| `PAGY_STATE_DIR`     | —            | State directory (locks, manifest, cache)      |
| `PAGY_NO_CACHE`      | —            | Force full signature scan without cache (`1`) |

## Backups & Integrity

- Original files are preserved alongside targets with a `.agybak` extension.
- SHA-256 hashes are tracked in `.pagy.json` for reliable restoration.
- On macOS, modified binaries and bundles are automatically ad-hoc signed and cleared of quarantine attributes.

## Development

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked --bins
```
