# All targets run inside the nix-shell via scripts/devsh.sh.
# This keeps the build environment hermetic and identical across machines.
# For an interactive shell, use `just shell`.

devsh := "./scripts/devsh.sh"

default: build

build:
    {{devsh}} cargo build --workspace

test:
    {{devsh}} cargo test --workspace

fmt:
    {{devsh}} cargo fmt --all

lint:
    {{devsh}} cargo clippy --workspace --all-targets -- -D warnings

dev:
    {{devsh}} cargo run --bin quantumd

# TS bindings are committed manually until specta is wired

frontend-install:
    {{devsh}} pnpm -C src/ui install

frontend-build:
    {{devsh}} pnpm -C src/ui -r --parallel build

frontend-test:
    {{devsh}} pnpm -C src/ui -r test

shell:
    {{devsh}}
