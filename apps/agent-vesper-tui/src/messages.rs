//! Terminal-owned message composition and durable future-turn lifecycle.
//! Payloads have one journal owner; queue order and active state refer to IDs.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PasteOrigin {
    #[default]
    TerminalDelivered,
    LegacySubmitted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PasteMetadata {
    pub id: String,
    #[serde(default)]
    pub origin: PasteOrigin,
    pub start: usize,
    pub end: usize,
    pub characters: usize,
    pub lines: usize,
}

impl PasteMetadata {
    pub fn label(&self) -> String {
        format!(
            "[{} · {} chars · {} lines · {}]",
            if self.origin == PasteOrigin::TerminalDelivered {
                "Pasted Content"
            } else {
                "Submitted Content"
            },
            grouped(self.characters),
            self.lines,
            self.id
        )
    }
}

pub fn grouped(count: usize) -> String {
    let digits = count.to_string();
    let mut result = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

pub fn logical_lines(text: &str) -> usize {
    let mut count = 1;
    let mut previous_cr = false;
    for ch in text.chars() {
        if ch == '\r' || (ch == '\n' && !previous_cr) {
            count += 1;
        }
        previous_cr = ch == '\r';
    }
    count
}

pub fn should_collapse(text: &str) -> bool {
    text.contains(['\r', '\n']) || text.chars().take(1000).count() >= 1000
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerPaste {
    pub offset: usize,
    pub text: String,
    pub metadata: PasteMetadata,
}

impl ComposerPaste {
    pub fn new(id: String, offset: usize, text: String) -> Self {
        let metadata = PasteMetadata {
            id,
            origin: PasteOrigin::TerminalDelivered,
            start: 0,
            end: text.len(),
            characters: text.chars().count(),
            lines: logical_lines(&text),
        };
        Self {
            offset,
            text,
            metadata,
        }
    }
}

/// Assemble a snapshot, preserving insertion positions and attachment order.
/// The caller clears the composer only after durable admission succeeds.
pub fn compose(input: &str, pastes: &[ComposerPaste]) -> Result<Message, String> {
    let size = pastes
        .iter()
        .try_fold(input.len(), |n, paste| n.checked_add(paste.text.len()))
        .ok_or("message size overflow")?;
    if size > MAX_MESSAGE_BYTES {
        return Err("Message exceeds the 1 MiB text limit; draft retained.".into());
    }
    let mut ordered = pastes.iter().enumerate().collect::<Vec<_>>();
    ordered.sort_by_key(|(index, paste)| (paste.offset, *index));
    let mut text = String::with_capacity(size);
    let mut metadata = Vec::new();
    let mut position = 0;
    for (_, paste) in ordered {
        if paste.offset > input.len() || !input.is_char_boundary(paste.offset) {
            return Err("Invalid paste position; draft retained.".into());
        }
        text.push_str(&input[position..paste.offset]);
        let mut span = paste.metadata.clone();
        span.start = text.len();
        text.push_str(&paste.text);
        span.end = text.len();
        metadata.push(span);
        position = paste.offset;
    }
    text.push_str(&input[position..]);
    Ok(Message {
        id: String::new(),
        text,
        images: Vec::new(),
        pastes: metadata,
        state: MessageState::Pending,
        transcript_committed: false,
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageState {
    Pending,
    Active,
    Settled,
    Interrupted,
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageImage {
    pub descriptor: vesper_domain::ImageDescriptor,
    pub path: std::path::PathBuf,
    pub encoded: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub images: Vec<MessageImage>,
    pub pastes: Vec<PasteMetadata>,
    pub state: MessageState,
    /// Display projection only; never authorizes execution or promotion.
    #[serde(default)]
    pub transcript_committed: bool,
}

impl Message {
    pub fn plain(text: String) -> Self {
        Self {
            id: String::new(),
            text,
            images: Vec::new(),
            pastes: Vec::new(),
            state: MessageState::Pending,
            transcript_committed: false,
        }
    }
    /// Never walk attachment payloads to construct ordinary frame previews.
    pub fn preview(&self) -> String {
        let mut output = String::new();
        let mut position = 0;
        for paste in &self.pastes {
            if output.len() > 600 {
                output.push_str(" …");
                return output;
            }
            append_preview(&mut output, &self.text[position..paste.start]);
            output.push_str(&paste.label());
            position = paste.end;
        }
        append_preview(&mut output, &self.text[position..]);
        output
    }
}

fn append_preview(output: &mut String, text: &str) {
    let remaining = 240usize.saturating_sub(output.chars().count());
    output.extend(
        text.chars()
            .take(remaining)
            .map(|ch| if ch.is_control() { ' ' } else { ch }),
    );
    if text.chars().take(remaining + 1).count() > remaining {
        output.push('…');
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageJournal {
    pub messages: Vec<Message>,
    pub pending: VecDeque<String>,
    pub active: Option<String>,
    pub paused: Option<String>,
    pub next_id: u64,
    pub next_paste_id: u64,
}

impl MessageJournal {
    pub fn paste_id(&mut self) -> Result<String, String> {
        self.next_paste_id = self
            .next_paste_id
            .checked_add(1)
            .ok_or("Paste identity exhausted; draft retained.")?;
        Ok(format!("p{}", self.next_paste_id))
    }
    pub fn admit(&mut self, mut message: Message) -> Result<String, String> {
        if message.text.len() > MAX_MESSAGE_BYTES {
            return Err("Message exceeds the 1 MiB text limit.".into());
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("message identity exhausted")?;
        message.id = format!("m{}", self.next_id);
        let id = message.id.clone();
        self.pending.push_back(id.clone());
        self.messages.push(message);
        Ok(id)
    }
    pub fn get(&self, id: &str) -> Option<&Message> {
        self.messages.iter().find(|message| message.id == id)
    }
    pub fn promote(&mut self, running: bool, approval: bool) -> Option<String> {
        if running || approval || self.active.is_some() || self.paused.is_some() {
            return None;
        }
        let id = self.pending.pop_front()?;
        self.messages.iter_mut().find(|m| m.id == id)?.state = MessageState::Active;
        self.active = Some(id.clone());
        Some(id)
    }
    pub fn settle(&mut self, success: bool) {
        if let Some(id) = self.active.take() {
            if let Some(message) = self.messages.iter_mut().find(|m| m.id == id) {
                message.state = if success {
                    MessageState::Settled
                } else {
                    MessageState::Interrupted
                };
            }
            if !success {
                self.paused =
                    Some("Current turn interrupted; use /queue resume for future messages.".into());
            }
        } else if !success {
            self.pause("Current turn interrupted.");
        }
    }
    pub fn pause(&mut self, reason: &str) {
        self.paused = Some(reason.to_owned());
    }
    pub fn recover(&mut self) {
        if self.active.is_some() {
            self.settle(false);
        }
        if !self.pending.is_empty() {
            self.pause("Recovered queue paused. Interrupted work is not replayed; /queue resume starts only pending future turns.");
        }
    }
    pub fn drop_pending(&mut self, id: &str) -> Result<Message, String> {
        let index = self
            .pending
            .iter()
            .position(|entry| entry == id)
            .ok_or("ID is not a pending message; active work uses normal cancellation.")?;
        self.pending.remove(index);
        let message = self
            .messages
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or("Pending message payload is missing.")?;
        Ok(std::mem::replace(
            message,
            Message {
                id: id.to_owned(),
                text: String::new(),
                images: Vec::new(),
                pastes: Vec::new(),
                state: MessageState::Dropped,
                transcript_committed: false,
            },
        ))
    }
    pub fn clear(&mut self) {
        while let Some(id) = self.pending.front().cloned() {
            let _ = self.drop_pending(&id);
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        let mut paste_ids = std::collections::BTreeSet::new();
        for message in &self.messages {
            if message.id.is_empty()
                || !ids.insert(&message.id)
                || message.text.len() > MAX_MESSAGE_BYTES
            {
                return Err("Invalid message journal identity or payload bounds.".into());
            }
            let counter = message
                .id
                .strip_prefix('m')
                .and_then(|id| id.parse::<u64>().ok())
                .ok_or("Invalid persisted message ID.")?;
            if counter > self.next_id {
                return Err("Persisted message counter precedes its identities.".into());
            }
            let mut end = 0;
            for paste in &message.pastes {
                let counter = paste
                    .id
                    .strip_prefix('p')
                    .and_then(|id| id.parse::<u64>().ok());
                if !paste_ids.insert(&paste.id)
                    || counter.is_none_or(|counter| counter == 0 || counter > self.next_paste_id)
                {
                    return Err("Invalid persisted paste identity or counter.".into());
                }
                if paste.start < end
                    || paste.end < paste.start
                    || paste.end > message.text.len()
                    || !message.text.is_char_boundary(paste.start)
                    || !message.text.is_char_boundary(paste.end)
                {
                    return Err("Invalid persisted paste ranges.".into());
                }
                let payload = &message.text[paste.start..paste.end];
                if paste.characters != payload.chars().count()
                    || paste.lines != logical_lines(payload)
                {
                    return Err("Persisted paste metadata disagrees with its content.".into());
                }
                end = paste.end;
            }
        }
        let mut queued = std::collections::BTreeSet::new();
        for id in &self.pending {
            if !queued.insert(id)
                || !self
                    .get(id)
                    .is_some_and(|m| m.state == MessageState::Pending)
            {
                return Err("Invalid pending queue identity.".into());
            }
        }
        for message in &self.messages {
            if message.state == MessageState::Pending && !self.pending.contains(&message.id) {
                return Err("Pending message is absent from queue order.".into());
            }
            if message.state == MessageState::Active && self.active.as_ref() != Some(&message.id) {
                return Err("Active message has no matching dispatch owner.".into());
            }
        }
        if self.active.as_ref().is_some_and(|id| {
            !self
                .get(id)
                .is_some_and(|m| m.state == MessageState::Active)
        }) {
            return Err("Invalid active message identity.".into());
        }
        Ok(())
    }
}

/// Adjust paste anchors after a contiguous text edit (including VIM actions).
/// A removed text range moves its attached blocks to that range's start.
pub fn reconcile_edit(before: &str, after: &str, pastes: &mut [ComposerPaste]) {
    let mut prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(prefix) || !after.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = before[prefix..]
        .bytes()
        .rev()
        .zip(after[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(before.len() - suffix)
        || !after.is_char_boundary(after.len() - suffix)
    {
        suffix -= 1;
    }
    let old_end = before.len() - suffix;
    let new_end = after.len() - suffix;
    for paste in pastes {
        if paste.offset > old_end {
            paste.offset = paste.offset - old_end + new_end;
        } else if paste.offset > prefix {
            paste.offset = prefix;
        }
    }
}

/// Position-preserving chip projection. Attachment payloads are never scanned.
pub fn composer_projection(
    input: &str,
    pastes: &[ComposerPaste],
    cursor: usize,
) -> Option<(String, u16)> {
    if pastes.is_empty() {
        return None;
    }
    let mut text = String::new();
    let mut position = 0;
    let mut preceding = 0;
    for paste in pastes.iter().take(20) {
        let offset = paste.offset.min(input.len());
        if !input.is_char_boundary(offset) {
            continue;
        }
        text.push_str(&input[position..offset]);
        let label = paste.metadata.label();
        if offset <= cursor {
            preceding += label.chars().count();
        }
        text.push_str(&label);
        position = offset;
    }
    text.push_str(&input[position..]);
    if pastes.len() > 20 {
        text.push_str(" [more pasted attachments]");
    }
    let cursor = cursor.min(input.len());
    let cells = input[..cursor].chars().count() + preceding;
    Some((text, cells.min(u16::MAX as usize) as u16))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collapse_thresholds_unicode_and_newlines() {
        for n in [0, 1, 999] {
            assert!(!should_collapse(&"界".repeat(n)));
        }
        assert!(should_collapse(&"界".repeat(1000)));
        for text in ["a\nb", "a\rb", "a\r\nb", "a\n"] {
            assert!(should_collapse(text));
            assert_eq!(logical_lines(text), 2);
        }
    }
    #[test]
    fn mixed_events_preserve_exact_order() {
        let mut first = ComposerPaste::new("p1".into(), 1, "B\r\n💡".into());
        let second = ComposerPaste::new("p2".into(), 2, "D\n".into());
        let message = compose("AC", &[first.clone(), second]).unwrap();
        assert_eq!(message.text, "AB\r\n💡CD\n");
        assert_eq!(message.pastes.len(), 2);
        first.offset = 100;
        assert!(compose("AC", &[first]).is_err());
    }
    #[test]
    fn lifecycle_fifo_one_owner_no_redraw_duplicate() {
        let mut journal = MessageJournal::default();
        let a = journal.admit(Message::plain("a".into())).unwrap();
        let b = journal.admit(Message::plain("b".into())).unwrap();
        assert!(journal.promote(true, false).is_none());
        assert!(journal.promote(false, true).is_none());
        assert_eq!(journal.promote(false, false), Some(a));
        for _ in 0..100 {
            assert!(journal.promote(false, false).is_none());
        }
        journal.settle(true);
        assert_eq!(journal.promote(false, false), Some(b));
    }
    #[test]
    fn cancellation_error_recovery_preserve_future_without_replay() {
        for reason in ["cancelled", "provider failed", "channel closed"] {
            let mut journal = MessageJournal::default();
            let a = journal.admit(Message::plain("active".into())).unwrap();
            let b = journal.admit(Message::plain("future".into())).unwrap();
            journal.promote(false, false);
            journal.pause(reason);
            journal.settle(false);
            assert!(journal.promote(false, false).is_none());
            assert_eq!(journal.get(&a).unwrap().state, MessageState::Interrupted);
            journal.paused = None;
            assert_eq!(journal.promote(false, false), Some(b));
        }
    }
    #[test]
    fn persistence_complete_payload_ids_and_uncertain_recovery() {
        let mut journal = MessageJournal::default();
        let paste = ComposerPaste::new(journal.paste_id().unwrap(), 0, "界\r\n".repeat(1000));
        let message = compose("", &[paste]).unwrap();
        let full = message.text.clone();
        let active = journal.admit(Message::plain("in-flight".into())).unwrap();
        let pending = journal.admit(message).unwrap();
        journal.promote(false, false);
        let mut loaded: MessageJournal =
            serde_json::from_slice(&serde_json::to_vec(&journal).unwrap()).unwrap();
        loaded.validate().unwrap();
        loaded.recover();
        assert_eq!(loaded.get(&pending).unwrap().text, full);
        assert_eq!(
            loaded.get(&active).unwrap().state,
            MessageState::Interrupted
        );
        assert!(loaded.promote(false, false).is_none());
        loaded.paused = None;
        assert_eq!(loaded.promote(false, false), Some(pending));
    }
    #[test]
    fn queue_drop_clear_and_bounded_paste_projection() {
        let message = compose(
            "beforeafter",
            &[ComposerPaste::new("p1".into(), 6, "💡\r\n".repeat(100_000))],
        )
        .unwrap();
        assert!(message.preview().len() < 200);
        assert!(!message.preview().contains("💡"));
        let mut journal = MessageJournal::default();
        let first = journal.admit(message).unwrap();
        journal.admit(Message::plain("second".into())).unwrap();
        journal.drop_pending(&first).unwrap();
        assert!(journal.drop_pending(&first).is_err());
        journal.clear();
        assert!(journal.pending.is_empty());
    }
    #[test]
    fn corrupt_paste_identities_and_exhausted_counters_fail_closed() {
        let mut journal = MessageJournal::default();
        let paste = ComposerPaste::new(journal.paste_id().unwrap(), 0, "a\nb".into());
        journal.admit(compose("", &[paste]).unwrap()).unwrap();
        journal.validate().unwrap();
        journal.next_paste_id = 0;
        assert!(journal.validate().is_err());
        journal.next_paste_id = u64::MAX;
        assert!(journal.paste_id().is_err());
        journal.messages[0].pastes[0].id = "invalid".into();
        assert!(journal.validate().is_err());
    }
}
