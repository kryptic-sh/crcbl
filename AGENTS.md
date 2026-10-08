# Shell preference

On Windows, use Git Bash whenever it is available. Do not use PowerShell. If Git
Bash cannot launch, report the failure instead of silently switching to
PowerShell.

For command tools, select Git for Windows' `bin/sh.exe` with `login: false`.
Selecting `bin/bash.exe` may resolve to WindowsApps Bash or WSL instead.
