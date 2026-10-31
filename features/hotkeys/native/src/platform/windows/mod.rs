//! Windows-only semantics kept out of the domain and application layers.
pub mod backend;
pub mod compiled;
pub mod engine;
pub mod hot_path;
pub mod keys;
pub mod output;
// The only module allowed to cross Windows FFI. All planning stays safe Rust.
#[allow(unsafe_code)]
mod ffi;

// Expose only the production injector for the separate opt-in OS-output test
// target. Normal builds do not enable this feature or compile that test binary.
#[cfg(all(feature = "isolated-output-acceptance", debug_assertions))]
pub use ffi::SystemInjector as IsolatedSystemInjector;
