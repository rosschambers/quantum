#!/usr/bin/env bash
# Run a command inside the nix-shell environment defined by shell.nix.
# Replaces the docker-based devcontainer; shell.nix provides the same
# toolchain (Rust 1.85 + GTK4 + WebKit + gtk4-layer-shell) without the
# container-rebuild cost.
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
