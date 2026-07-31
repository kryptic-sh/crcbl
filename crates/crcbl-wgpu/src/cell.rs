//! Polymorphic ownership: `Arc`/`Mutex` on native, `Rc`/`RefCell` on wasm32.
//!
//! wgpu's web backend returns `!Send` types backed by `Rc`, so `Arc<Mutex<…>>`
//! does not compile on wasm32. Browser code is single-threaded, so `Rc<RefCell<…>>`
//! is the correct substitute.
//!
//! [`Lock`] provides a uniform `.lock()` → `.unwrap()` API on both targets,
//! so pool users need no `#[cfg]` branches.

#[cfg(not(target_arch = "wasm32"))]
mod inner {
    use std::sync::{Arc, Mutex, MutexGuard};
    pub type Shared<T> = Arc<T>;
    pub type Lock<T> = MyMutex<T>;

    pub struct MyMutex<T>(Mutex<T>);
    impl<T> MyMutex<T> {
        pub fn new(val: T) -> Self {
            Self(Mutex::new(val))
        }
        pub fn lock(&self) -> Result<MutexGuard<'_, T>, NeverPanic> {
            Ok(self.0.lock().unwrap())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for MyMutex<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("Lock").field(&self.0).finish()
        }
    }
    #[derive(Debug)]
    pub enum NeverPanic {}
}

#[cfg(target_arch = "wasm32")]
mod inner {
    use std::cell::{RefCell, RefMut};
    use std::rc::Rc;
    pub type Shared<T> = Rc<T>;
    pub type Lock<T> = MyMutex<T>;

    pub struct MyMutex<T>(RefCell<T>);
    impl<T> MyMutex<T> {
        pub fn new(val: T) -> Self {
            Self(RefCell::new(val))
        }
        pub fn lock(&self) -> Result<RefMut<'_, T>, NeverPanic> {
            Ok(self.0.borrow_mut())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for MyMutex<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("Lock").field(&self.0).finish()
        }
    }
    #[derive(Debug)]
    pub enum NeverPanic {}
}

pub use inner::{Lock, Shared};
