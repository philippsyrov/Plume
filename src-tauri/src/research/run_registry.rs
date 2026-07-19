//! Duplicate-safe active research-run ownership and cancellation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ResearchRegistryError {
    #[error("invalid research run id or owner identity")]
    InvalidIdentity,
    #[error("research run id is already active")]
    Duplicate,
}

pub(crate) fn local_owner_key(session_id: &str) -> String {
    format!("local:{session_id}")
}

pub(crate) fn project_owner_key(project_id: &str, session_id: &str) -> String {
    format!("project:{project_id}:{session_id}")
}

#[derive(Default)]
pub(crate) struct ResearchRunRegistry {
    active: Mutex<HashMap<String, ActiveRun>>,
}

struct ActiveRun {
    owner_key: String,
    cancel: Arc<AtomicBool>,
}

impl ResearchRunRegistry {
    pub(crate) fn register(
        self: &Arc<Self>,
        run_id: &str,
        owner_key: &str,
    ) -> Result<ResearchRunLease, ResearchRegistryError> {
        if !valid_identity(run_id) || owner_key.is_empty() || owner_key.len() > 256 {
            return Err(ResearchRegistryError::InvalidIdentity);
        }
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active.contains_key(run_id) {
            return Err(ResearchRegistryError::Duplicate);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        active.insert(
            run_id.to_string(),
            ActiveRun {
                owner_key: owner_key.to_string(),
                cancel: cancel.clone(),
            },
        );
        Ok(ResearchRunLease {
            registry: Arc::downgrade(self),
            run_id: run_id.to_string(),
            owner_key: owner_key.to_string(),
            cancel,
        })
    }

    pub(crate) fn cancel(&self, run_id: &str) -> bool {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(run) = active.get(run_id) else {
            return false;
        };
        run.cancel.store(true, Ordering::SeqCst);
        true
    }

    pub(crate) fn cancel_owner(&self, owner_key: &str) {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for run in active.values().filter(|run| run.owner_key == owner_key) {
            run.cancel.store(true, Ordering::SeqCst);
        }
    }

    pub(crate) fn cancel_all(&self) {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for run in active.values() {
            run.cancel.store(true, Ordering::SeqCst);
        }
    }
}

pub(crate) struct ResearchRunLease {
    registry: Weak<ResearchRunRegistry>,
    run_id: String,
    owner_key: String,
    cancel: Arc<AtomicBool>,
}

impl ResearchRunLease {
    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }
}

impl Drop for ResearchRunLease {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let mut active = registry
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let remove = active.get(&self.run_id).is_some_and(|run| {
            run.owner_key == self.owner_key && Arc::ptr_eq(&run.cancel, &self.cancel)
        });
        if remove {
            active.remove(&self.run_id);
        }
    }
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
