use std::path::Path;

use crate::runtime;

pub fn run_file(filename: &str) -> Result<(), String> {
    runtime::run_file(Path::new(filename))
}