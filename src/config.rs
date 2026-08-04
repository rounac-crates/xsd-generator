//! Types for the generator configuration.
//!

use serde::Deserialize;
use std::{fs, io, path::Path};

pub fn config_from_file<P: AsRef<Path>>(path: P) -> io::Result<GenConfig> {
	toml::from_str(&fs::read_to_string(path)?).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct GenConfig {
	format_uuids_simple: bool,
}
