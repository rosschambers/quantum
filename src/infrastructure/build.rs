use std::path::Path;

fn main() {
    // Check that the default launcher bundle exists
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let bundle_path =
        Path::new(&manifest_dir).join("../ui/themes/default/views/launcher/dist/index.html");

    if !bundle_path.exists() {
        eprintln!(
            "ERROR: Default launcher bundle not found at: {}",
            bundle_path.display()
        );
        eprintln!("Please build the frontend bundle with:");
        eprintln!("  pnpm --filter default-launcher build");
        std::process::exit(1);
    }

    println!("cargo:rerun-if-changed=../ui/themes/default");
}
