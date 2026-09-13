# Third-party notices

The Rust signature definitions and patch/signing workflow are derived from [QNIX-Dev/eligibility-antigravity-patcher](https://github.com/QNIX-Dev/eligibility-antigravity-patcher), commit `8ea0f5ed0c60affa78623bbcdda44a6cb3bc60f8`, MIT, Copyright (c) 2026 QNIX-Dev. Its original license is preserved in `licenses/UPSTREAM-MIT.txt`.

The v0.1.0 Python snapshot had SHA-256 `c4a235de0f36fb15ac6bfcd7c4c371a454e3a4e2652f4fb3275b9f1e80908c98`. It remains in Git history; the current executables do not invoke or embed Python. Rust code, native installation, metadata caching, argument forwarding, the old Linux Hub signature integration and tests are maintained by Egor and contributors.

`Cargo.lock` pins Rust dependencies. Their copyright/license notices are collected in `licenses/DEPENDENCIES.txt` for distribution with native binaries. Individual dependency licenses continue to apply.

No Google executables, credentials, account data or personal logs are distributed. Synthetic test fixtures are not application binaries. Antigravity and Google names belong to their respective owners. This project is independent of Google.
