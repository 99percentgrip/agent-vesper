//! Context-local secret scope for multi-profile credential isolation.
//!
//! Inspired by Hermes' `secret_scope.py`: when multiple profiles share one
//! process (gateway multiplexing), credentials must be resolved from a
//! per-task scope rather than the process-global `std::env`, otherwise
//! profile A's keys leak into profile B's turns. This module provides a
//! fail-closed, `tokio::task_local!`-backed scope that:
//!
//! - Resolves secrets from the active task-local mapping when installed.
//! - Falls back to `std::env` in single-profile mode (default).
//! - **Fails closed** with [`SecretScopeError::Unscoped`] when multiplexing
//!   is active and no scope is set — an un-migrated call site fails loud
//!   instead of silently leaking another profile's value.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;

use crate::SecretValue;

tokio::task_local! {
    /// Active secret scope for the current task. `None` when no scope is installed.
    static ACTIVE_SCOPE: Option<Arc<BTreeMap<String, SecretValue>>>;
}

// ── multiplex-active flag ────────────────────────────────────────────────
// Process-global: set once when the runtime enters multiplex mode. Governs
// whether `current()` fails closed on an unscoped read.
static MULTIPLEX_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Mark whether the process is running as a profile multiplexer.
///
/// When `true`, [`SecretScope::current`] fails closed on an unscoped read
/// instead of falling back to `std::env`. Called once at startup.
pub fn set_multiplex_active(active: bool) {
    MULTIPLEX_ACTIVE.store(active, Ordering::SeqCst);
}

/// Returns whether the process is running as a profile multiplexer.
#[must_use]
pub fn is_multiplex_active() -> bool {
    MULTIPLEX_ACTIVE.load(Ordering::SeqCst)
}

/// Provider-neutral, context-local secret scope.
///
/// Wraps an `Arc<BTreeMap<String, SecretValue>>` so it can be cheaply shared
/// across `.await` points within one task without leaking into sibling tasks.
/// Install it via [`SecretScope::install`] before resolving credentials.
#[derive(Clone)]
pub struct SecretScope {
    values: Arc<BTreeMap<String, SecretValue>>,
}

impl SecretScope {
    /// Creates an empty scope with no secrets.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            values: Arc::new(BTreeMap::new()),
        }
    }

    /// Creates a scope from a pre-built name→value mapping.
    #[must_use]
    pub fn from_map(values: BTreeMap<String, SecretValue>) -> Self {
        Self {
            values: Arc::new(values),
        }
    }

    /// Inserts one secret into a new scope (builder style).
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: SecretValue) -> Self {
        Arc::make_mut(&mut self.values).insert(name.into(), value);
        self
    }

    /// Number of secrets stored in this scope.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether this scope holds no secrets.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Runs `future` with this scope installed as the active task-local scope.
    ///
    /// Secrets resolved via [`SecretScope::current`] inside `future` (or any
    /// task it spawns that inherits the context) will see this scope's values.
    pub async fn install<F, T>(&self, future: F) -> T
    where
        F: Future<Output = T>,
    {
        let snapshot = Some(Arc::clone(&self.values));
        ACTIVE_SCOPE.scope(snapshot, future).await
    }

    /// Resolves a secret by name from the active scope or environment.
    ///
    /// Resolution order:
    /// 1. Active task-local scope (if installed and contains `name`).
    /// 2. If not found and multiplexing is **active**: fail closed.
    /// 3. If not found and multiplexing is **inactive** (default): fall back
    ///    to `std::env::var(name)`, filtering empty values.
    ///
    /// This is safe to call from any context (sync or async, with or without a
    /// tokio runtime). When no runtime is present, step 1 is skipped.
    pub fn current(name: &str) -> Result<SecretValue, SecretScopeError> {
        Self::current_with_mode(name, is_multiplex_active())
    }

    /// Internal resolution with an explicit multiplex-mode parameter.
    ///
    /// Exposed at `pub(crate)` visibility so unit tests can test each mode
    /// deterministically without racing the process-global flag under the
    /// parallel test runner.
    pub(crate) fn current_with_mode(
        name: &str,
        multiplex_active: bool,
    ) -> Result<SecretValue, SecretScopeError> {
        if let Some(value) = try_scope_lookup(name) {
            return Ok(value);
        }
        if multiplex_active {
            return Err(SecretScopeError::Unscoped {
                name: name.to_owned(),
            });
        }
        // Single-profile fallback to environment.
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(SecretValue::new)
            .ok_or_else(|| SecretScopeError::Missing {
                name: name.to_owned(),
            })
    }
}

impl fmt::Debug for SecretScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never expose key names or values — only the count.
        formatter
            .debug_struct("SecretScope")
            .field("entries", &self.values.len())
            .finish()
    }
}

impl Default for SecretScope {
    fn default() -> Self {
        Self::empty()
    }
}

/// Attempts to read `name` from the active task-local scope.
///
/// Returns `None` when there is no scope, no tokio runtime, or the name is
/// absent. Uses `catch_unwind` to safely handle the "no task context" panic
/// that `task_local` raises when accessed outside a tokio task.
fn try_scope_lookup(name: &str) -> Option<SecretValue> {
    let owned_name = name.to_owned();
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        ACTIVE_SCOPE.with(|opt_scope| {
            opt_scope
                .as_ref()
                .and_then(|scope| scope.get(&owned_name).cloned())
        })
    }));
    result.ok().flatten()
}

/// Error from secret-scope resolution.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretScopeError {
    /// Secret was requested with no scope active in multiplex mode.
    ///
    /// This is the fail-closed signal: a credential read reached
    /// [`SecretScope::current`] without a profile scope active, which in a
    /// multiplexer would otherwise leak whichever profile's value happened to
    /// be in `std::env`.
    #[error("secret '{name}' requested with no scope active in multiplex mode")]
    Unscoped {
        /// The secret name that was requested.
        name: String,
    },
    /// Secret was not found in the scope or environment.
    #[error("secret '{name}' not found in scope or environment")]
    Missing {
        /// The secret name that was not found.
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scope_resolves_installed_secret() {
        let scope = SecretScope::empty().with("MY_KEY", SecretValue::new("scope-value"));
        let resolved = scope
            .install(async { SecretScope::current_with_mode("MY_KEY", false).unwrap() })
            .await;
        assert_eq!(resolved.expose().as_str(), "scope-value");
    }

    #[tokio::test]
    async fn scope_takes_precedence_over_env() {
        let scope = SecretScope::empty().with("SHARED_KEY", SecretValue::new("from-scope"));
        let resolved = scope
            .install(async { SecretScope::current_with_mode("SHARED_KEY", false).unwrap() })
            .await;
        assert_eq!(resolved.expose().as_str(), "from-scope");
    }

    #[tokio::test]
    async fn scope_falls_back_to_env_when_not_multiplexing() {
        // No scope installed; multiplex=false → env fallback → missing.
        let resolved = SecretScope::current_with_mode("DEFINITELY_NONEXISTENT_KEY_12345", false);
        assert!(matches!(
            resolved,
            Err(SecretScopeError::Missing { ref name })
                if name == "DEFINITELY_NONEXISTENT_KEY_12345"));
    }

    #[tokio::test]
    async fn scope_fails_closed_when_multiplexing_and_unscoped() {
        // No scope installed; multiplex=true → must fail closed.
        let result = SecretScope::current_with_mode("ANY_SECRET_KEY", true);
        assert!(matches!(
            result,
            Err(SecretScopeError::Unscoped { ref name })
                if name == "ANY_SECRET_KEY"));
    }

    #[tokio::test]
    async fn scope_fails_closed_when_multiplexing_and_secret_absent() {
        let scope = SecretScope::empty().with("PRESENT", SecretValue::new("val"));
        let result = scope
            .install(async { SecretScope::current_with_mode("ABSENT", true) })
            .await;
        assert!(matches!(
            result,
            Err(SecretScopeError::Unscoped { ref name })
                if name == "ABSENT"));
    }

    #[tokio::test]
    async fn scope_does_not_leak_between_tasks() {
        let scope_a = SecretScope::empty().with("KEY", SecretValue::new("alpha"));
        let scope_b = SecretScope::empty().with("KEY", SecretValue::new("beta"));

        let a_val = scope_a
            .install(async {
                SecretScope::current_with_mode("KEY", false)
                    .unwrap()
                    .expose()
                    .as_str()
                    .to_owned()
            })
            .await;
        let b_val = scope_b
            .install(async {
                SecretScope::current_with_mode("KEY", false)
                    .unwrap()
                    .expose()
                    .as_str()
                    .to_owned()
            })
            .await;

        assert_eq!(a_val, "alpha");
        assert_eq!(b_val, "beta");
    }

    #[tokio::test]
    async fn scope_outside_install_falls_back_to_env() {
        // Called from within a tokio task but no scope installed → env fallback.
        let result = SecretScope::current_with_mode("ANOTHER_MISSING_KEY_67890", false);
        assert!(result.is_err());
    }

    #[test]
    fn scope_current_works_without_tokio_runtime() {
        // Sync context, no tokio runtime → try_scope_lookup returns None,
        // falls through to env. Must not panic.
        let result = SecretScope::current_with_mode("SYNC_MISSING_KEY_54321", false);
        assert!(matches!(
            result,
            Err(SecretScopeError::Missing { ref name })
                if name == "SYNC_MISSING_KEY_54321"));
    }

    #[test]
    fn multiplex_flag_set_and_get() {
        // Test the global flag directly — this is the only test that touches it.
        set_multiplex_active(true);
        assert!(is_multiplex_active());
        set_multiplex_active(false);
        assert!(!is_multiplex_active());
    }

    #[test]
    fn debug_never_exposes_values() {
        let scope = SecretScope::empty()
            .with("SECRET_ONE", SecretValue::new("canary-alpha"))
            .with("SECRET_TWO", SecretValue::new("canary-beta"));
        let debug = format!("{scope:?}");
        assert!(debug.contains("entries") || debug.contains("SecretScope"));
        assert!(!debug.contains("canary-alpha"));
        assert!(!debug.contains("canary-beta"));
        assert!(!debug.contains("SECRET_ONE"));
        assert!(!debug.contains("SECRET_TWO"));
    }

    #[test]
    fn empty_scope_is_empty() {
        let scope = SecretScope::empty();
        assert!(scope.is_empty());
        assert_eq!(scope.len(), 0);
        let scope = scope.with("KEY", SecretValue::new("val"));
        assert!(!scope.is_empty());
        assert_eq!(scope.len(), 1);
    }

    #[test]
    fn from_map_construction() {
        let mut map = BTreeMap::new();
        map.insert("A".to_owned(), SecretValue::new("val-a"));
        map.insert("B".to_owned(), SecretValue::new("val-b"));
        let scope = SecretScope::from_map(map);
        assert_eq!(scope.len(), 2);
    }

    #[tokio::test]
    async fn scope_clones_share_underlying_data() {
        let scope = SecretScope::empty().with("KEY", SecretValue::new("shared"));
        let cloned = scope.clone();
        let val = cloned
            .install(async {
                SecretScope::current_with_mode("KEY", false)
                    .unwrap()
                    .expose()
                    .as_str()
                    .to_owned()
            })
            .await;
        assert_eq!(val, "shared");
    }
}
