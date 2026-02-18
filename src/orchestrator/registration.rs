use crate::error::Result;
use crate::state::{RegistrationOutcome, ServiceState, StateTracker};
use std::sync::Arc;
use tokio::sync::RwLock;

/// RAII guard that unregisters a service from the state tracker on drop
/// unless explicitly committed (i.e., the service started successfully).
///
/// This eliminates manual `unregister_service` calls across async error paths
/// in `start_service_impl`. Every `?` between `register()` and `commit()` is
/// automatically covered — if the service fails to start, it gets cleaned up.
///
/// # Drop behavior
///
/// If `commit()` was not called, Drop spawns a background task to unregister
/// the service. This is best-effort: if the tokio runtime is shutting down,
/// the task may not run, but process exit cleans up everything anyway.
pub(super) struct ServiceRegistration {
    state_tracker: Arc<RwLock<StateTracker>>,
    service_name: String,
    committed: bool,
}

impl ServiceRegistration {
    /// Attempt to register a service in the state tracker.
    ///
    /// Returns:
    /// - `Ok(Some(guard))` if newly registered — caller must start the service
    /// - `Ok(None)` if already registered — caller should skip
    /// - `Err` on database failure
    pub async fn register(
        state_tracker: &Arc<RwLock<StateTracker>>,
        state: ServiceState,
    ) -> Result<Option<Self>> {
        let name = state.id.clone();
        let outcome = state_tracker.write().await.register_service(state).await?;
        match outcome {
            RegistrationOutcome::Registered => Ok(Some(Self {
                state_tracker: Arc::clone(state_tracker),
                service_name: name,
                committed: false,
            })),
            RegistrationOutcome::AlreadyExists { status } => {
                tracing::debug!(
                    "Service '{}' already registered (status: {}), skipping start",
                    name,
                    status
                );
                Ok(None)
            }
        }
    }

    /// Mark the registration as successful — Drop will no longer unregister.
    ///
    /// Call this after the service has been started and its state updated to
    /// Running. Once committed, the service lives until explicitly stopped.
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for ServiceRegistration {
    fn drop(&mut self) {
        if !self.committed {
            let tracker = Arc::clone(&self.state_tracker);
            let name = std::mem::take(&mut self.service_name);
            tracing::debug!(
                "ServiceRegistration guard dropping uncommitted '{}' — spawning cleanup",
                name
            );
            tokio::spawn(async move {
                if let Err(e) = tracker.write().await.unregister_service(&name).await {
                    tracing::warn!(
                        "Failed to unregister '{}' during guard cleanup: {}",
                        name,
                        e
                    );
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServiceType;
    use crate::state::ServiceState;

    #[tokio::test]
    async fn test_commit_prevents_unregister() {
        let tracker = Arc::new(RwLock::new(StateTracker::new_ephemeral().await.unwrap()));
        {
            let mut t = tracker.write().await;
            t.initialize().await.unwrap();
        }

        let state = ServiceState::new("svc".into(), ServiceType::Process, "test".into());
        let guard = ServiceRegistration::register(&tracker, state)
            .await
            .unwrap()
            .expect("should be Registered");

        guard.commit();

        // Give any hypothetical spawn a chance to run
        tokio::task::yield_now().await;

        // Service should still be registered
        let t = tracker.read().await;
        assert!(t.get_service("svc").await.is_some());
    }

    #[tokio::test]
    async fn test_drop_without_commit_unregisters() {
        let tracker = Arc::new(RwLock::new(StateTracker::new_ephemeral().await.unwrap()));
        {
            let mut t = tracker.write().await;
            t.initialize().await.unwrap();
        }

        let state = ServiceState::new("svc".into(), ServiceType::Process, "test".into());
        let guard = ServiceRegistration::register(&tracker, state)
            .await
            .unwrap()
            .expect("should be Registered");

        // Drop without commit
        drop(guard);

        // Let the spawned cleanup task run
        tokio::task::yield_now().await;
        // Extra yield to be safe — the spawn needs to acquire the write lock
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let t = tracker.read().await;
        assert!(
            t.get_service("svc").await.is_none(),
            "Service should be unregistered after guard dropped without commit"
        );
    }

    #[tokio::test]
    async fn test_already_exists_returns_none() {
        let tracker = Arc::new(RwLock::new(StateTracker::new_ephemeral().await.unwrap()));
        {
            let mut t = tracker.write().await;
            t.initialize().await.unwrap();
        }

        let state = ServiceState::new("svc".into(), ServiceType::Process, "test".into());
        let guard = ServiceRegistration::register(&tracker, state)
            .await
            .unwrap()
            .expect("first should be Registered");
        guard.commit();

        // Second registration should return None
        let state2 = ServiceState::new("svc".into(), ServiceType::Docker, "test".into());
        let result = ServiceRegistration::register(&tracker, state2)
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "Duplicate registration should return None"
        );
    }
}
