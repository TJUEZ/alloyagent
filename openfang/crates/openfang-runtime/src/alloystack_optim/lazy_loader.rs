//! On-demand WASM module loading with LRU caching.
//!
//! Inspired by AlloyStack's `ServiceLoader::service_or_load()` pattern
//! (`libasvisor/src/service/loader.rs`), which loads dynamic libraries via
//! `dlmopen` only when first referenced.
//!
//! This module adapts the concept to wasmtime: WASM module bytes are registered
//! eagerly but compiled lazily on first access, with an LRU cache bounding
//! memory usage.

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

use lru::LruCache;

/// Statistics for the lazy loader.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LoaderStats {
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub total_compile_time_us: u64,
    pub modules_registered: u64,
    pub modules_compiled: u64,
    pub evictions: u64,
}

/// A lazily-compiled WASM module entry.
#[allow(dead_code)]
struct ModuleEntry {
    module: Arc<wasmtime::Module>,
    compiled_at: Instant,
    compile_time_us: u64,
}

/// On-demand WASM module loader with LRU caching.
///
/// Mirrors AlloyStack's on-demand loading: modules are registered (bytes stored)
/// but not compiled until first access. An LRU cache evicts least-recently-used
/// compiled modules when the cache is full.
pub struct LazyWasmLoader {
    /// Wasmtime engine (shared across all modules).
    engine: wasmtime::Engine,
    /// Raw WASM bytes, keyed by module id. Stored permanently until explicitly removed.
    registered: HashMap<String, Vec<u8>>,
    /// LRU cache of compiled modules.
    cache: LruCache<String, ModuleEntry>,
    /// Cumulative statistics.
    stats: LoaderStats,
}

impl LazyWasmLoader {
    /// Create a new loader with the given maximum number of cached compiled modules.
    pub fn new(max_cached: usize) -> Self {
        let engine = wasmtime::Engine::default();
        let cap = NonZeroUsize::new(max_cached.max(1)).unwrap();
        Self {
            engine,
            registered: HashMap::new(),
            cache: LruCache::new(cap),
            stats: LoaderStats::default(),
        }
    }

    /// Create a new loader with a custom wasmtime engine.
    pub fn with_engine(engine: wasmtime::Engine, max_cached: usize) -> Self {
        let cap = NonZeroUsize::new(max_cached.max(1)).unwrap();
        Self {
            engine,
            registered: HashMap::new(),
            cache: LruCache::new(cap),
            stats: LoaderStats::default(),
        }
    }

    /// Register a WASM module's bytes without compiling it.
    ///
    /// The module will be compiled on first access via `get_or_compile`.
    pub fn register_module(&mut self, id: &str, wasm_bytes: Vec<u8>) {
        self.registered.insert(id.to_owned(), wasm_bytes);
        self.stats.modules_registered += 1;
    }

    /// Remove a registered module and its cached compilation.
    pub fn unregister_module(&mut self, id: &str) {
        self.registered.remove(id);
        self.cache.pop(id);
    }

    /// Get a compiled module, compiling on first access (lazy).
    ///
    /// Subsequent calls return the cached compilation until evicted.
    pub fn get_or_compile(&mut self, id: &str) -> Result<Arc<wasmtime::Module>, anyhow::Error> {
        // Check cache first.
        if let Some(entry) = self.cache.get(id) {
            self.stats.cache_hits += 1;
            return Ok(Arc::clone(&entry.module));
        }

        self.stats.cache_misses += 1;

        // Compile from registered bytes.
        let wasm_bytes = self
            .registered
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("module '{}' not registered", id))?;

        let start = Instant::now();
        let module = wasmtime::Module::new(&self.engine, wasm_bytes)?;
        let compile_time_us = start.elapsed().as_micros() as u64;

        self.stats.total_compile_time_us += compile_time_us;
        self.stats.modules_compiled += 1;

        let arc_module = Arc::new(module);

        // Track evictions.
        if self.cache.len() == self.cache.cap().get() {
            self.stats.evictions += 1;
        }

        self.cache.put(
            id.to_owned(),
            ModuleEntry {
                module: Arc::clone(&arc_module),
                compiled_at: Instant::now(),
                compile_time_us,
            },
        );

        Ok(arc_module)
    }

    /// Eagerly compile and cache a module (preload for critical-path modules).
    pub fn preload(&mut self, id: &str) -> Result<(), anyhow::Error> {
        self.get_or_compile(id)?;
        Ok(())
    }

    /// Evict a compiled module from the cache (bytes remain registered).
    pub fn evict(&mut self, id: &str) {
        if self.cache.pop(id).is_some() {
            self.stats.evictions += 1;
        }
    }

    /// Number of currently cached compiled modules.
    pub fn cached_count(&self) -> usize {
        self.cache.len()
    }

    /// Number of registered module sources.
    pub fn registered_count(&self) -> usize {
        self.registered.len()
    }

    /// Returns a reference to the engine.
    pub fn engine(&self) -> &wasmtime::Engine {
        &self.engine
    }

    /// Returns a clone of cumulative statistics.
    pub fn stats(&self) -> LoaderStats {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal valid WASM module (binary encoding of an empty module).
    fn empty_wasm() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6D, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
        ]
    }

    #[test]
    fn test_register_and_compile() {
        let mut loader = LazyWasmLoader::new(4);
        loader.register_module("test", empty_wasm());

        assert_eq!(loader.registered_count(), 1);
        assert_eq!(loader.cached_count(), 0);

        let module = loader.get_or_compile("test");
        assert!(module.is_ok());
        assert_eq!(loader.cached_count(), 1);
        assert_eq!(loader.stats().cache_misses, 1);
    }

    #[test]
    fn test_cache_hit() {
        let mut loader = LazyWasmLoader::new(4);
        loader.register_module("test", empty_wasm());

        let _ = loader.get_or_compile("test").unwrap();
        let _ = loader.get_or_compile("test").unwrap();

        let stats = loader.stats();
        assert_eq!(stats.cache_misses, 1);
        assert_eq!(stats.cache_hits, 1);
    }

    #[test]
    fn test_unregistered_module_error() {
        let mut loader = LazyWasmLoader::new(4);
        let result = loader.get_or_compile("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_lru_eviction() {
        let mut loader = LazyWasmLoader::new(2); // Cache capacity = 2
        loader.register_module("a", empty_wasm());
        loader.register_module("b", empty_wasm());
        loader.register_module("c", empty_wasm());

        let _ = loader.get_or_compile("a").unwrap();
        let _ = loader.get_or_compile("b").unwrap();
        assert_eq!(loader.cached_count(), 2);

        // This should evict "a" (least recently used).
        let _ = loader.get_or_compile("c").unwrap();
        assert_eq!(loader.cached_count(), 2);
        assert!(loader.stats().evictions >= 1);
    }
}
