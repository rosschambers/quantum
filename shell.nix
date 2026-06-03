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
  # in a temp dir with the alias and prepend it to PKG_CONFIG_PATH.
  shellHook = ''
    PKG_CONFIG_SHIM="$(mktemp -d)"
    ln -sf "${pkgs.gtk4-layer-shell.dev}/lib/pkgconfig/gtk4-layer-shell-0.pc" \
           "$PKG_CONFIG_SHIM/gtk4-layer-shell.pc"
    export PKG_CONFIG_PATH="$PKG_CONFIG_SHIM:$PKG_CONFIG_PATH"

    echo "quantum host shell ready"
    echo "  rustc:           $(rustc --version)"
    echo "  gtk4:            $(pkg-config --modversion gtk4)"
    echo "  webkitgtk-6.0:   $(pkg-config --modversion webkitgtk-6.0)"
    echo "  gtk4-layer-shell:$(pkg-config --modversion gtk4-layer-shell)"
  '';
}
