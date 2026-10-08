//! Bounded native credential records; the transport owns OS access.

use super::{CredentialStoreError, MAX_RECORD_BYTES, validate_record};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const PREFIX: &str = "\u{1e}vesper-native-record-v1:";
const CHUNK_BYTES: usize = 1200;
const MAX_CHUNKS: usize = MAX_RECORD_BYTES / (CHUNK_BYTES - 3) + 1;

pub(super) trait Backend {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>, CredentialStoreError>;
    fn write(&self, account: &str, value: &str) -> Result<(), CredentialStoreError>;
    fn delete(&self, account: &str) -> Result<(), CredentialStoreError>;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    generation: String,
    chunks: usize,
    bytes: usize,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    previous: Option<Record>,
    next: Option<Record>,
    previous_hash: Option<String>,
    next_hash: Option<String>,
    deleting: bool,
}

fn unavailable() -> CredentialStoreError {
    CredentialStoreError::Unavailable
}

fn digest(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|c| c.is_ascii_hexdigit())
}

impl Record {
    fn validate(&self) -> Result<(), CredentialStoreError> {
        if !hex(&self.generation, 32)
            || !hex(&self.sha256, 64)
            || self.chunks == 0
            || self.chunks > MAX_CHUNKS
            || self.bytes == 0
            || self.bytes > MAX_RECORD_BYTES
        {
            return Err(unavailable());
        }
        Ok(())
    }

    fn account(&self, account: &str, index: usize) -> String {
        format!("{account}:record:{}:{index}", self.generation)
    }
}

fn descriptor(value: Option<&str>) -> Result<Option<Record>, CredentialStoreError> {
    let Some(encoded) = value.and_then(|v| v.strip_prefix(PREFIX)) else {
        return Ok(None);
    };
    if encoded.len() > CHUNK_BYTES {
        return Err(unavailable());
    }
    let record: Record = serde_json::from_str(encoded).map_err(|_| unavailable())?;
    record.validate()?;
    Ok(Some(record))
}

fn journal_account(account: &str) -> String {
    format!("{account}:record-journal-v1")
}

fn read_journal(
    backend: &impl Backend,
    account: &str,
) -> Result<Option<Journal>, CredentialStoreError> {
    let Some(value) = backend.read(&journal_account(account))? else {
        return Ok(None);
    };
    if value.len() > CHUNK_BYTES {
        return Err(unavailable());
    }
    let journal: Journal = serde_json::from_str(&value).map_err(|_| unavailable())?;
    for record in [&journal.previous, &journal.next].into_iter().flatten() {
        record.validate()?;
    }
    for hash in [&journal.previous_hash, &journal.next_hash]
        .into_iter()
        .flatten()
    {
        if !hex(hash, 64) {
            return Err(unavailable());
        }
    }
    for (record, expected_hash) in [
        (&journal.previous, &journal.previous_hash),
        (&journal.next, &journal.next_hash),
    ] {
        if let Some(record) = record {
            let primary = format!(
                "{PREFIX}{}",
                serde_json::to_string(record).map_err(|_| unavailable())?
            );
            if expected_hash.as_deref() != Some(digest(&primary).as_str()) {
                return Err(unavailable());
            }
        }
    }
    if !journal.deleting && journal.next_hash.is_none() {
        return Err(unavailable());
    }
    if let (Some(previous), Some(next)) = (&journal.previous, &journal.next)
        && previous.generation == next.generation
    {
        return Err(unavailable());
    }
    Ok(Some(journal))
}

fn write_journal(
    backend: &impl Backend,
    account: &str,
    journal: &Journal,
) -> Result<(), CredentialStoreError> {
    let encoded = serde_json::to_string(journal).map_err(|_| unavailable())?;
    if encoded.len() > CHUNK_BYTES {
        return Err(unavailable());
    }
    backend.write(&journal_account(account), &encoded)
}

fn cleanup(
    backend: &impl Backend,
    account: &str,
    record: Option<&Record>,
) -> Result<(), CredentialStoreError> {
    let Some(record) = record else { return Ok(()) };
    let mut failed = false;
    for index in 0..record.chunks {
        failed |= backend.delete(&record.account(account, index)).is_err();
    }
    if failed { Err(unavailable()) } else { Ok(()) }
}

fn settle_delete(
    backend: &impl Backend,
    account: &str,
    journal: &Journal,
) -> Result<(), CredentialStoreError> {
    let deleted = backend.delete(account);
    if backend.read(account)?.is_some() {
        return Err(unavailable());
    }
    // A delete reported uncertain by the OS is settled only by absent readback.
    let _ = deleted;
    let previous = cleanup(backend, account, journal.previous.as_ref());
    let next = cleanup(backend, account, journal.next.as_ref());
    previous?;
    next?;
    backend.delete(&journal_account(account))
}

fn recover(backend: &impl Backend, account: &str) -> Result<(), CredentialStoreError> {
    let Some(journal) = read_journal(backend, account)? else {
        return Ok(());
    };
    if journal.deleting {
        return settle_delete(backend, account, &journal);
    }
    let current = backend.read(account)?.as_deref().map(|v| digest(v));
    if current == journal.next_hash {
        cleanup(backend, account, journal.previous.as_ref())?;
    } else if current == journal.previous_hash {
        cleanup(backend, account, journal.next.as_ref())?;
    } else {
        // An independently changed primary record is never guessed or removed.
        return Err(unavailable());
    }
    backend.delete(&journal_account(account))
}

fn chunks(secret: &str) -> Vec<&str> {
    let mut rest = secret;
    let mut result = Vec::new();
    while !rest.is_empty() {
        let mut end = rest.len().min(CHUNK_BYTES);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        result.push(&rest[..end]);
        rest = &rest[end..];
    }
    result
}

fn read_record(
    backend: &impl Backend,
    account: &str,
    record: &Record,
) -> Result<Zeroizing<String>, CredentialStoreError> {
    record.validate()?;
    let mut secret = Zeroizing::new(String::with_capacity(record.bytes));
    for index in 0..record.chunks {
        let part = backend
            .read(&record.account(account, index))?
            .ok_or_else(unavailable)?;
        if part.is_empty() || part.len() > CHUNK_BYTES || secret.len() + part.len() > record.bytes {
            return Err(unavailable());
        }
        secret.push_str(&part);
    }
    if secret.len() != record.bytes || digest(&secret) != record.sha256 {
        return Err(unavailable());
    }
    validate_record(&secret)?;
    Ok(secret)
}

pub(super) fn load(
    backend: &impl Backend,
    account: &str,
) -> Result<Option<Zeroizing<String>>, CredentialStoreError> {
    if read_journal(backend, account)?.is_some_and(|journal| journal.deleting) {
        return Ok(None);
    }
    let Some(value) = backend.read(account)? else {
        return Ok(None);
    };
    if let Some(record) = descriptor(Some(&value))? {
        return read_record(backend, account, &record).map(Some);
    }
    validate_record(&value)?;
    Ok(Some(value))
}

/// The caller holds its native cross-process lease throughout this operation.
pub(super) fn store(
    backend: &impl Backend,
    account: &str,
    secret: &str,
) -> Result<(), CredentialStoreError> {
    let secret = validate_record(secret)?;
    recover(backend, account)?;
    let old = backend.read(account)?;
    if old.as_deref().is_some_and(|v| v == secret) {
        return Ok(());
    }
    let previous = descriptor(old.as_deref().map(|v| v.as_str()))?;
    let parts = chunks(secret);
    let next = (secret.encode_utf16().count() * 2 > 2560).then(|| Record {
        generation: uuid::Uuid::new_v4().simple().to_string(),
        chunks: parts.len(),
        bytes: secret.len(),
        sha256: digest(secret),
    });
    let primary = match &next {
        Some(record) => format!(
            "{PREFIX}{}",
            serde_json::to_string(record).map_err(|_| unavailable())?
        ),
        None => secret.to_owned(),
    };
    let primary = Zeroizing::new(primary);
    let journal = Journal {
        previous,
        next: next.clone(),
        previous_hash: old.as_deref().map(|v| digest(v)),
        next_hash: Some(digest(&primary)),
        deleting: false,
    };
    // Persist recovery metadata before any generation entry is created.
    write_journal(backend, account, &journal)?;
    if let Some(record) = &next {
        for (index, part) in parts.iter().enumerate() {
            if backend
                .write(&record.account(account, index), part)
                .is_err()
            {
                let _ = recover(backend, account);
                return Err(unavailable());
            }
        }
        if read_record(backend, account, record).is_err() {
            let _ = recover(backend, account);
            return Err(unavailable());
        }
    }
    let written = backend.write(account, &primary);
    let confirmed = backend
        .read(account)?
        .as_deref()
        .is_some_and(|v| v == primary.as_str());
    if !confirmed {
        let _ = recover(backend, account);
        return Err(unavailable());
    }
    // A verified atomic primary swap is success even if old-entry cleanup must
    // resume later. The retained journal owns every pending generation.
    let _ = written;
    let _ = recover(backend, account);
    Ok(())
}

pub(super) fn remove(backend: &impl Backend, account: &str) -> Result<(), CredentialStoreError> {
    let primary = backend.read(account)?;
    let mut journal = match read_journal(backend, account)? {
        Some(journal) => {
            let current = primary.as_deref().map(|value| digest(value));
            if current != journal.previous_hash
                && current != journal.next_hash
                && !(journal.deleting && current.is_none())
            {
                return Err(unavailable());
            }
            journal
        }
        None => Journal {
            previous: descriptor(primary.as_deref().map(|v| v.as_str()))?,
            next: None,
            previous_hash: primary.as_deref().map(|v| digest(v)),
            next_hash: None,
            deleting: true,
        },
    };
    journal.deleting = true;
    write_journal(backend, account, &journal)?;
    settle_delete(backend, account, &journal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};

    #[derive(Default)]
    struct WindowsSizedBackend {
        entries: RefCell<BTreeMap<String, String>>,
        fail_write: RefCell<Option<String>>,
        fail_delete: RefCell<Option<String>>,
        uncertain_write: RefCell<Option<String>>,
        fail_exact_write: RefCell<Option<String>>,
    }

    impl Backend for WindowsSizedBackend {
        fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>, CredentialStoreError> {
            Ok(self
                .entries
                .borrow()
                .get(account)
                .cloned()
                .map(Zeroizing::new))
        }
        fn write(&self, account: &str, value: &str) -> Result<(), CredentialStoreError> {
            if self.fail_exact_write.borrow().as_deref() == Some(account) {
                return Err(CredentialStoreError::Unavailable);
            }
            if self
                .fail_write
                .borrow()
                .as_deref()
                .is_some_and(|pattern| account.contains(pattern))
            {
                return Err(CredentialStoreError::Unavailable);
            }
            if value.encode_utf16().count() * 2 > 2560 {
                return Err(CredentialStoreError::Unavailable);
            }
            self.entries
                .borrow_mut()
                .insert(account.into(), value.into());
            if self.uncertain_write.borrow().as_deref() == Some(account) {
                return Err(CredentialStoreError::Unavailable);
            }
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), CredentialStoreError> {
            if self
                .fail_delete
                .borrow()
                .as_deref()
                .is_some_and(|pattern| account.contains(pattern))
            {
                return Err(CredentialStoreError::Unavailable);
            }
            self.entries.borrow_mut().remove(account);
            Ok(())
        }
    }

    #[test]
    fn windows_sized_native_store_accepts_subscription_token_record() {
        let backend = WindowsSizedBackend::default();
        let record = format!(
            "{{\"mode\":\"chatgpt\",\"tokens\":\"{}\"}}",
            "synthetic-token".repeat(600)
        );
        assert!(record.len() < super::super::MAX_RECORD_BYTES);
        assert!(store(&backend, "openai:credential", &record).is_ok());
        assert_eq!(
            load(&backend, "openai:credential")
                .unwrap()
                .unwrap()
                .as_str(),
            record
        );
        remove(&backend, "openai:credential").unwrap();
        assert!(backend.entries.borrow().is_empty());
    }
    const ACCOUNT: &str = "provider:credential";

    #[test]
    fn native_record_rotation_and_legacy_keys_preserve_neighbor_provider() {
        let backend = WindowsSizedBackend::default();
        store(&backend, "neighbor:credential", "neighbor-key").unwrap();
        store(&backend, ACCOUNT, "legacy-key").unwrap();
        assert_eq!(backend.entries.borrow()[ACCOUNT], "legacy-key");
        for secret in ["a".repeat(8000), "b".repeat(15000), "small-key".to_owned()] {
            store(&backend, ACCOUNT, &secret).unwrap();
            assert_eq!(load(&backend, ACCOUNT).unwrap().unwrap().as_str(), secret);
            assert!(
                !backend
                    .entries
                    .borrow()
                    .contains_key(&journal_account(ACCOUNT))
            );
        }
        remove(&backend, ACCOUNT).unwrap();
        assert_eq!(
            load(&backend, "neighbor:credential")
                .unwrap()
                .unwrap()
                .as_str(),
            "neighbor-key"
        );
        assert_eq!(backend.entries.borrow().len(), 1);
    }

    #[test]
    fn native_record_failed_chunk_save_retains_old_credential_and_recovers() {
        let backend = WindowsSizedBackend::default();
        store(&backend, ACCOUNT, "previous-key").unwrap();
        *backend.fail_write.borrow_mut() = Some(":1".into());
        assert!(store(&backend, ACCOUNT, &"a".repeat(8000)).is_err());
        assert_eq!(
            load(&backend, ACCOUNT).unwrap().unwrap().as_str(),
            "previous-key"
        );
        assert_eq!(backend.entries.borrow().len(), 1);
        *backend.fail_write.borrow_mut() = None;
        store(&backend, ACCOUNT, &"b".repeat(8000)).unwrap();
        assert_eq!(
            load(&backend, ACCOUNT).unwrap().unwrap().as_str(),
            "b".repeat(8000)
        );
    }

    #[test]
    fn native_record_failed_primary_swap_preserves_previous_generation() {
        let backend = WindowsSizedBackend::default();
        let old = "old".repeat(3000);
        store(&backend, ACCOUNT, &old).unwrap();
        *backend.fail_exact_write.borrow_mut() = Some(ACCOUNT.into());
        assert!(store(&backend, ACCOUNT, &"next".repeat(3000)).is_err());
        assert_eq!(load(&backend, ACCOUNT).unwrap().unwrap().as_str(), old);
        *backend.fail_exact_write.borrow_mut() = None;
        *backend.uncertain_write.borrow_mut() = Some(ACCOUNT.into());
        store(&backend, ACCOUNT, "confirmed-key").unwrap();
        assert_eq!(
            load(&backend, ACCOUNT).unwrap().unwrap().as_str(),
            "confirmed-key"
        );
    }

    #[test]
    fn native_record_old_cleanup_is_owned_by_durable_journal() {
        let backend = WindowsSizedBackend::default();
        store(&backend, ACCOUNT, &"old".repeat(3000)).unwrap();
        let old = descriptor(Some(&backend.entries.borrow()[ACCOUNT]))
            .unwrap()
            .unwrap();
        *backend.fail_delete.borrow_mut() = Some(old.generation.clone());
        store(&backend, ACCOUNT, &"new".repeat(3000)).unwrap();
        assert!(
            backend
                .entries
                .borrow()
                .contains_key(&journal_account(ACCOUNT))
        );
        assert_eq!(
            load(&backend, ACCOUNT).unwrap().unwrap().as_str(),
            "new".repeat(3000)
        );
        *backend.fail_delete.borrow_mut() = None;
        store(&backend, ACCOUNT, "final-key").unwrap();
        assert_eq!(backend.entries.borrow().len(), 1);
    }

    #[test]
    fn native_record_signout_tombstone_hides_credential_and_resumes_cleanup() {
        let backend = WindowsSizedBackend::default();
        store(&backend, ACCOUNT, &"old".repeat(3000)).unwrap();
        *backend.fail_delete.borrow_mut() = Some(ACCOUNT.into());
        assert!(remove(&backend, ACCOUNT).is_err());
        assert!(load(&backend, ACCOUNT).unwrap().is_none());
        assert!(
            backend
                .entries
                .borrow()
                .contains_key(&journal_account(ACCOUNT))
        );
        *backend.fail_delete.borrow_mut() = None;
        remove(&backend, ACCOUNT).unwrap();
        assert!(backend.entries.borrow().is_empty());
        store(&backend, ACCOUNT, "new-signin").unwrap();
        assert_eq!(
            load(&backend, ACCOUNT).unwrap().unwrap().as_str(),
            "new-signin"
        );
    }

    #[test]
    fn native_record_missing_or_tampered_chunks_fail_closed() {
        let backend = WindowsSizedBackend::default();
        store(&backend, ACCOUNT, &"old".repeat(3000)).unwrap();
        let record = descriptor(Some(&backend.entries.borrow()[ACCOUNT]))
            .unwrap()
            .unwrap();
        let chunk = record.account(ACCOUNT, 0);
        let original = backend.entries.borrow_mut().remove(&chunk).unwrap();
        assert!(load(&backend, ACCOUNT).is_err());
        backend
            .entries
            .borrow_mut()
            .insert(chunk.clone(), "x".repeat(original.len()));
        assert!(load(&backend, ACCOUNT).is_err());
        backend.entries.borrow_mut().insert(chunk, original);
        assert!(load(&backend, ACCOUNT).unwrap().is_some());
    }

    #[test]
    fn native_record_unicode_and_maximum_composite_size_round_trip() {
        let backend = WindowsSizedBackend::default();
        for secret in [
            "🧑é".repeat(3000),
            format!(
                "{{\"access_token\":\"{}\",\"refresh_token\":\"{}\",\"api_key\":\"{}\"}}",
                "a".repeat(64 * 1024),
                "r".repeat(64 * 1024),
                "k".repeat(16 * 1024)
            ),
            "a".repeat(MAX_RECORD_BYTES),
        ] {
            store(&backend, ACCOUNT, &secret).unwrap();
            assert_eq!(load(&backend, ACCOUNT).unwrap().unwrap().as_str(), secret);
            assert!(
                backend
                    .entries
                    .borrow()
                    .values()
                    .all(|v| v.encode_utf16().count() * 2 <= 2560)
            );
        }
        assert!(store(&backend, ACCOUNT, &"x".repeat(MAX_RECORD_BYTES + 1)).is_err());
        remove(&backend, ACCOUNT).unwrap();
        assert!(backend.entries.borrow().is_empty());
    }

    #[test]
    fn native_record_malformed_metadata_and_external_changes_are_not_overwritten() {
        let backend = WindowsSizedBackend::default();
        backend
            .entries
            .borrow_mut()
            .insert(ACCOUNT.into(), format!("{PREFIX}{{}}"));
        assert!(load(&backend, ACCOUNT).is_err());
        assert!(store(&backend, ACCOUNT, "replacement").is_err());
        backend.entries.borrow_mut().clear();
        store(&backend, ACCOUNT, &"old".repeat(3000)).unwrap();
        let record = descriptor(Some(&backend.entries.borrow()[ACCOUNT]))
            .unwrap()
            .unwrap();
        *backend.fail_delete.borrow_mut() = Some(record.generation);
        store(&backend, ACCOUNT, "new").unwrap();
        backend
            .entries
            .borrow_mut()
            .insert(ACCOUNT.into(), "external".into());
        assert!(store(&backend, ACCOUNT, "replacement").is_err());
        assert!(remove(&backend, ACCOUNT).is_err());
        assert_eq!(backend.entries.borrow()[ACCOUNT], "external");
    }

    #[test]
    fn native_record_restart_settles_abandoned_uncommitted_generation() {
        let backend = WindowsSizedBackend::default();
        store(&backend, ACCOUNT, "previous-key").unwrap();
        let abandoned = "abandoned".repeat(1000);
        let parts = chunks(&abandoned);
        let record = Record {
            generation: uuid::Uuid::new_v4().simple().to_string(),
            chunks: parts.len(),
            bytes: abandoned.len(),
            sha256: digest(&abandoned),
        };
        let primary = format!("{PREFIX}{}", serde_json::to_string(&record).unwrap());
        write_journal(
            &backend,
            ACCOUNT,
            &Journal {
                previous: None,
                next: Some(record.clone()),
                previous_hash: Some(digest("previous-key")),
                next_hash: Some(digest(&primary)),
                deleting: false,
            },
        )
        .unwrap();
        backend
            .write(&record.account(ACCOUNT, 0), parts[0])
            .unwrap();
        // This backend snapshot represents a process that died before swapping
        // the primary. A fresh coordinator must settle its persisted journal.
        let restarted = WindowsSizedBackend {
            entries: RefCell::new(backend.entries.borrow().clone()),
            ..WindowsSizedBackend::default()
        };
        assert_eq!(
            load(&restarted, ACCOUNT).unwrap().unwrap().as_str(),
            "previous-key"
        );
        store(&restarted, ACCOUNT, "new-key").unwrap();
        assert_eq!(restarted.entries.borrow().len(), 1);
        assert_eq!(
            load(&restarted, ACCOUNT).unwrap().unwrap().as_str(),
            "new-key"
        );
    }
}
