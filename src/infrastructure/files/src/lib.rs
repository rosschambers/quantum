//! quantum-files: infrastructure implementations of the domain file-system
//! ports (`FileSystemPort`, `DirectoryWatcher`, `FileOpener`, `RecursiveSizer`).
//!
//! This crate is a sibling infrastructure crate. It may depend on
//! `quantum-domain` and other sibling infrastructure crates, but never on the
//! application or user-interface layers. The module stubs below are filled in
//! by later tasks; this scaffold exists only to compile and wire the crate into
//! the workspace.

pub mod applications;
pub mod filesystem;
pub mod opener;
pub mod operations;
pub mod pins;
pub mod sizer;
pub mod watcher;

pub use applications::DesktopApplicationCatalog;
pub use filesystem::LocalFileSystem;
pub use opener::ProcessFileOpener;
pub use pins::{Pin, PinStore};
pub use sizer::BackgroundSizer;
pub use watcher::NotifyDirectoryWatcher;
