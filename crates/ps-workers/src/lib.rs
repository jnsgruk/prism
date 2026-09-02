// Restate 0.11 retains the trait service API for compatibility while its new
// impl-block API stabilizes. Keep existing service contracts unchanged during
// this dependency-only migration.
#![allow(deprecated)]

pub mod features;
pub mod infra;
