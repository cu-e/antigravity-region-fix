# Validation

Date: September 13, 2026. Version: 0.2.0.

## Linux x64

- `cargo test --locked`: 19 tests passed (2 unit tests and 17 integration tests).
- `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings`: passed.
- Native release build and installation succeeded. Installed `pagy --version` returns `1.2.2`; status recognizes all three installed clients as patched.
- Copies of the actual CLI, desktop language server and IDE entry point were patched and restored. Restored SHA-256 hashes exactly matched the originals.
- The real CLI's `--help` was checked through the wrapper; repeated launch output and exit status matched direct invocation.

Tests cover executable-range selection for ELF, PE and Mach-O; x64 and ARM64 signatures; consistent universal slices; malformed, ambiguous and unknown inputs; backup integrity and rollback; update detection and cache invalidation; lock contention; reversible installation and rejection of unrelated commands.

A compiled native probe verifies argument forwarding (including empty, Unicode, quoted, multiline and Unix non-UTF-8 arguments), stdin/stdout/stderr, working directory, environment and exit status.

## Startup benchmark

Fifteen interleaved, warmed `--version` runs per command on the same local machine and CLI:

| Command | Median elapsed time |
| --- | ---: |
| Direct `agy` | 56.346 ms |
| Previous Python `pagy` | 232.875 ms |
| Rust `pagy` | 56.537 ms |

The warmed Rust command was about 4.1 times faster than the previous wrapper in this test. These are whole-process timings, not model inference timings. Cold scans, patch operations and other machines will differ; the small difference from direct invocation is within measurement noise.

## Other platforms

Cross-target checks of all targets passed for Windows x64 MSVC, macOS x64 and macOS ARM64. Clippy with warnings denied also passed for Windows x64 MSVC and macOS ARM64.

Windows/macOS execution, macOS signing and Linux ARM64 execution have **not been validated on native systems here**. Executable-format fixtures do not establish native runtime compatibility. GitHub Actions is configured to test and build on Linux, Windows and macOS, but has not run for this local repository.

These checks establish local patch and launcher behavior, not future model availability, compatibility with unknown client versions or provider approval.
