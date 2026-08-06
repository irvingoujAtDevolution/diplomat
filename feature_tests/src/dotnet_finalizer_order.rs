//! .NET-only repro for finalizer ordering across an owned borrowing return.

#[diplomat::bridge]
pub mod ffi {
    #[diplomat::attr(not(dotnet), disable)]
    #[diplomat::opaque]
    pub struct FinalizerOrderSource(pub(super) std::sync::Arc<super::OrderState>);

    #[diplomat::attr(not(dotnet), disable)]
    #[diplomat::opaque]
    pub struct FinalizerOrderDependent<'a> {
        pub(super) source: &'a FinalizerOrderSource,
        pub(super) state: std::sync::Arc<super::OrderState>,
    }

    impl FinalizerOrderSource {
        pub fn create() -> Box<Self> {
            Box::new(Self(std::sync::Arc::new(super::OrderState::default())))
        }

        pub fn make_dependent<'a>(&'a self) -> Box<FinalizerOrderDependent<'a>> {
            Box::new(FinalizerOrderDependent {
                source: self,
                state: std::sync::Arc::clone(&self.0),
            })
        }

        pub fn reset_probe() {
            super::SOURCE_DROPS.store(0, super::Ordering::SeqCst);
            super::DEPENDENT_DROPS.store(0, super::Ordering::SeqCst);
            super::BAD_ORDER_DROPS.store(0, super::Ordering::SeqCst);
        }

        pub fn source_drops() -> u64 {
            super::SOURCE_DROPS.load(super::Ordering::SeqCst)
        }

        pub fn dependent_drops() -> u64 {
            super::DEPENDENT_DROPS.load(super::Ordering::SeqCst)
        }

        pub fn bad_order_drops() -> u64 {
            super::BAD_ORDER_DROPS.load(super::Ordering::SeqCst)
        }
    }

    impl<'a> FinalizerOrderDependent<'a> {
        pub fn reads_source(&self) -> bool {
            std::sync::Arc::ptr_eq(&self.source.0, &self.state)
        }
    }
}

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

#[derive(Default)]
pub(crate) struct OrderState {
    source_dropped: AtomicBool,
}

static SOURCE_DROPS: AtomicU64 = AtomicU64::new(0);
static DEPENDENT_DROPS: AtomicU64 = AtomicU64::new(0);
static BAD_ORDER_DROPS: AtomicU64 = AtomicU64::new(0);

impl Drop for ffi::FinalizerOrderSource {
    fn drop(&mut self) {
        self.0.source_dropped.store(true, Ordering::SeqCst);
        SOURCE_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

impl Drop for ffi::FinalizerOrderDependent<'_> {
    fn drop(&mut self) {
        if self.state.source_dropped.load(Ordering::SeqCst) {
            BAD_ORDER_DROPS.fetch_add(1, Ordering::SeqCst);
        }

        // A real Rust destructor is allowed to read through this borrow.
        let _ = Arc::ptr_eq(&self.source.0, &self.state);
        DEPENDENT_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}
