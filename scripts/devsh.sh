#!/usr/bin/env bash
# Run a command inside the quantum dev container, building the image on first use.
# Mounts the repo at /workspaces/quantum so paths match the devcontainer config.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_TAG="quantum-dev:latest"
DOCKERFILE="${REPO_DIR}/.devcontainer/Dockerfile"

if ! docker image inspect "${IMAGE_TAG}" >/dev/null 2>&1; then
    echo "building ${IMAGE_TAG} ..." >&2
    docker build -t "${IMAGE_TAG}" -f "${DOCKERFILE}" "${REPO_DIR}/.devcontainer"
fi

if [[ $# -eq 0 ]]; then
    set -- bash
fi

USER_UID="$(id -u)"
USER_GID="$(id -g)"
TTY_FLAG=()
if [[ -t 0 && -t 1 ]]; then
    TTY_FLAG+=(-t)
fi

CARGO_CACHE_VOL="quantum-cargo-cache"
PNPM_CACHE_VOL="quantum-pnpm-store"

# Make sure the named volumes are owned by the host user so cargo/pnpm can write.
# We do this with a privileged one-shot run that runs only when the volume is fresh.
for vol_dir in "${CARGO_CACHE_VOL}:/cache/cargo" "${PNPM_CACHE_VOL}:/cache/pnpm"; do
    vol_name="${vol_dir%%:*}"
    mount_point="${vol_dir##*:}"
    if ! docker volume inspect "${vol_name}" >/dev/null 2>&1; then
        docker volume create "${vol_name}" >/dev/null
        docker run --rm -v "${vol_name}:${mount_point}" "${IMAGE_TAG}" \
            chown "${USER_UID}:${USER_GID}" "${mount_point}" >/dev/null
    fi
done

exec docker run --rm -i "${TTY_FLAG[@]}" \
    -u "${USER_UID}:${USER_GID}" \
    -v "${REPO_DIR}:/workspaces/quantum" \
    -v "${CARGO_CACHE_VOL}:/cache/cargo" \
    -v "${PNPM_CACHE_VOL}:/cache/pnpm" \
    -w /workspaces/quantum \
    -e CARGO_TERM_COLOR=always \
    -e RUST_BACKTRACE=1 \
    -e CARGO_HOME=/cache/cargo \
    -e PNPM_STORE_PATH=/cache/pnpm \
    -e HOME=/tmp \
    "${IMAGE_TAG}" \
    "$@"
