# Shell preference

On Windows, use Git Bash whenever it is available. Do not use PowerShell. If Git
Bash cannot launch, report the failure instead of silently switching to
PowerShell.

For command tools, select Git for Windows' `bin/sh.exe` with `login: false`.
Selecting `bin/bash.exe` may resolve to WindowsApps Bash or WSL instead.

# Error types

Every error type in this project derives `crcbl_error::Error`, the project's own
derive. Never add `thiserror`, `anyhow`, `snafu` or any other third-party error
crate to a crate's manifest. `thiserror` came in by oversight and is being
replaced across the workspace. Until `crcbl-error` exists (`docs/backlog.md`,
"`crcbl-error` replaces `thiserror` workspace-wide"), new error types in a crate
that does not already depend on `thiserror` implement `Display` and
`std::error::Error` by hand.

More generally, check what the project already provides before reaching for a
third-party crate, and a new dependency is always the user's call.
