# Build + dev shell for the Quantum workspace. Used by `scripts/devsh.sh` to
# run all builds, tests, lint, and fmt commands. Run `nix-shell` directly to
# drop into an interactive shell.

{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    pkg-config
    rustc
    cargo
    rustfmt
    clippy
    mold
    # The clipboard subsystem shells out to wl-copy / wl-paste (the wlr-data-control
    # tools). Pin them here so the dependency is reproducible in the dev/CI shell
    # rather than relying on whatever is in the user profile.
    wl-clipboard
  ];

  buildInputs = with pkgs; [
    glib
    gtk4
    webkitgtk_6_0
    gtk4-layer-shell
    cairo
    pango
    gdk-pixbuf
    graphene
    libsoup_3
    wayland
    wayland-protocols
    libGL
  ];

  # The Rust crate `gtk4-layer-shell = "0.5"` looks up `gtk4-layer-shell` via
  # pkg-config, but upstream installs the file as `gtk4-layer-shell-0.pc`. Shim
  # in a STABLE, repo-local directory with the alias and prepend it to
  # PKG_CONFIG_PATH. The directory path must be deterministic across shell
  # entries: a fresh `mktemp -d` each time changes PKG_CONFIG_PATH, which
  # invalidates the gtk/webkit `-sys` build-script fingerprints and forces a
  # full recompile on every devsh.sh invocation. `.pkgconfig-shim/` is
  # gitignored.
  shellHook = ''
    PKG_CONFIG_SHIM="$PWD/.pkgconfig-shim"
    mkdir -p "$PKG_CONFIG_SHIM"
    ln -sf "${pkgs.gtk4-layer-shell.dev}/lib/pkgconfig/gtk4-layer-shell-0.pc" \
           "$PKG_CONFIG_SHIM/gtk4-layer-shell.pc"
    export PKG_CONFIG_PATH="$PKG_CONFIG_SHIM:$PKG_CONFIG_PATH"

    # Use the mold linker for the link-heavy gtk/webkit tree. This is set here
    # (only inside the dev/CI nix-shell where mold is on PATH) rather than in a
    # committed .cargo/config.toml, so packaged/downstream cargo builds that do
    # not source shell.nix still link with the default linker and do not require
    # mold to be installed.
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"

    echo "quantum host shell ready"
    echo "  rustc:           $(rustc --version)"
    echo "  gtk4:            $(pkg-config --modversion gtk4)"
    echo "  webkitgtk-6.0:   $(pkg-config --modversion webkitgtk-6.0)"
    echo "  gtk4-layer-shell:$(pkg-config --modversion gtk4-layer-shell)"
  '';
}
