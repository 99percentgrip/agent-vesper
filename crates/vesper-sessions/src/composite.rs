use std::{collections::BTreeMap, sync::Arc};

use vesper_domain::SessionId;

use crate::{
    BoxSessionFuture, SessionListFilter, SessionLister, SessionMetadata, SessionReadIntent,
    SessionReader, SessionRecord, SessionRepository, SessionSource, SessionStoreError,
    sort_session_metadata,
};

/// Read-only empty source used when a configured source is disabled.
#[derive(Debug, Clone)]
pub struct EmptySessionRepository {
    source: SessionSource,
}

impl EmptySessionRepository {
    /// Creates an empty leaf source.
    pub fn new(source: SessionSource) -> Result<Self, SessionStoreError> {
        if source == SessionSource::Composite {
            return Err(SessionStoreError::InvalidSourceOrder);
        }
        Ok(Self { source })
    }
}

impl SessionReader for EmptySessionRepository {
    fn source(&self) -> SessionSource {
        self.source.clone()
    }

    fn read<'a>(
        &'a self,
        _session_id: &'a SessionId,
        _intent: SessionReadIntent,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>> {
        Box::pin(async { Ok(None) })
    }
}

impl SessionLister for EmptySessionRepository {
    fn list_filtered(
        &self,
        _filter: SessionListFilter,
    ) -> BoxSessionFuture<'_, Result<Vec<SessionMetadata>, SessionStoreError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// Fixed-order composite: memory, Agent Vesper, then legacy compatibility state.
pub struct CompositeSessionRepository {
    sources: [Arc<dyn SessionRepository>; 3],
}

impl CompositeSessionRepository {
    /// Creates the fixed precedence chain and rejects incorrectly ordered sources.
    pub fn new(
        in_memory: Arc<dyn SessionRepository>,
        agent_vesper: Arc<dyn SessionRepository>,
        legacy: Arc<dyn SessionRepository>,
    ) -> Result<Self, SessionStoreError> {
        if in_memory.source() != SessionSource::InMemory
            || agent_vesper.source() != SessionSource::AgentVesper
            || !matches!(legacy.source(), SessionSource::LegacyNativeGlm { .. })
        {
            return Err(SessionStoreError::InvalidSourceOrder);
        }
        Ok(Self {
            sources: [in_memory, agent_vesper, legacy],
        })
    }
}

impl SessionReader for CompositeSessionRepository {
    fn source(&self) -> SessionSource {
        SessionSource::Composite
    }

    fn read<'a>(
        &'a self,
        session_id: &'a SessionId,
        intent: SessionReadIntent,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>> {
        Box::pin(async move {
            for source in &self.sources {
                if let Some(record) = source.read(session_id, intent).await? {
                    return Ok(Some(record));
                }
            }
            Ok(None)
        })
    }
}

impl SessionLister for CompositeSessionRepository {
    fn list_filtered(
        &self,
        filter: SessionListFilter,
    ) -> BoxSessionFuture<'_, Result<Vec<SessionMetadata>, SessionStoreError>> {
        Box::pin(async move {
            let mut selected = BTreeMap::new();
            for source in &self.sources {
                for metadata in source.list_filtered(filter.clone()).await? {
                    selected
                        .entry(metadata.session_id.clone())
                        .or_insert(metadata);
                }
            }
            let mut values = selected.into_values().collect::<Vec<_>>();
            sort_session_metadata(&mut values);
            Ok(values)
        })
    }
}
