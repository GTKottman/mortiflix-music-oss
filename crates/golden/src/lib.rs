//! Golden-image tests for Mortiflix Studio's renderer (test-only crate; the library is empty).
//!
//! `tests/golden.rs` builds small procedural projects (no media files), renders one frame of each
//! through the CPU reference compositor (`mortiflix-render`) and compares it with a committed PNG
//! in `goldens/` using `mortiflix_testkit::golden`. The same scenes are composited on the GPU
//! (`mortiflix-gpu`) when an adapter is available and compared with the CPU result.
//!
//! Regenerate references with `MFX_STUDIO_BLESS=1 cargo test -p mortiflix-golden`.
