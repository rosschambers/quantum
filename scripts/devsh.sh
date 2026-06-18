#!/usr/bin/env bash
# Run a command inside the nix-shell environment defined by shell.nix.
# shell.nix supplies the toolchain from the host nixpkgs: rustc/cargo
# (currently 1.95, tracks nixpkgs) plus GTK4, WebKit, and gtk4-layer-shell.
# Note: rust-toolchain.toml is NOT honored here (no rustup inside the shell);
# it only governs non-nix rustup users.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -eq 0 ]]; then
    exec nix-shell "${REPO_DIR}/shell.nix"
fi

# Build a single command string so nix-shell --run executes the user's
# command verbatim. Quote each arg so spaces survive.
CMD=""
for arg in "$@"; do
    CMD+=" $(printf '%q' "$arg")"
done

exec nix-shell --run "cd ${REPO_DIR} &&${CMD}" "${REPO_DIR}/shell.nix"
