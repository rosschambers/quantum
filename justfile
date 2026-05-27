# All targets run inside the dev container via scripts/devsh.sh.
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

ts-bindgen:
    {{devsh}} cargo run --bin quantum-dev -- generate-ts

frontend-install:
    {{devsh}} pnpm -C frontend install

frontend-build:
    {{devsh}} pnpm -C frontend -r build

frontend-test:
    {{devsh}} pnpm -C frontend -r test

shell:
    {{devsh}}
