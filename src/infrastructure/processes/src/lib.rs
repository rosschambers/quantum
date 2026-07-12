//! quantum-processes: shared process and `/proc` filesystem parsing helpers.
//!
//! This crate is a sibling infrastructure crate. It may depend on
//! `quantum-domain` and other sibling infrastructure crates, but never on the
//! application or user-interface layers. It houses the pure parsers used by the
//! provider layer (and later tasks) to read `/proc/stat` and `/proc/meminfo`.

pub mod procfs_parse;
