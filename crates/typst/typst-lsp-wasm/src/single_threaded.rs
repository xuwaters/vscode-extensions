//! The one place this stack does something unsound-looking, made checkable.
//!
//! `World: Send + Sync`, but `js_sys::Function` is neither. On
//! `wasm32-unknown-unknown` there are genuinely no threads, so sharing one is
//! fine — but "fine because of an assumption" is how unsound code gets written.
//!
//! [`SingleThreaded`] turns the assumption into a compile-time assertion: build
//! this module for a target that *does* have threads and it fails to compile
//! rather than racing.

// The whole module only exists on the single-threaded WASM target; the
// assertion below is what a future port to a threaded target would trip over.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
compile_error!(
    "SingleThreaded is only sound on wasm32-unknown-unknown, which has no \
     threads. Porting to wasm32-wasip1-threads or a native target means giving \
     the JS handles real synchronisation, not relaxing this assertion."
);

/// A value that is only safe to share because this build target has no threads.
pub struct SingleThreaded<T>(T);

impl<T> SingleThreaded<T> {
    /// Wrap a value, asserting the target has no threads.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrow the wrapped value.
    pub fn get(&self) -> &T {
        &self.0
    }
}

// SAFETY: `wasm32-unknown-unknown` is single-threaded — there is no way to
// construct a second thread that could observe the value concurrently. The
// `compile_error!` above is what keeps that true.
unsafe impl<T> Send for SingleThreaded<T> {}
// SAFETY: as above.
unsafe impl<T> Sync for SingleThreaded<T> {}
