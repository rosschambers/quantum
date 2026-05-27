default: build

build:
    cargo build --workspace

test:
    cargo test --workspace

fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets -- -D warnings

dev:
    cargo run --bin quantumd

ts-bindgen:
    cargo run --bin quantum-dev -- generate-ts
