//! Shared [`wasmtime::Engine`] factory with Cranelift compilation and disk
//! caching.
//!
//! Every embedded DSL engine (Python-WASI, QuickJS-WASI) shares a single
//! `Engine` configuration so compilation artefacts are cached once and reused
//! across runtimes.

use anyhow::{Context, Result};
use wasmtime::{Cache, CacheConfig, Config, Engine, OptLevel};

/// Create a wasmtime [`Engine`] configured for Cranelift compilation with
/// on-disk caching under `~/.harmont/runtimes/cache/`.
///
/// # Errors
///
/// Returns an error if the engine configuration is invalid or if wasmtime
/// cannot be initialised on this platform.
pub fn create_engine() -> Result<Engine> {
    let mut config = Config::new();

    // Explicitly enable proposals used by WASI runtimes (both are on by
    // default in wasmtime 33 but being explicit avoids breakage if defaults
    // change).
    config.wasm_bulk_memory(true);
    config.wasm_multi_value(true);
    config.cranelift_opt_level(OptLevel::Speed);

    // Enable compilation caching for fast subsequent runs.
    if let Some(home) = dirs::home_dir() {
        let cache_dir = home.join(".harmont").join("runtimes").join("cache");
        let _ = std::fs::create_dir_all(&cache_dir);

        let mut cache_config = CacheConfig::new();
        cache_config.with_directory(&cache_dir);

        if let Ok(cache) = Cache::new(cache_config) {
            config.cache(Some(cache));
        }
    }

    Engine::new(&config).context("creating wasmtime engine")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_creates_successfully() {
        let engine = create_engine().expect("engine should be created successfully");
        drop(engine);
    }
}
