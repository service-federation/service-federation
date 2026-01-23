/// Lock ordering enforcement module for deadlock prevention.
///
/// This module provides debug-mode tracking of lock acquisitions to detect violations
/// of the documented lock ordering hierarchy. In release builds, this module compiles
/// to zero-cost (all tracking is conditional on `debug_assertions`).
///
/// # Lock Ordering Hierarchy
///
/// To prevent deadlocks, locks MUST be acquired in this order:
/// 1. `services` (RwLock) - Service registry
/// 2. `health_checkers` (RwLock) - Health checker registry
/// 3. `state_tracker` (RwLock) - Persistent state
/// 4. Individual service `Mutex`es - Per-service locks
///
/// # Usage
///
/// ```ignore
/// use crate::lock_order::{LockId, track_lock_acquisition, track_lock_release};
///
/// // In orchestrator code:
/// #[cfg(debug_assertions)]
/// track_lock_acquisition(LockId::Services);
///
/// let services = self.services.read().await;
///
/// #[cfg(debug_assertions)]
/// track_lock_release(LockId::Services);
/// ```
///
/// # Violation Detection
///
/// If a lock is acquired out of order, the program will panic in debug mode
/// with a message explaining the violation. This helps catch potential deadlocks
/// during development before they manifest in production.
#[cfg(debug_assertions)]
use std::cell::RefCell;

/// Identifiers for tracked locks in the orchestrator.
///
/// The order of variants defines the required acquisition order.
/// Locks with lower discriminant values MUST be acquired before
/// locks with higher discriminant values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockId {
    /// Service registry read/write lock (lowest priority - acquire first)
    Services = 1,
    /// Health checker registry read/write lock
    HealthCheckers = 2,
    /// State tracker read/write lock
    StateTracker = 3,
    /// Individual service mutex (highest priority - acquire last)
    ServiceMutex = 4,
}

impl LockId {
    /// Get a human-readable name for this lock
    pub fn name(&self) -> &'static str {
        match self {
            LockId::Services => "services",
            LockId::HealthCheckers => "health_checkers",
            LockId::StateTracker => "state_tracker",
            LockId::ServiceMutex => "service_mutex",
        }
    }
}

#[cfg(debug_assertions)]
thread_local! {
    /// Thread-local stack of currently held locks.
    ///
    /// This is a RefCell because we need interior mutability to track locks
    /// without requiring &mut self everywhere. The RefCell panic on conflicting
    /// borrows is acceptable here since lock tracking code should never recurse.
    static LOCK_STACK: RefCell<Vec<LockId>> = const { RefCell::new(Vec::new()) };
}

/// Track acquisition of a lock in debug mode.
///
/// This function validates that the lock being acquired respects the lock ordering
/// hierarchy. If a lock is acquired out of order (e.g., acquiring `Services` after
/// `StateTracker`), this function will panic with a descriptive error message.
///
/// In release builds, this function compiles to a no-op.
///
/// # Panics
///
/// Panics if a lock ordering violation is detected (debug mode only).
///
/// # Examples
///
/// ```ignore
/// #[cfg(debug_assertions)]
/// track_lock_acquisition(LockId::Services);
///
/// let services = self.services.read().await;
/// ```
#[cfg(debug_assertions)]
pub fn track_lock_acquisition(lock: LockId) {
    LOCK_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();

        // Check if any currently held lock has higher priority (lower discriminant)
        // than the lock we're trying to acquire
        for held_lock in stack.iter() {
            if *held_lock > lock {
                panic!(
                    "Lock ordering violation: attempted to acquire '{}' (priority {}) \
                     while holding '{}' (priority {}). \
                     Locks must be acquired in order: Services < HealthCheckers < StateTracker < ServiceMutex",
                    lock.name(),
                    lock as u8,
                    held_lock.name(),
                    *held_lock as u8
                );
            }
        }

        stack.push(lock);
    });
}

/// Track release of a lock in debug mode.
///
/// This function removes the lock from the thread-local stack of held locks.
/// It validates that locks are released in LIFO order (the most recently acquired
/// lock should be released first).
///
/// In release builds, this function compiles to a no-op.
///
/// # Panics
///
/// Panics if the lock being released wasn't the most recently acquired lock,
/// or if no locks are currently held (debug mode only).
///
/// # Examples
///
/// ```ignore
/// drop(services); // Release the lock
///
/// #[cfg(debug_assertions)]
/// track_lock_release(LockId::Services);
/// ```
#[cfg(debug_assertions)]
pub fn track_lock_release(lock: LockId) {
    LOCK_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();

        // Pop the most recent lock and verify it matches what we're releasing
        match stack.pop() {
            Some(top_lock) if top_lock == lock => {
                // Correct LIFO release
            }
            Some(top_lock) => {
                panic!(
                    "Lock release order violation: attempted to release '{}' \
                     but most recently acquired lock was '{}'. \
                     Locks must be released in LIFO order.",
                    lock.name(),
                    top_lock.name()
                );
            }
            None => {
                panic!(
                    "Lock release without acquisition: attempted to release '{}' \
                     but no locks are currently held.",
                    lock.name()
                );
            }
        }
    });
}

/// No-op versions for release builds
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn track_lock_acquisition(_lock: LockId) {}

#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn track_lock_release(_lock: LockId) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(debug_assertions)]
    fn test_valid_lock_order() {
        // Acquire locks in correct order
        track_lock_acquisition(LockId::Services);
        track_lock_acquisition(LockId::HealthCheckers);
        track_lock_acquisition(LockId::StateTracker);
        track_lock_acquisition(LockId::ServiceMutex);

        // Release in LIFO order
        track_lock_release(LockId::ServiceMutex);
        track_lock_release(LockId::StateTracker);
        track_lock_release(LockId::HealthCheckers);
        track_lock_release(LockId::Services);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Lock ordering violation")]
    fn test_invalid_lock_order() {
        // Try to acquire StateTracker before Services - should panic
        track_lock_acquisition(LockId::StateTracker);
        track_lock_acquisition(LockId::Services);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Lock release order violation")]
    fn test_invalid_release_order() {
        track_lock_acquisition(LockId::Services);
        track_lock_acquisition(LockId::HealthCheckers);

        // Try to release Services before HealthCheckers - should panic
        track_lock_release(LockId::Services);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Lock release without acquisition")]
    fn test_release_without_acquisition() {
        // Try to release a lock that was never acquired - should panic
        track_lock_release(LockId::Services);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_reacquisition_same_lock() {
        // Acquire, release, then acquire again - should be valid
        track_lock_acquisition(LockId::Services);
        track_lock_release(LockId::Services);
        track_lock_acquisition(LockId::Services);
        track_lock_release(LockId::Services);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_partial_ordering() {
        // Acquire some locks, release them, then acquire others
        track_lock_acquisition(LockId::Services);
        track_lock_acquisition(LockId::HealthCheckers);
        track_lock_release(LockId::HealthCheckers);
        track_lock_release(LockId::Services);

        // Now acquire in different order
        track_lock_acquisition(LockId::StateTracker);
        track_lock_release(LockId::StateTracker);
    }
}
