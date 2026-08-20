#![forbid(unsafe_code)]
//! Store-backed execution of host-owned slash commands, shared by the ACP
//! harness engine (and available to any store-holding composition).
//!
//! `vesper-harness` owns the durable checkpoint and MCP/plugin roots, so the
//! compositions do not need to duplicate the TUI's binary-level drain logic:
//! `HarnessToolService::execute_host_command` resolves one host-owned
//! catalog command (`/checkpoint`, `/rollback`, `/undo`, `/export`,
//! `/sessions`, `/lineage`, `/ci`, `/plugins`, `/mcp`) against
//! `vesper-checkpoints` and `vesper-mcp` stores rooted at the same
//! `.agent-vesper/` layout, using the exact response formats the TUI
//! composition renders.
//!
//! Commands that mutate live conversation state (`/compact`,
//! `/clear-history`, `/clear-plan`), drive a real agent turn (`/diff`,
//! `/release`), or query the live provider (`/usage`) remain with the
//! composing host: this module never touches conversation state, provider
//! wire, or terminals.

use std::path::Path;

use vesper_checkpoints::CheckpointKind;

use crate::HarnessToolService;

/// Resolves the lineage record id for a host session id: direct record-id
/// match first, then the record whose name carries the host session id
/// (seeded records use the lineage store's own `sess-N` ids).
fn lineage_record_id(
    sessions: &vesper_checkpoints::SessionLineage,
    session_id: &str,
) -> Option<String> {
    sessions
        .get(session_id)
        .map(|record| record.id)
        .or_else(|| {
            sessions
                .list()
                .into_iter()
                .find(|record| record.name == session_id)
                .map(|record| record.id)
        })
}

impl HarnessToolService {
    /// Executes one host-owned catalog slash command against the durable
    /// checkpoint / MCP / plugin stores this service already owns.
    ///
    /// `session_id` scopes checkpoint lineage, `workspace_root` confines
    /// snapshot/restore confinement checks, and `transcript` supplies the
    /// `role: text` lines `/export` renders. Blocking (subprocess
    /// handshakes for `/mcp tools`, `gh` for `/ci`): hosts call this from a
    /// blocking thread.
    pub fn execute_host_command(
        &self,
        name: &str,
        argument: &str,
        session_id: &str,
        workspace_root: &Path,
        transcript: &[String],
    ) -> String {
        // Checkpoint-family commands are opt-in (see
        // `new_with_checkpoint_gate`). When the composition disabled the
        // subsystem, answer truthfully instead of creating durable state.
        if !self.checkpoints_enabled
            && matches!(name, "checkpoint" | "rollback" | "undo" | "sessions" | "lineage")
        {
            return format!(
                "/{name}: session checkpoints and lineage are disabled by default in this \
                 composition. Set AGENT_VESPER_ENABLE_CHECKPOINTS=1 (or \
                 AGENT_VESPER_CHECKPOINT_ROOT) and restart the agent to enable them."
            );
        }
        match name {
            "checkpoint" => self.checkpoint_command(argument, session_id, workspace_root),
            "rollback" => self.rollback_command(argument, session_id, workspace_root),
            "undo" => self.undo_command(argument, session_id, workspace_root),
            "export" => self.export_command(argument, session_id, transcript),
            "sessions" => self.sessions_command(argument),
            "lineage" => {
                self.ensure_lineage_session(session_id, workspace_root);
                self.lineage_command(session_id)
            }
            "ci" => {
                let status = vesper_checkpoints::CiStatusReader::status();
                format!("ci: {}", status.output)
            }
            "plugins" => self.plugins_command(argument),
            "mcp" => self.mcp_command(argument),
            _ => format!(
                "/{name} is a host-owned command this composition does not serve \
                 through the shared executor."
            ),
        }
    }

    /// `/checkpoint [label]` creates a manual snapshot; `/checkpoint list`
    /// lists this session's checkpoints (oracle description covers both).
    fn checkpoint_command(
        &self,
        argument: &str,
        session_id: &str,
        workspace_root: &Path,
    ) -> String {
        let argument = argument.trim();
        if argument.eq_ignore_ascii_case("list") {
            return self.list_checkpoints(session_id);
        }
        // First checkpoint in a session seeds its lineage record so
        // `/sessions` and `/lineage` are immediately meaningful (the TUI
        // composition reaches the same state through its default session).
        self.ensure_lineage_session(session_id, workspace_root);
        let Ok(ledger) = vesper_checkpoints::CheckpointsLedger::open(&self.cron_root) else {
            return format!(
                "checkpoint: ledger unavailable (root {})",
                self.cron_root.display()
            );
        };
        let label = (!argument.is_empty()).then_some(argument);
        let parent_id = ledger
            .list()
            .iter()
            .rev()
            .find(|record| record.session_id == session_id)
            .map(|record| record.id.clone());
        match ledger.create(
            session_id,
            parent_id.as_deref(),
            CheckpointKind::Manual,
            label,
            workspace_root,
        ) {
            Ok(record) => format!(
                "checkpoint: {} captured {} file(s), {} byte(s){}",
                record.id,
                record.files.len(),
                record.total_bytes,
                record
                    .label
                    .as_ref()
                    .map(|label| format!(" — `{label}`"))
                    .unwrap_or_default()
            ),
            Err(error) => format!("checkpoint: failed — {error}"),
        }
    }

    fn list_checkpoints(&self, session_id: &str) -> String {
        let Ok(ledger) = vesper_checkpoints::CheckpointsLedger::open(&self.cron_root) else {
            return format!(
                "checkpoint: ledger unavailable (root {})",
                self.cron_root.display()
            );
        };
        let records: Vec<_> = ledger
            .list()
            .into_iter()
            .filter(|record| record.session_id == session_id)
            .take(50)
            .collect();
        if records.is_empty() {
            return "checkpoint: (no checkpoints recorded)".to_owned();
        }
        let mut lines = format!("checkpoint: {} checkpoint(s)", records.len());
        for record in &records {
            lines.push_str(&format!(
                "\n  {} {} file(s), {} byte(s){}",
                record.id,
                record.files.len(),
                record.total_bytes,
                record
                    .label
                    .as_ref()
                    .map(|label| format!(" — `{label}`"))
                    .unwrap_or_default()
            ));
        }
        lines
    }

    /// `/rollback [id]` restores the named checkpoint, or the most recent
    /// checkpoint of this session when no id is given (oracle: optional id).
    fn rollback_command(&self, argument: &str, session_id: &str, workspace_root: &Path) -> String {
        let Ok(ledger) = vesper_checkpoints::CheckpointsLedger::open(&self.cron_root) else {
            return format!(
                "rollback: ledger unavailable (root {})",
                self.cron_root.display()
            );
        };
        let id = argument.trim();
        let target = if id.is_empty() {
            match ledger
                .list()
                .iter()
                .rev()
                .find(|record| record.session_id == session_id)
            {
                Some(record) => record.id.clone(),
                None => {
                    return "rollback: no checkpoint to restore".to_owned();
                }
            }
        } else {
            id.to_owned()
        };
        match ledger.restore(&target, workspace_root) {
            Ok(restored) => format!("rollback: restored {restored} file(s) from {target}"),
            Err(error) => format!("rollback: failed — {error}"),
        }
    }

    /// `/undo [N]` rolls the workspace back to the prior checkpoint
    /// (mirrors the TUI composition's undo drain).
    fn undo_command(&self, argument: &str, session_id: &str, workspace_root: &Path) -> String {
        let Ok(ledger) = vesper_checkpoints::CheckpointsLedger::open(&self.cron_root) else {
            return format!(
                "undo: ledger unavailable (root {})",
                self.cron_root.display()
            );
        };
        let count = argument
            .trim()
            .parse::<usize>()
            .map(|value| value.max(1))
            .unwrap_or(1);
        let recent: Vec<_> = ledger
            .recent(count)
            .into_iter()
            .filter(|record| record.session_id == session_id)
            .collect();
        // The N-th most recent is the restore target (skip the most
        // recent, which is the current state).
        let target = recent.iter().rev().nth(1).or(recent.last());
        match target {
            Some(record) => match ledger.restore(&record.id, workspace_root) {
                Ok(restored) => {
                    format!(
                        "undo: rolled back to {} — restored {restored} file(s)",
                        record.id
                    )
                }
                Err(error) => format!("undo: failed — {error}"),
            },
            None => "undo: no prior checkpoint to roll back to".to_owned(),
        }
    }

    /// `/export` renders the conversation transcript as Markdown;
    /// `/export last` exports only the most recent assistant response.
    fn export_command(&self, argument: &str, session_id: &str, transcript: &[String]) -> String {
        let Ok(exporter) = vesper_checkpoints::SessionExporter::open(&self.cron_root) else {
            return format!(
                "export: exporter unavailable (root {})",
                self.cron_root.display()
            );
        };
        if argument.trim().eq_ignore_ascii_case("last") {
            return match exporter.export_last_response(transcript) {
                Ok(path) => format!("export last: wrote {}", path.display()),
                Err(vesper_checkpoints::CheckpointError::Unavailable("no response to export")) => {
                    "export last: no response to export".to_owned()
                }
                Err(error) => format!("export last: failed — {error}"),
            };
        }
        let lineage = vesper_checkpoints::SessionLineage::open(&self.cron_root)
            .map(|sessions| sessions.lineage(session_id))
            .unwrap_or_default();
        match exporter.export(transcript, &lineage) {
            Ok(path) => format!("export: wrote {}", path.display()),
            Err(error) => format!("export: failed — {error}"),
        }
    }

    /// Seeds a lineage record for `session_id` when none exists yet, so the
    /// ACP session appears in `/sessions` and `/lineage` without an explicit
    /// session-create command (the oracle catalog has no `/sessions-new`).
    fn ensure_lineage_session(&self, session_id: &str, workspace_root: &Path) {
        if let Ok(sessions) = vesper_checkpoints::SessionLineage::open(&self.cron_root)
            && lineage_record_id(&sessions, session_id).is_none()
        {
            let _ = sessions.create(None, Some(session_id), workspace_root);
        }
    }

    /// `/sessions [query]` lists recorded lineage sessions, optionally
    /// filtered by a case-insensitive query (oracle: browse or search).
    fn sessions_command(&self, argument: &str) -> String {
        let Ok(sessions) = vesper_checkpoints::SessionLineage::open(&self.cron_root) else {
            return format!(
                "sessions: lineage store unavailable (root {})",
                self.cron_root.display()
            );
        };
        let query = argument.trim().to_ascii_lowercase();
        let records: Vec<_> = sessions
            .list()
            .into_iter()
            .filter(|record| {
                query.is_empty()
                    || record.name.to_ascii_lowercase().contains(&query)
                    || record.id.to_ascii_lowercase().contains(&query)
            })
            .take(50)
            .collect();
        if records.is_empty() {
            return "sessions: (no sessions recorded)".to_owned();
        }
        let mut lines = format!("sessions: {} session(s)", records.len());
        for record in &records {
            lines.push_str(&format!(
                "\n  {} `{}` ({:?}) parent={:?}",
                record.id, record.name, record.status, record.parent_id
            ));
        }
        lines
    }

    /// `/lineage` shows this session's parent chain. The dispatch arm seeds
    /// the lineage record first, so the lookup resolves the record named
    /// for this ACP session.
    fn lineage_command(&self, session_id: &str) -> String {
        let Ok(sessions) = vesper_checkpoints::SessionLineage::open(&self.cron_root) else {
            return format!(
                "lineage: lineage store unavailable (root {})",
                self.cron_root.display()
            );
        };
        let Some(record_id) = lineage_record_id(&sessions, session_id) else {
            return format!("lineage: (no chain for {session_id})");
        };
        let chain = sessions.lineage(&record_id);
        if chain.is_empty() {
            return format!("lineage: (no chain for {session_id})");
        }
        let mut lines = format!("lineage: {} hop(s)", chain.len());
        for record in &chain {
            lines.push_str(&format!(
                "\n  {} `{}` ({:?})",
                record.id, record.name, record.status
            ));
        }
        lines
    }

    /// `/plugins [list|publishers|verify <path>|load <path>|trust <publisher> <key>]`.
    fn plugins_command(&self, argument: &str) -> String {
        let argument = argument.trim();
        if argument.is_empty() {
            return self.plugins_list();
        }
        let (sub, rest) = argument
            .split_once(char::is_whitespace)
            .unwrap_or((argument, ""));
        match sub {
            "list" => self.plugins_list(),
            "publishers" => {
                let publishers = self.trusted_publishers.list();
                if publishers.is_empty() {
                    return "plugins publishers: (none trusted)".to_owned();
                }
                let mut lines = format!("plugins publishers: {} trusted", publishers.len());
                for publisher in publishers.iter().take(50) {
                    let shown = &publisher.public_key_hex[..publisher.public_key_hex.len().min(16)];
                    lines.push_str(&format!("\n  `{}` key={shown}…", publisher.publisher));
                }
                lines
            }
            "verify" => {
                let path = rest.trim();
                if path.is_empty() {
                    return "Usage: /plugins verify <path>".to_owned();
                }
                let Some(loader) = self.plugin_loader.as_ref() else {
                    return format!(
                        "plugins verify: loader unavailable (root {})",
                        self.plugin_root.display()
                    );
                };
                match loader.verify(Path::new(path)) {
                    Ok(manifest) => format!(
                        "plugins verify: `{}` v{} by `{}` — signature VALID",
                        manifest.name, manifest.version, manifest.publisher
                    ),
                    Err(error) => format!("plugins verify: {path} — {error}"),
                }
            }
            "load" => {
                let path = rest.trim();
                if path.is_empty() {
                    return "Usage: /plugins load <path>".to_owned();
                }
                let Some(loader) = self.plugin_loader.as_ref() else {
                    return format!(
                        "plugins load: loader unavailable (root {})",
                        self.plugin_root.display()
                    );
                };
                match loader.load(Path::new(path)) {
                    Ok(record) => format!(
                        "plugins load: `{}` v{} by `{}` loaded ({})",
                        record.manifest.name, record.manifest.version, record.publisher, record.id
                    ),
                    Err(error) => format!("plugins load: {path} — {error}"),
                }
            }
            "trust" => {
                let mut parts = rest.split_whitespace();
                let publisher = parts.next().unwrap_or_default();
                let public_key_hex = parts.next().unwrap_or_default();
                if publisher.is_empty() || public_key_hex.is_empty() {
                    return "Usage: /plugins trust <publisher> <pubkey-hex>".to_owned();
                }
                let entry = vesper_mcp::TrustedPublisher {
                    publisher: publisher.to_owned(),
                    public_key_hex: public_key_hex.to_owned(),
                };
                match self.trusted_publishers.trust(entry.clone()) {
                    Ok(()) => {
                        // Persist to publishers.jsonl (best-effort append).
                        if let Ok(serialized) = serde_json::to_string(&entry)
                            && let Ok(mut file) = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(self.plugin_root.join("publishers.jsonl"))
                        {
                            use std::io::Write;
                            let _ = writeln!(file, "{serialized}");
                        }
                        format!("plugins trust: `{publisher}` now trusted")
                    }
                    Err(error) => format!("plugins trust: failed — {error}"),
                }
            }
            _ => format!(
                "Unknown /plugins subcommand: {sub}. Available: list, publishers, verify, load, trust."
            ),
        }
    }

    fn plugins_list(&self) -> String {
        let Some(loader) = self.plugin_loader.as_ref() else {
            return format!(
                "plugins: loader unavailable (root {})",
                self.plugin_root.display()
            );
        };
        let records = loader.list();
        if records.is_empty() {
            return "plugins: (no plugins loaded)".to_owned();
        }
        let mut lines = format!("plugins: {} plugin(s) loaded", records.len());
        for record in records.iter().take(50) {
            let signed = if record.unsigned_debug {
                "UNSIGNED(debug)"
            } else {
                "signed"
            };
            lines.push_str(&format!(
                "\n  {} `{}` v{} by `{}` ({signed})",
                record.id, record.manifest.name, record.manifest.version, record.publisher
            ));
        }
        lines
    }

    /// `/mcp [list|add <id> <command> [args...]|remove <id>|tools <id>]`.
    fn mcp_command(&self, argument: &str) -> String {
        let argument = argument.trim();
        if argument.is_empty() {
            return self.mcp_list();
        }
        let (sub, rest) = argument
            .split_once(char::is_whitespace)
            .unwrap_or((argument, ""));
        match sub {
            "list" => self.mcp_list(),
            "add" => {
                let rest = rest.trim();
                if rest.is_empty() {
                    return "Usage: /mcp add <id> <command> [args...]".to_owned();
                }
                let mut parts = rest.split_whitespace();
                let id = parts.next().unwrap_or_default();
                let command = parts.next().unwrap_or_default();
                if id.is_empty() || command.is_empty() {
                    return "Usage: /mcp add <id> <command> [args...]".to_owned();
                }
                let config = vesper_mcp::McpServerConfig {
                    id: id.to_owned(),
                    transport: vesper_mcp::McpTransport::Stdio,
                    command: Some(command.to_owned()),
                    args: parts.map(String::from).collect(),
                    url: None,
                    auth_env: None,
                    label: None,
                    created_at: std::time::SystemTime::UNIX_EPOCH,
                };
                let Ok(registry) = vesper_mcp::McpRegistry::open(&self.plugin_root) else {
                    return format!(
                        "mcp add: registry unavailable (root {})",
                        self.plugin_root.display()
                    );
                };
                match registry.add(config) {
                    Ok(added) => format!("mcp add: registered `{}`", added.id),
                    Err(error) => format!("mcp add: failed — {error}"),
                }
            }
            "remove" => {
                let id = rest.trim();
                if id.is_empty() {
                    return "Usage: /mcp remove <id>".to_owned();
                }
                let Ok(registry) = vesper_mcp::McpRegistry::open(&self.plugin_root) else {
                    return format!(
                        "mcp remove: registry unavailable (root {})",
                        self.plugin_root.display()
                    );
                };
                match registry.remove(id) {
                    Ok(true) => format!("mcp remove: unregistered `{id}`"),
                    Ok(false) => format!("mcp remove: `{id}` was not registered"),
                    Err(error) => format!("mcp remove: failed — {error}"),
                }
            }
            "tools" => {
                let id = rest.trim();
                if id.is_empty() {
                    return "Usage: /mcp tools <id>".to_owned();
                }
                let Ok(registry) = vesper_mcp::McpRegistry::open(&self.plugin_root) else {
                    return format!(
                        "mcp tools: registry unavailable (root {})",
                        self.plugin_root.display()
                    );
                };
                let Some(config) = registry.get(id) else {
                    return format!("mcp tools: `{id}` is not registered");
                };
                match vesper_mcp::McpClient::tools(&config) {
                    Ok(tools) if tools.is_empty() => {
                        format!("mcp tools: `{id}` advertised no tools")
                    }
                    Ok(tools) => {
                        let mut lines =
                            format!("mcp tools: `{id}` advertised {} tool(s)", tools.len());
                        for tool in tools.iter().take(50) {
                            let desc = tool.description.as_deref().unwrap_or("");
                            lines.push_str(&format!("\n  - {} {desc}", tool.name));
                        }
                        lines
                    }
                    Err(error) => format!("mcp tools: `{id}` failed — {error}"),
                }
            }
            _ => format!("Unknown /mcp subcommand: {sub}. Available: list, add, remove, tools."),
        }
    }

    fn mcp_list(&self) -> String {
        let Ok(registry) = vesper_mcp::McpRegistry::open(&self.plugin_root) else {
            return format!(
                "mcp: registry unavailable (root {})",
                self.plugin_root.display()
            );
        };
        let servers = registry.list();
        if servers.is_empty() {
            return "mcp: (no servers configured)".to_owned();
        }
        let mut lines = format!("mcp: {} server(s)", servers.len());
        for server in servers.iter().take(50) {
            let cmd = server.command.as_deref().unwrap_or("(no command)");
            lines.push_str(&format!(
                "\n  {} [{:?}] `{}` {}",
                server.id,
                server.transport,
                cmd,
                server.args.join(" ")
            ));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn service_under(checkpoints: &std::path::Path, mcp: &std::path::Path) -> HarnessToolService {
        let local_memory = tempfile::tempdir().unwrap();
        let global_memory = tempfile::tempdir().unwrap();
        HarnessToolService::new(
            Arc::new(crate::MemoryStores::open_at(
                local_memory.path(),
                global_memory.path(),
            )),
            checkpoints.to_path_buf(),
            mcp.to_path_buf(),
            None,
        )
    }

    #[test]
    fn checkpoint_gate_disables_durable_state_and_answers_truthfully() {
        let root = tempfile::tempdir().unwrap();
        let checkpoints = root.path().join("checkpoints");
        let mcp = root.path().join("mcp");
        let local_memory = tempfile::tempdir().unwrap();
        let global_memory = tempfile::tempdir().unwrap();
        let service = HarnessToolService::new_with_checkpoint_gate(
            Arc::new(crate::MemoryStores::open_at(
                local_memory.path(),
                global_memory.path(),
            )),
            checkpoints.clone(),
            mcp.clone(),
            None,
            false,
        );
        let workspace = tempfile::tempdir().unwrap();
        for command in ["checkpoint", "rollback", "undo", "sessions", "lineage"] {
            let output = service.execute_host_command(
                command,
                "",
                "sess-gated",
                workspace.path(),
                &[],
            );
            assert!(
                output.contains("disabled by default"),
                "/{command} should explain the opt-in, got: {output}"
            );
            assert!(
                output.contains("AGENT_VESPER_ENABLE_CHECKPOINTS=1"),
                "/{command} should name the enabling variable, got: {output}"
            );
        }
        // Read-only and separate-subsystem commands stay functional.
        assert!(service.execute_host_command("ci", "", "s", workspace.path(), &[]).starts_with("ci:"));
        assert!(service.execute_host_command("plugins", "list", "s", workspace.path(), &[]).len() > 4);
        // The gate must not create the durable checkpoint root.
        assert!(
            !checkpoints.exists(),
            "gated service must not create the checkpoint root"
        );
    }

    #[test]
    fn checkpoint_gate_enabled_path_still_creates_the_roots() {
        let root = tempfile::tempdir().unwrap();
        let checkpoints = root.path().join("checkpoints");
        let local_memory = tempfile::tempdir().unwrap();
        let global_memory = tempfile::tempdir().unwrap();
        let _service = HarnessToolService::new_with_checkpoint_gate(
            Arc::new(crate::MemoryStores::open_at(
                local_memory.path(),
                global_memory.path(),
            )),
            checkpoints.clone(),
            root.path().join("mcp"),
            None,
            true,
        );
        assert!(
            checkpoints.exists(),
            "enabled service creates the checkpoint root"
        );
    }

    fn workspace_with_file(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join(format!("{tag}.txt"));
        std::fs::write(&file, format!("original {tag}")).unwrap();
        (workspace, file)
    }

    #[test]
    fn checkpoint_create_and_rollback_round_trip() {
        let roots = tempfile::tempdir().unwrap();
        let service = service_under(&roots.path().join("checkpoints"), &roots.path().join("mcp"));
        let (workspace, file) = workspace_with_file("round-trip");

        let created =
            service.execute_host_command("checkpoint", "", "sess-acp", workspace.path(), &[]);
        assert!(
            created.starts_with("checkpoint: ") && created.contains("captured"),
            "got {created}"
        );
        let checkpoint_id = created
            .split_whitespace()
            .nth(1)
            .expect("checkpoint id token")
            .to_owned();

        // Mutate the workspace, then roll back without an explicit id.
        std::fs::write(&file, "mutated").unwrap();
        let rolled =
            service.execute_host_command("rollback", "", "sess-acp", workspace.path(), &[]);
        assert!(
            rolled.starts_with("rollback: restored") && rolled.contains(&checkpoint_id),
            "got {rolled}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "original round-trip"
        );
    }

    #[test]
    fn checkpoint_list_and_undo_report_truthfully() {
        let roots = tempfile::tempdir().unwrap();
        let service = service_under(&roots.path().join("checkpoints"), &roots.path().join("mcp"));
        let (workspace, _file) = workspace_with_file("listing");

        let empty =
            service.execute_host_command("checkpoint", "list", "sess-acp", workspace.path(), &[]);
        assert_eq!(empty, "checkpoint: (no checkpoints recorded)");

        let undo_before =
            service.execute_host_command("undo", "", "sess-acp", workspace.path(), &[]);
        assert_eq!(undo_before, "undo: no prior checkpoint to roll back to");

        service.execute_host_command("checkpoint", "", "sess-acp", workspace.path(), &[]);
        let listed =
            service.execute_host_command("checkpoint", "list", "sess-acp", workspace.path(), &[]);
        assert!(
            listed.starts_with("checkpoint: 1 checkpoint(s)"),
            "got {listed}"
        );
    }

    #[test]
    fn export_writes_the_transcript_as_markdown() {
        let roots = tempfile::tempdir().unwrap();
        let service = service_under(&roots.path().join("checkpoints"), &roots.path().join("mcp"));
        let (workspace, _file) = workspace_with_file("export");
        let transcript = vec![
            "user: fix the parity gap".to_owned(),
            "assistant: wired and verified".to_owned(),
        ];
        let exported =
            service.execute_host_command("export", "", "sess-acp", workspace.path(), &transcript);
        assert!(exported.starts_with("export: wrote "), "got {exported}");
        let path = exported.strip_prefix("export: wrote ").unwrap();
        let body = std::fs::read_to_string(path).expect("export file exists");
        assert!(body.contains("fix the parity gap"));
        assert!(body.contains("wired and verified"));

        let last = service.execute_host_command(
            "export",
            "last",
            "sess-acp",
            workspace.path(),
            &transcript,
        );
        assert!(last.starts_with("export last: wrote "), "got {last}");
        let last_path = last.strip_prefix("export last: wrote ").unwrap();
        let last_body = std::fs::read_to_string(last_path).unwrap();
        assert!(last_body.contains("wired and verified"));
        assert!(!last_body.contains("fix the parity gap"));
    }

    #[test]
    fn sessions_and_lineage_round_trip() {
        let roots = tempfile::tempdir().unwrap();
        let service = service_under(&roots.path().join("checkpoints"), &roots.path().join("mcp"));
        let (workspace, _file) = workspace_with_file("lineage");

        let empty = service.execute_host_command("sessions", "", "sess-acp", workspace.path(), &[]);
        assert_eq!(empty, "sessions: (no sessions recorded)");
        // A fresh session's /lineage seeds its own record and reports the
        // (single-hop) chain — the oracle's "this session's lineage" view.
        let seeded = service.execute_host_command("lineage", "", "sess-acp", workspace.path(), &[]);
        assert!(
            seeded.starts_with("lineage: 1 hop(s)") && seeded.contains("sess-acp"),
            "got {seeded}"
        );

        // A checkpoint keeps using the same record (no duplicate seeds).
        service.execute_host_command("checkpoint", "", "sess-acp", workspace.path(), &[]);
        let listed =
            service.execute_host_command("sessions", "", "sess-acp", workspace.path(), &[]);
        assert!(
            listed.starts_with("sessions: 1 session(s)") && listed.contains("sess-acp"),
            "got {listed}"
        );
        let chain = service.execute_host_command("lineage", "", "sess-acp", workspace.path(), &[]);
        assert!(chain.starts_with("lineage: 1 hop(s)"), "got {chain}");
        assert!(chain.contains("sess-acp"));
    }

    #[test]
    fn mcp_add_list_remove_round_trip() {
        let roots = tempfile::tempdir().unwrap();
        let service = service_under(&roots.path().join("checkpoints"), &roots.path().join("mcp"));
        let (workspace, _file) = workspace_with_file("mcp");

        let empty = service.execute_host_command("mcp", "", "s", workspace.path(), &[]);
        assert_eq!(empty, "mcp: (no servers configured)");

        let added = service.execute_host_command(
            "mcp",
            "add test-server /bin/echo --flag",
            "s",
            workspace.path(),
            &[],
        );
        assert_eq!(added, "mcp add: registered `test-server`");
        let listed = service.execute_host_command("mcp", "list", "s", workspace.path(), &[]);
        assert!(listed.starts_with("mcp: 1 server(s)"), "got {listed}");
        assert!(listed.contains("test-server"));

        let removed =
            service.execute_host_command("mcp", "remove test-server", "s", workspace.path(), &[]);
        assert_eq!(removed, "mcp remove: unregistered `test-server`");
    }

    #[test]
    fn plugins_and_ci_report_real_state() {
        let roots = tempfile::tempdir().unwrap();
        let service = service_under(&roots.path().join("checkpoints"), &roots.path().join("mcp"));
        let (workspace, _file) = workspace_with_file("plugins");

        let plugins = service.execute_host_command("plugins", "", "s", workspace.path(), &[]);
        assert_eq!(plugins, "plugins: (no plugins loaded)");
        let publishers =
            service.execute_host_command("plugins", "publishers", "s", workspace.path(), &[]);
        assert_eq!(publishers, "plugins publishers: (none trusted)");

        let ci = service.execute_host_command("ci", "", "s", workspace.path(), &[]);
        assert!(ci.starts_with("ci: "), "got {ci}");
    }

    #[test]
    fn unknown_host_command_falls_back_truthfully() {
        let roots = tempfile::tempdir().unwrap();
        let service = service_under(&roots.path().join("checkpoints"), &roots.path().join("mcp"));
        let (workspace, _file) = workspace_with_file("fallback");
        let text = service.execute_host_command("compact", "", "s", workspace.path(), &[]);
        assert!(text.contains("host-owned command"), "got {text}");
    }
}
