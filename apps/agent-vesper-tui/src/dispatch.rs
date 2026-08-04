//! Event-loop dispatch — the pure bridge between the command registry, the
//! Plan Mode state machine, the superpower override store, and the transcript.
//!
//! This module owns **no** terminal I/O. The production binary's event loop
//! owns the crossterm input buffer, the raw-mode lifecycle, and the
//! [`ratatui`] frame; it hands each parsed [`CommandIntent`] to [`dispatch`],
//! which mutates a [`SessionState`] in place and reports whether to continue
//! or quit. Keeping the dispatch logic here lets the full Plan Mode lifecycle
//! (`NORMAL → PLANNING → REVIEW → EXECUTING`) be exercised by integration
//! tests under a [`crate::ui::StubRenderer`] without ever touching a terminal.
//!
//! ## Driver/Navigator contract
//!
//! The Plan Mode state machine is pure: it owns *transition discipline*, not
//! reasoning. Under ADR 0010 (Tier C Phase 5) the model drives the
//! `PLANNING → REVIEW` transition by emitting the `update_plan` tool; the
//! agent loop surfaces the rendered plan and [`apply_model_plan`] finalizes
//! it for review. The human-authored `/review <body>` placeholder is retired.
//! Free text typed while a question is pending is routed to
//! [`PlanState::answer`](crate::plan_mode::PlanState::answer) so the driver
//! can refine the plan inline.

use vesper_domain::{BoundedString, ProviderId, SessionOperatingMode, SessionPermissionMode};
use vesper_provider::SuperpowerValue;

use crate::commands::{
    CommandIntent, CommandOutcome, CommandRegistry, PlanGesture, SessionConfigKey, UiAction,
};
use crate::plan_mode::{PlanPhase, PlanState};
use crate::superpowers::{ProviderSuperpowerSurface, SuperpowerOverrides};

/// Pure, terminal-free session state mutated by [`dispatch`].
///
/// The binary wraps this together with an `input: String` buffer that never
/// crosses the [`dispatch`] boundary; only the parsed [`CommandIntent`] does.
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    /// Plan Mode state machine.
    pub plan: PlanState,
    /// Active superpower overrides, mutated by `/effort`, `/thinking`,
    /// `/model`. Surfaced to the renderer and applied to every next provider
    /// request by the binary composition boundary.
    pub overrides: SuperpowerOverrides,
    /// Visible transcript lines, oldest first.
    pub transcript: Vec<String>,
    /// One-line status / error / notice. `None` clears the status line.
    pub status: Option<String>,
    /// A pending session-reasoning update produced by `/thinking <level>`
    /// (ADR 0009). The binary's async event loop drains this into the runtime
    /// `UpdateSessionReasoning` command after each dispatch; `None` means no
    /// update is pending.
    pub pending_reasoning: Option<BoundedString<128>>,
    /// Phase 7 (ADR 0010): a workflow command (`/security-review`, `/smart`,
    /// `/release`, `/insights`, `/diff`) built a prompt that should drive the
    /// next background `AgentLoop` turn. The binary drains this after dispatch
    /// alongside free-text prompts; `None` means no workflow prompt is
    /// pending. Keeping the prompt here (rather than returning it from
    /// `dispatch`) preserves the existing `dispatch -> DispatchOutcome`
    /// signature and matches the `pending_reasoning` drain pattern.
    pub pending_prompt: Option<String>,
    /// Phase 8 (ADR 0011): a memory command (`/memory`, `/goal`, `/skills`,
    /// `/profile`, `/awareness`, etc.) resolved to a structured
    /// [`crate::commands::MemoryOp`]. The binary owns the durable
    /// `vesper_memory::MemoryStore` / `SkillStore` / `UserProfile` /
    /// `AwarenessLedger` and drains this after dispatch (same pattern as
    /// `pending_prompt`); `None` means no memory op is pending.
    pub pending_memory_op: Option<crate::commands::MemoryOp>,
    /// Phase 9 (ADR 0012): a checkpoint command (`/sessions`,
    /// `/checkpoint`, `/rollback`, `/undo`, `/loop`, `/export`, `/copy`,
    /// `/ci`, etc.) resolved to a structured
    /// [`crate::commands::CheckpointOp`]. The binary owns the durable
    /// `vesper_checkpoints::CheckpointsLedger` / `SessionLineage` /
    /// `CronRegistry` / `SessionExporter` / `ClipboardPort` /
    /// `CiStatusReader` and drains this after dispatch (same pattern as
    /// `pending_memory_op`); `None` means no checkpoint op is pending.
    pub pending_checkpoint_op: Option<crate::commands::CheckpointOp>,
    /// Phase 10 (ADR 0013): an MCP or plugins command (`/mcp`, `/plugins`)
    /// resolved to a structured [`crate::commands::McpOp`]. The binary
    /// owns the durable `vesper_mcp::McpRegistry` / `McpClient` /
    /// `PluginLoader` / `TrustedPublishers` and drains this after
    /// dispatch; `None` means no MCP op is pending.
    pub pending_mcp_op: Option<crate::commands::McpOp>,
    /// Phase 11 (ADR 0015 — Stage 16): a cognitive-memory command
    /// resolved to a structured [`crate::commands::CognitionOp`].
    pub pending_cognition_op: Option<crate::commands::CognitionOp>,
    /// Live execution controls used by both the picker UI and every agent turn.
    pub controls: SessionControls,
    /// Pending runtime mode synchronization after `/permission` or `/mode`.
    pub pending_mode_update: Option<(SessionOperatingMode, SessionPermissionMode)>,
    /// Latest model-authored TODO plan, independent of Plan Mode phase.
    pub task_plan: Vec<TaskItem>,
    /// Native terminal panel visibility.
    pub panels: PanelVisibility,
    /// Native presentation and terminal-integration preferences.
    pub preferences: TerminalPreferences,
    /// Crossterm side effect for the binary to drain after pure dispatch.
    pub pending_terminal_action: Option<TerminalAction>,
    /// Persisted session selected through `/history` for the binary to load.
    pub pending_history_session: Option<String>,
    /// Whether the binary should temporarily suspend the TUI and run $EDITOR.
    pub pending_prompt_editor: bool,
    /// Whether to open the real working-tree diff annotation editor.
    pub pending_diff_annotator: bool,
    /// Toggle request for the optional mobile approval server.
    pub pending_mobile_toggle: bool,
    /// Whether to edit and live-reload persistent keybindings.
    pub pending_keybind_editor: bool,
    /// Native image operation for the binary to execute after dispatch.
    pub pending_media_op: Option<crate::commands::MediaOp>,
    /// Side question to execute with the configured auxiliary model.
    pub pending_auxiliary_question: Option<String>,
    /// Whether to query the selected provider's real quota endpoint.
    pub pending_provider_usage: bool,
    /// Whether the binary should calculate the oracle-compatible live context estimate.
    pub pending_context_report: bool,
    /// Selected fenced-code block action, executed by the binary.
    pub pending_code_block: Option<(usize, bool)>,
    /// Whether the binary should re-open the provider-routed authentication
    /// screen (`/auth`) using the active provider's advertised descriptor.
    pub pending_reauth: bool,
    /// Manual conversation scroll expressed as **lines up from the bottom**.
    /// `None` = auto-follow (stick to bottom, the default); `Some(n)` = the
    /// user pressed PageUp/Home and is reading history `n` lines above the
    /// newest line. Tracking from the bottom (rather than absolute offset
    /// from the top) means the input handler can update this without knowing
    /// `max_scroll`, which only the renderer can compute from the wrapped
    /// markdown line count. The renderer mirrors this into a `ScrollbarState`
    /// so the visual scrollbar reflects the same position the
    /// `Paragraph::scroll` call uses. Reset to `None` by `End`, a new prompt
    /// submission, or PageDown/ScrollDown reaching the bottom.
    pub conversation_manual_scroll: Option<u16>,
    /// Currently focused action button in the tool-permission modal. Defaults
    /// to `Allow` (the conservative pick); Tab/Left/Right toggles between
    /// `Deny` and `Allow`. Mirrored into the renderer's
    /// `ViewModel::pending_permission` only while a request is pending.
    pub permission_modal_focus: crate::ui::PermissionChoice,
}

/// Typed session controls. Defaults match the Python oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionControls {
    pub endpoint_plan: String,
    pub permission_mode: SessionPermissionMode,
    pub operating_mode: SessionOperatingMode,
    pub generation_profile: String,
    pub auxiliary_model: String,
    pub mixture_mode: String,
    pub max_tool_iterations: u32,
}

impl Default for SessionControls {
    fn default() -> Self {
        Self {
            endpoint_plan: "coding".into(),
            permission_mode: SessionPermissionMode::Ask,
            operating_mode: SessionOperatingMode::Code,
            generation_profile: "balanced".into(),
            auxiliary_model: "main".into(),
            mixture_mode: "off".into(),
            max_tool_iterations: vesper_agent::DEFAULT_MAX_TOOL_ITERATIONS,
        }
    }
}

/// User-controlled dashboard panel visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelVisibility {
    pub reasoning: bool,
    pub tasks: bool,
    pub sidebar: bool,
}

impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            reasoning: true,
            tasks: true,
            sidebar: true,
        }
    }
}

/// Native terminal preferences which immediately affect rendering/feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalPreferences {
    pub theme: String,
    pub screen_reader: bool,
    pub native_mouse: bool,
    pub sound: bool,
    pub vim: bool,
    pub vim_mode: String,
    pub composer_cursor: usize,
    pub vim_pending_operator: Option<char>,
    pub vim_pending_g: bool,
    pub vim_clipboard: String,
    pub vim_undo: String,
    pub vim_visual_anchor: usize,
}

impl Default for TerminalPreferences {
    fn default() -> Self {
        Self {
            theme: "vesper".into(),
            screen_reader: false,
            native_mouse: false,
            sound: false,
            vim: false,
            vim_mode: "insert".into(),
            composer_cursor: 0,
            vim_pending_operator: None,
            vim_pending_g: false,
            vim_clipboard: String::new(),
            vim_undo: String::new(),
            vim_visual_anchor: 0,
        }
    }
}

/// Terminal side effects kept out of the pure dispatch module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalAction {
    EnableMouseCapture,
    DisableMouseCapture,
}

/// One model-authored task rendered in the native TODO panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskItem {
    pub content: String,
    pub status: String,
    pub priority: String,
}

impl SessionState {
    /// Creates an empty session (NORMAL phase, no overrides, no transcript).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current Plan Mode phase — convenience pass-through for the renderer.
    #[must_use]
    pub fn phase(&self) -> PlanPhase {
        self.plan.phase()
    }
}

/// What [`dispatch`] decided after applying one input line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// State was mutated in place; keep running the loop.
    Continue,
    /// The user requested to quit (`/quit`, `/exit`).
    Quit,
}

/// Resolves and applies one parsed input line.
///
/// The active provider and its superpower surface come from the composition
/// boundary; superpower commands are resolved against `surface.descriptors()`
/// filtered to `provider_id`. Returns [`DispatchOutcome::Quit`] when the user
/// typed `/quit`; otherwise mutates `state` in place and returns
/// [`DispatchOutcome::Continue`].
#[must_use = "the caller must act on Quit"]
pub fn dispatch(
    intent: &CommandIntent,
    registry: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    provider_id: &ProviderId,
    state: &mut SessionState,
) -> DispatchOutcome {
    let outcome = registry.resolve(intent, &state.plan, provider_id, surface.descriptors());
    match outcome {
        CommandOutcome::Quit => DispatchOutcome::Quit,
        other => {
            apply_outcome(other, surface, state);
            DispatchOutcome::Continue
        }
    }
}

/// Phase 5 bridge (ADR 0010): drives `PLANNING → REVIEW` from a **model-
/// generated** plan body.
///
/// The agent loop's `update_plan` executor writes `.agent/plan.md` and the loop
/// surfaces the rendered plan in `AgentTurnOutcome::plan`. The TUI's event loop
/// calls this with that body so the human reviews the model-authored plan
/// (retiring the human-authored `/review` placeholder). Returns the Plan Mode
/// transition so the caller can render it.
pub fn apply_model_plan(state: &mut SessionState, plan_body: &str) -> crate::PlanTransition {
    let SessionState {
        plan,
        transcript,
        status,
        ..
    } = state;
    match plan.finalize(plan_body) {
        Ok(transition) => {
            transcript.push(format!(
                "plan: REVIEW (model plan, {} bytes)",
                plan_body.len()
            ));
            *status = Some("Plan under review — /approve to execute, /cancel to abort.".into());
            transition
        }
        Err(error) => {
            *status = Some(error.to_string());
            crate::PlanTransition::Notice(
                vesper_domain::SafeMessage::new("plan could not be finalized")
                    .expect("static notice is bounded"),
            )
        }
    }
}

/// Applies a resolved [`CommandOutcome`] to `state`.
///
/// Pure: no I/O, no async, no terminal. The Plan Mode transitions, override
/// store, transcript, and status line are all updated here so the loop body in
/// the binary stays a thin key→intent→dispatch shim.
fn apply_outcome(
    outcome: CommandOutcome,
    surface: &ProviderSuperpowerSurface,
    state: &mut SessionState,
) {
    let SessionState {
        plan,
        overrides,
        transcript,
        status,
        pending_reasoning,
        pending_prompt,
        pending_memory_op,
        pending_checkpoint_op,
        pending_mcp_op,
        pending_cognition_op,
        controls,
        pending_mode_update,
        task_plan: _,
        panels,
        preferences,
        pending_terminal_action,
        pending_history_session,
        pending_prompt_editor,
        pending_diff_annotator,
        pending_mobile_toggle,
        pending_keybind_editor,
        pending_media_op,
        pending_auxiliary_question,
        pending_provider_usage,
        pending_context_report,
        pending_code_block,
        pending_reauth,
        conversation_manual_scroll: _,
        permission_modal_focus: _,
    } = state;
    match outcome {
        CommandOutcome::Error(message) => {
            *status = Some(message);
        }
        CommandOutcome::Prompt(text) => {
            // During PLANNING the driver answers the oldest pending question
            // inline. Outside PLANNING (or when nothing is pending) the text
            // is a normal free-text utterance routed to the runtime.
            if plan.phase() == PlanPhase::Planning && !plan.pending_questions().is_empty() {
                match plan.answer(&text) {
                    Ok(_) => *status = Some("Answer recorded; interrogation continues.".into()),
                    Err(error) => *status = Some(error.to_string()),
                }
            } else {
                transcript.push(format!("user: {text}"));
                *status = None;
            }
        }
        CommandOutcome::Plan { prd } => match plan.start(&prd) {
            Ok(_) => {
                transcript.push(format!("plan: entered PLANNING ({} bytes)", prd.len()));
                *status = Some("Plan Mode active — the model plans via update_plan.".into());
            }
            Err(error) => *status = Some(error.to_string()),
        },
        CommandOutcome::PlanGesture(gesture) => match gesture {
            PlanGesture::Approve => match plan.approve() {
                Ok(_) => *status = Some("Plan approved — EXECUTING.".into()),
                Err(error) => *status = Some(error.to_string()),
            },
            PlanGesture::Cancel => {
                plan.cancel();
                *status = Some("Plan cancelled.".into());
            }
        },
        CommandOutcome::Superpower {
            descriptor, value, ..
        } => {
            if descriptor.id.as_str() == "zai:model"
                && let SuperpowerValue::Choice { value: model } = &value
                && !vesper_provider_glm::GlmCatalog::supports_plan(
                    model.as_str(),
                    glm_plan(&controls.endpoint_plan),
                )
            {
                *status = Some(format!(
                    "Model `{}` is unavailable on the {} API plan.",
                    model.as_str(),
                    controls.endpoint_plan
                ));
                return;
            }
            if descriptor.id.as_str() == "zai:model"
                && let SuperpowerValue::Choice { value: model } = &value
                && model.as_str() != "glm-5.2"
                && let Some(thinking) = surface.by_alias("thinking")
                && let Some(SuperpowerValue::Choice { value: current }) =
                    overrides.get(thinking.id.as_str(), Some(&thinking.default_value))
                && matches!(current.as_str(), "high" | "max")
            {
                let enabled = SuperpowerValue::Choice {
                    value: BoundedString::new("enabled").expect("static mode is bounded"),
                };
                overrides.set(thinking.id.as_str(), enabled.clone());
                if thinking.id.as_str() == "zai:reasoning" {
                    *pending_reasoning = Some(
                        vesper_provider_glm::reasoning_mode_for_superpower(&enabled)
                            .expect("static reasoning mode is valid"),
                    );
                }
                transcript.push("thinking reset to enabled for the selected model".into());
            }
            // Persist the override so it surfaces in the renderer.
            overrides.set(descriptor.id.as_str(), value.clone());

            // ADR 0009: when the resolved superpower is the reconciled
            // `zai:reasoning` dial, translate the value into the runtime
            // reasoning-mode label the binary will push through the
            // `UpdateSessionReasoning` command. The mapping lives in the GLM
            // crate (the TUI is a GLM composition boundary); other providers
            // would supply their own mapper when registered.
            if descriptor.id.as_str() == "zai:reasoning" {
                match vesper_provider_glm::reasoning_mode_for_superpower(&value) {
                    Ok(mode) => *pending_reasoning = Some(mode),
                    Err(error) => {
                        *status = Some(format!("invalid reasoning value: {error}"));
                        return;
                    }
                }
            }

            transcript.push(format!(
                "superpower: {} set to {}",
                descriptor.display_name.as_str(),
                format_superpower_value(&value)
            ));
            // Surface the active override set so the driver sees the layer
            // was updated; the renderer also shows every active override.
            let count = surface
                .descriptors()
                .iter()
                .filter(|descriptor| overrides.get(descriptor.id.as_str(), None).is_some())
                .count();
            *status = Some(format!("{count} active superpower override(s)."));
        }
        CommandOutcome::Help(text) => transcript.push(text),
        CommandOutcome::Version => {
            transcript.push(format!("agent-vesper {}", env!("CARGO_PKG_VERSION")));
            *status = Some(format!("v{}", env!("CARGO_PKG_VERSION")));
        }
        CommandOutcome::ClearView => {
            transcript.clear();
            *status = Some("Transcript cleared.".into());
        }
        CommandOutcome::SessionConfig { key, value } => {
            match key {
                SessionConfigKey::EndpointPlan => {
                    if surface.provider_id().as_str() != "zai" {
                        *status = Some("API plans are owned by the active provider.".into());
                        return;
                    }
                    controls.endpoint_plan = value.clone();
                    if let Some(descriptor) = surface.by_alias("model") {
                        let active =
                            overrides.get(descriptor.id.as_str(), Some(&descriptor.default_value));
                        if let Some(SuperpowerValue::Choice { value: model }) = active
                            && !vesper_provider_glm::GlmCatalog::supports_plan(
                                model.as_str(),
                                glm_plan(&value),
                            )
                        {
                            overrides.set(
                                descriptor.id.as_str(),
                                SuperpowerValue::Choice {
                                    value: BoundedString::new("glm-5.2")
                                        .expect("static model is bounded"),
                                },
                            );
                            transcript.push(format!(
                                "model reset to glm-5.2 because `{}` is unavailable on {value}",
                                model.as_str()
                            ));
                        }
                    }
                    if controls.auxiliary_model != "main"
                        && (!vesper_provider_glm::GlmCatalog::supports_plan(
                            &controls.auxiliary_model,
                            glm_plan(&value),
                        ) || vesper_provider_glm::GlmCatalog::is_vision_model(
                            &controls.auxiliary_model,
                        ))
                    {
                        controls.auxiliary_model = "main".into();
                    }
                }
                SessionConfigKey::Permission => {
                    controls.permission_mode = match value.as_str() {
                        "read" => SessionPermissionMode::ReadOnly,
                        "bypass" => SessionPermissionMode::Bypass,
                        _ => SessionPermissionMode::Ask,
                    };
                    *pending_mode_update =
                        Some((controls.operating_mode, controls.permission_mode));
                }
                SessionConfigKey::OperatingMode => {
                    controls.operating_mode = if value == "code" {
                        SessionOperatingMode::Code
                    } else {
                        SessionOperatingMode::Plan
                    };
                    *pending_mode_update =
                        Some((controls.operating_mode, controls.permission_mode));
                }
                SessionConfigKey::GenerationProfile => {
                    controls.generation_profile = value.clone();
                }
                SessionConfigKey::AuxiliaryModel => {
                    controls.auxiliary_model = value.clone();
                }
                SessionConfigKey::MixtureMode => controls.mixture_mode = value.clone(),
                SessionConfigKey::Theme => preferences.theme = value.clone(),
                SessionConfigKey::MaxIterations => {
                    controls.max_tool_iterations = value
                        .parse()
                        .expect("command resolver validates max iterations");
                }
            }
            transcript.push(format!("session setting: {key:?} → {value}"));
            *status = Some(format!("Updated {key:?} to {value}."));
        }
        CommandOutcome::Ui(action) => match action {
            UiAction::OpenSettings => {
                *status = Some("Select a setting, then choose its value.".into());
            }
            UiAction::OpenAuth => {
                *pending_reauth = true;
                *status = Some("Opening provider authentication…".into());
            }
            UiAction::ToggleReasoning => {
                panels.reasoning = !panels.reasoning;
                *status = Some(format!(
                    "Reasoning panel {}.",
                    if panels.reasoning { "shown" } else { "hidden" }
                ));
            }
            UiAction::ToggleTasks => {
                panels.tasks = !panels.tasks;
                *status = Some(format!(
                    "TODO panel {}.",
                    if panels.tasks { "shown" } else { "hidden" }
                ));
            }
            UiAction::ToggleSidebar => {
                panels.sidebar = !panels.sidebar;
                *status = Some(format!(
                    "Session sidebar {}.",
                    if panels.sidebar { "shown" } else { "hidden" }
                ));
            }
            UiAction::ToggleScreenReader => {
                preferences.screen_reader = !preferences.screen_reader;
                *status = Some(format!(
                    "Screen-reader mode {}.",
                    if preferences.screen_reader {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ));
            }
            UiAction::ToggleNativeMouse => {
                preferences.native_mouse = !preferences.native_mouse;
                *pending_terminal_action = Some(if preferences.native_mouse {
                    TerminalAction::DisableMouseCapture
                } else {
                    TerminalAction::EnableMouseCapture
                });
                *status = Some(format!(
                    "Native terminal mouse {}.",
                    if preferences.native_mouse {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ));
            }
            UiAction::ToggleSound => {
                preferences.sound = !preferences.sound;
                *status = Some(format!(
                    "Completion sound {}.",
                    if preferences.sound {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ));
            }
            UiAction::OpenPromptEditor => {
                *pending_prompt_editor = true;
                *status = Some("Opening $VISUAL/$EDITOR…".into());
            }
            UiAction::OpenDiffAnnotator => {
                *pending_diff_annotator = true;
                *status = Some("Opening working-tree diff annotator…".into());
            }
            UiAction::ToggleVim => {
                preferences.vim = !preferences.vim;
                preferences.vim_mode = if preferences.vim { "normal" } else { "insert" }.into();
                preferences.vim_pending_operator = None;
                *status = Some(if preferences.vim {
                    "Vim composer enabled — press i to edit.".into()
                } else {
                    "Vim composer disabled.".into()
                });
            }
            UiAction::ToggleMobile => {
                *pending_mobile_toggle = true;
                *status = Some("Toggling mobile approval companion…".into());
            }
            UiAction::OpenKeybindEditor => {
                *pending_keybind_editor = true;
                *status = Some("Opening keybinding editor…".into());
            }
        },
        CommandOutcome::Search { query } => {
            let query_lower = query.to_lowercase();
            let matches = transcript
                .iter()
                .enumerate()
                .filter(|(_, line)| line.to_lowercase().contains(&query_lower))
                .map(|(index, line)| format!("search hit {}: {line}", index + 1))
                .take(50)
                .collect::<Vec<_>>();
            let count = matches.len();
            transcript.extend(matches);
            *status = Some(format!("Search `{query}`: {count} match(es)."));
        }
        CommandOutcome::History { session_id } => {
            *pending_history_session = Some(session_id.clone());
            *status = Some(format!("Loading session `{session_id}`…"));
        }
        CommandOutcome::Media(operation) => {
            *pending_media_op = Some(operation);
            *status = Some("Processing image operation…".into());
        }
        CommandOutcome::AuxiliaryQuestion { question } => {
            *pending_auxiliary_question = Some(question);
            *status = Some("Asking the auxiliary model…".into());
        }
        CommandOutcome::ProviderUsage => {
            *pending_provider_usage = true;
            *status = Some("Querying live provider quota…".into());
        }
        CommandOutcome::CodeBlock { index, write } => {
            *pending_code_block = Some((index, write));
            *status = Some("Processing selected code block…".into());
        }

        // === Phase 7 (ADR 0010) — context mutations ===
        CommandOutcome::ClearPlan => {
            plan.cancel();
            transcript.push("plan: cleared (back to NORMAL).".into());
            *status = Some("Plan cleared.".into());
        }
        CommandOutcome::Compact { keep } => {
            let dropped = transcript.len().saturating_sub(keep);
            if keep == 0 {
                transcript.clear();
            } else if transcript.len() > keep {
                let drain_from = transcript.len() - keep;
                transcript.drain(0..drain_from);
            }
            transcript.push(format!("compact: dropped {dropped} older line(s)."));
            *status = Some(format!("Compacted — {keep} recent line(s) kept."));
        }

        // === Phase 7 (ADR 0010) — context views ===
        // Each view inspects the live SessionState and pushes a one-line
        // summary to the transcript (the TUI's "display surface"). The
        // oracle's richer dashboards depend on subsystems Vesper does not
        // surface yet (token counters, live quota, queue state); the TUI
        // surfaces what it actually has.
        CommandOutcome::ContextView(view_kind) => {
            if view_kind == crate::commands::ViewKind::Context {
                *pending_context_report = true;
                *status = Some("Calculating context-window usage...".into());
            } else {
                let line = render_context_view(view_kind, plan, overrides, transcript, controls);
                transcript.push(line);
                *status = None;
            }
        }

        // === Phase 7 (ADR 0010) — workflow prompts ===
        // Stash the constructed prompt on the session; the binary drains it
        // after dispatch and spawns the AgentLoop. The display text lands in
        // the transcript so the driver sees what was sent.
        CommandOutcome::Workflow { display, prompt } => {
            transcript.push(format!("workflow: {display}"));
            *pending_prompt = Some(prompt);
            *status = Some("WORKING... (workflow agent turn)".into());
        }

        // === Phase 8 (ADR 0011) — memory subsystem commands ===
        // Stash the structured op on the session; the binary owns the durable
        // vesper_memory stores and drains this after dispatch (mirroring the
        // pending_prompt pattern). The transcript shows the op was accepted;
        // the binary will push the actual result line after execution.
        CommandOutcome::Memory(op) => {
            let name = op.command_name();
            transcript.push(format!(
                "memory: /{name} accepted (executing against the durable store)"
            ));
            *pending_memory_op = Some(op);
            *status = Some(format!("/{name}: reading/writing the memory store..."));
        }

        // === Phase 9 (ADR 0012) — checkpoints subsystem commands ===
        // Same drain pattern as Memory ops. The binary owns the
        // vesper_checkpoints stores and executes the op synchronously after
        // dispatch (local filesystem reads/writes, scoped subprocess for
        // /ci — fast enough not to block the UI).
        CommandOutcome::Checkpoint(op) => {
            let name = op.command_name();
            transcript.push(format!(
                "checkpoint: /{name} accepted (executing against the durable ledger)"
            ));
            *pending_checkpoint_op = Some(op);
            *status = Some(format!("/{name}: reading/writing the checkpoint ledger..."));
        }

        // === Phase 10 (ADR 0013) — MCP & plugins subsystem commands ===
        // Same drain pattern. The binary owns the vesper_mcp stores
        // (McpRegistry, McpClient, PluginLoader, TrustedPublishers) and
        // executes the op after dispatch.
        CommandOutcome::Mcp(op) => {
            let name = op.command_name();
            transcript.push(format!(
                "mcp: /{name} accepted (executing against the MCP/plugins subsystem)"
            ));
            *pending_mcp_op = Some(op);
            *status = Some(format!(
                "/{name}: reading/writing the MCP/plugins subsystem..."
            ));
        }

        // === Phase 11 (ADR 0015 — Stage 16) — cognitive-memory commands ===
        CommandOutcome::Cognition(op) => {
            let name = op.command_name();
            transcript.push(format!(
                "cognition: /{name} accepted (executing against the cognitive-memory engine)"
            ));
            *pending_cognition_op = Some(op);
            *status = Some(format!(
                "/{name}: reading/writing the cognitive-memory store..."
            ));
        }

        CommandOutcome::Quit => {}
    }
}

fn glm_plan(value: &str) -> vesper_provider_glm::GlmPlan {
    match value {
        "standard" => vesper_provider_glm::GlmPlan::Standard,
        "bigmodel" => vesper_provider_glm::GlmPlan::BigModel,
        _ => vesper_provider_glm::GlmPlan::Coding,
    }
}

/// Replaces the TODO projection from the bounded markdown emitted by
/// `update_plan`. Malformed lines are ignored rather than displayed as tasks.
pub fn apply_task_plan(state: &mut SessionState, markdown: &str) {
    state.task_plan = markdown
        .lines()
        .filter_map(parse_task_line)
        .take(100)
        .collect();
}

fn parse_task_line(line: &str) -> Option<TaskItem> {
    let trimmed = line.trim();
    let marker_end = trimmed.find(']')?;
    let status = match &trimmed[..=marker_end] {
        "[x]" => "completed",
        "[~]" => "in_progress",
        "[ ]" => "pending",
        _ => return None,
    };
    let rest = trimmed[marker_end + 1..].trim();
    let metadata_start = rest.find('(')?;
    let metadata_end = rest[metadata_start..].find(')')? + metadata_start;
    let metadata = &rest[metadata_start + 1..metadata_end];
    let (_declared_status, priority) = metadata.split_once('/')?;
    let content = rest[metadata_end + 1..].trim();
    if content.is_empty() {
        return None;
    }
    Some(TaskItem {
        content: content.to_string(),
        status: status.into(),
        priority: priority.to_string(),
    })
}

/// Renders one [`ViewKind`] line for the transcript. Pure: reads state only.
fn render_context_view(
    view_kind: crate::commands::ViewKind,
    plan: &PlanState,
    overrides: &SuperpowerOverrides,
    transcript: &[String],
    controls: &SessionControls,
) -> String {
    use crate::commands::ViewKind;
    let phase = plan.phase();
    let line_count = transcript.len();
    let override_count = overrides.len();
    match view_kind {
        ViewKind::Recap => {
            let phase_label = phase_label(phase);
            format!(
                "recap: {phase_label}, {line_count} transcript line(s), {override_count} active override(s)."
            )
        }
        ViewKind::Context => unreachable!("the binary owns live context accounting"),
        ViewKind::Status => format!(
            "status: Phase={phase_label}, transcript={line_count} lines, overrides={override_count}.",
            phase_label = phase_label(phase)
        ),
        ViewKind::Tasks => format!(
            "tasks: dashboard — Phase={phase_label}, {line_count} transcript line(s). \
             (Queue / token / model views need runtime integration.)",
            phase_label = phase_label(phase)
        ),
        ViewKind::MaxIterations => {
            format!(
                "max-iterations: live per-turn tool-call cap is {} (set with `/max-iterations 1-200`).",
                controls.max_tool_iterations
            )
        }
        ViewKind::Usage => "usage: query routed to the active provider quota integration.".into(),
    }
}

/// Lower-cased phase label for view rendering.
fn phase_label(phase: PlanPhase) -> &'static str {
    match phase {
        PlanPhase::Normal => "NORMAL",
        PlanPhase::Planning => "PLANNING",
        PlanPhase::Review => "REVIEW",
        PlanPhase::Executing => "EXECUTING",
    }
}

/// Formats a [`SuperpowerValue`] for transcript/status display.
fn format_superpower_value(value: &SuperpowerValue) -> String {
    match value {
        SuperpowerValue::Choice { value } => value.as_str().to_string(),
        SuperpowerValue::Flag { value } => value.to_string(),
        SuperpowerValue::Number { value } => value.to_string(),
    }
}

#[cfg(test)]
mod integration_tests {
    //! End-to-end Plan Mode lifecycle through the dispatch surface.
    //!
    //! These compose the command registry, the Plan Mode state machine, the
    //! superpower surface, the override store, and the [`StubRenderer`] into a
    //! simulated loop — proving the wiring the binary's event loop relies on
    //! without ever touching a real terminal or crossterm.

    use super::*;
    use crate::commands::CommandIntent;
    use crate::plan_mode::PlanPhase;
    use crate::ui::{StubRenderer, TerminalRenderer, ViewModel};
    use vesper_domain::{BoundedString, ProviderId};
    use vesper_provider::{SuperpowerDescriptor, SuperpowerKind, SuperpowerScope, SuperpowerValue};

    fn provider() -> ProviderId {
        ProviderId::new("zai").unwrap()
    }

    fn reasoning_descriptor() -> SuperpowerDescriptor {
        // Mirrors the reconciled GLM `zai:reasoning` dial (ADR 0009).
        SuperpowerDescriptor {
            id: BoundedString::new("zai:reasoning").unwrap(),
            provider_id: provider(),
            display_name: BoundedString::new("Thinking").unwrap(),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Session,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("enabled").unwrap(),
            },
            allowed_values: ["disabled", "enabled", "high", "max"]
                .iter()
                .copied()
                .map(|raw| SuperpowerValue::Choice {
                    value: BoundedString::new(raw).unwrap(),
                })
                .collect(),
            command_alias: Some(BoundedString::new("thinking").unwrap()),
            help: Some(BoundedString::new("Session reasoning depth.").unwrap()),
        }
    }

    fn model_descriptor() -> SuperpowerDescriptor {
        SuperpowerDescriptor {
            id: BoundedString::new("zai:model").unwrap(),
            provider_id: provider(),
            display_name: BoundedString::new("Model").unwrap(),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Session,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("glm-5.2").unwrap(),
            },
            allowed_values: ["glm-5.2", "glm-5-turbo", "glm-5v-turbo"]
                .into_iter()
                .map(|raw| SuperpowerValue::Choice {
                    value: BoundedString::new(raw).unwrap(),
                })
                .collect(),
            command_alias: Some(BoundedString::new("model").unwrap()),
            help: None,
        }
    }

    fn surface() -> ProviderSuperpowerSurface {
        ProviderSuperpowerSurface::new(provider(), vec![reasoning_descriptor(), model_descriptor()])
    }

    fn registry() -> CommandRegistry {
        CommandRegistry::stage_11b()
    }

    /// Steps a `SessionState` through one input line, mirroring what the
    /// binary's event loop does on `KeyCode::Enter`.
    fn step(
        state: &mut SessionState,
        registry: &CommandRegistry,
        surface: &ProviderSuperpowerSurface,
        raw: &str,
    ) -> DispatchOutcome {
        let intent = CommandIntent::parse(raw);
        dispatch(&intent, registry, surface, &provider(), state)
    }

    #[test]
    fn plan_command_drives_normal_into_planning() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        assert_eq!(state.phase(), PlanPhase::Normal);
        let outcome = step(&mut state, &registry, &surface, "/planmode ship the matrix");
        assert_eq!(outcome, DispatchOutcome::Continue);
        assert_eq!(state.phase(), PlanPhase::Planning);
        assert!(state.plan.prd().is_some());
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("PLANNING"))
        );
    }

    #[test]
    fn plan_then_review_drives_into_review_and_waits() {
        // ADR 0010 Phase 5: /plan … then a model-generated plan (via
        // `update_plan`) lands in REVIEW; the next step must be /approve.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(
            &mut state,
            &registry,
            &surface,
            "/planmode build a REST gateway",
        );
        assert_eq!(state.phase(), PlanPhase::Planning);

        // The agent loop's update_plan surfaces the plan here (Phase 5 bridge).
        let _ = apply_model_plan(&mut state, "1. scaffold\n2. routes\n3. tests");
        assert_eq!(state.phase(), PlanPhase::Review);
        // REVIEW must surface a confirmation prompt in the status line.
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("/approve"))
        );
        // The plan body must be retained so the driver can review it.
        assert!(state.plan.plan().is_some());
    }

    #[test]
    fn full_lifecycle_normal_planning_review_executing_normal() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/planmode PRD");
        assert_eq!(state.phase(), PlanPhase::Planning);
        let _ = apply_model_plan(&mut state, "the plan body");
        assert_eq!(state.phase(), PlanPhase::Review);
        step(&mut state, &registry, &surface, "/approve");
        assert_eq!(state.phase(), PlanPhase::Executing);
        // EXECUTING → NORMAL via complete() (in a real run the runtime drives
        // this; here we exercise the state machine directly to close the loop).
        let transition = state.plan.complete().unwrap();
        assert_eq!(
            transition,
            crate::plan_mode::PlanTransition::Entered(PlanPhase::Normal)
        );
        assert_eq!(state.phase(), PlanPhase::Normal);
    }

    #[test]
    fn cancel_aborts_an_in_flight_plan() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/planmode PRD");
        let _ = apply_model_plan(&mut state, "body");
        assert_eq!(state.phase(), PlanPhase::Review);

        step(&mut state, &registry, &surface, "/cancel");
        assert_eq!(state.phase(), PlanPhase::Normal);
        assert!(state.plan.plan().is_none());
    }

    #[test]
    fn thinking_command_sets_a_pending_reasoning_update() {
        // ADR 0009 / Tier A: `/thinking max` must (a) persist into the local
        // override layer for rendering and (b) produce a pending reasoning
        // update the binary drains into the runtime `UpdateSessionReasoning`
        // command. The runtime-side threading is proven in
        // `vesper-runtime/tests/runtime.rs`.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        assert!(state.overrides.is_empty());
        assert!(state.pending_reasoning.is_none());

        step(&mut state, &registry, &surface, "/thinking max");

        // Local override persisted for the renderer.
        assert_eq!(state.overrides.len(), 1);
        assert!(
            state
                .overrides
                .get("zai:reasoning", None)
                .is_some_and(|value| matches!(
                    value,
                    SuperpowerValue::Choice { ref value } if value.as_str() == "max"
                ))
        );
        // Pending runtime update carries the oracle-faithful mode label.
        assert_eq!(
            state.pending_reasoning.as_ref().map(|mode| mode.as_str()),
            Some("max")
        );
    }

    #[test]
    fn thinking_command_rejects_invented_modes() {
        // ADR 0009: `low`/`medium` are not in the oracle scale. Defense in
        // depth: the command registry rejects them at the descriptor's
        // `allowed_values` gate before the GLM mapper runs, so no pending
        // reasoning update is ever produced. (The mapper's own rejection path
        // is covered in `vesper-provider-glm`.)
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/thinking medium");
        assert!(
            state.pending_reasoning.is_none(),
            "no runtime update for invented modes"
        );
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| !status.is_empty()),
            "an invented mode must surface an error status"
        );

        // The oracle-faithful modes do produce an update.
        step(&mut state, &registry, &surface, "/thinking high");
        assert_eq!(
            state.pending_reasoning.as_ref().map(|mode| mode.as_str()),
            Some("high")
        );
    }

    #[test]
    fn permission_and_mode_commands_govern_pending_runtime_and_agent_state() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/permission bypass");
        assert_eq!(
            state.controls.permission_mode,
            SessionPermissionMode::Bypass
        );
        assert_eq!(
            state.pending_mode_update,
            Some((SessionOperatingMode::Code, SessionPermissionMode::Bypass))
        );

        state.pending_mode_update = None;
        step(&mut state, &registry, &surface, "/mode ask");
        assert_eq!(state.controls.operating_mode, SessionOperatingMode::Plan);
        assert_eq!(
            state.pending_mode_update,
            Some((SessionOperatingMode::Plan, SessionPermissionMode::Bypass))
        );
    }

    #[test]
    fn plan_model_and_thinking_compatibility_matches_the_oracle() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/model glm-5v-turbo");
        assert!(state.status.as_deref().unwrap().contains("unavailable"));

        step(&mut state, &registry, &surface, "/plan standard");
        step(&mut state, &registry, &surface, "/model glm-5.2");
        step(&mut state, &registry, &surface, "/thinking max");
        step(&mut state, &registry, &surface, "/model glm-5-turbo");
        assert!(matches!(
            state.overrides.get("zai:reasoning", None),
            Some(SuperpowerValue::Choice { value }) if value.as_str() == "enabled"
        ));

        step(&mut state, &registry, &surface, "/model glm-5v-turbo");
        step(&mut state, &registry, &surface, "/plan coding");
        assert!(matches!(
            state.overrides.get("zai:model", None),
            Some(SuperpowerValue::Choice { value }) if value.as_str() == "glm-5.2"
        ));
    }

    #[test]
    fn free_text_answers_a_pending_question_during_planning() {
        // Directive 2: the driver can type answers directly to refine the plan.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/planmode PRD");
        // Simulate the model raising one interrogation question.
        state.plan.ask("Which framework?").unwrap();
        assert_eq!(state.plan.pending_questions().len(), 1);

        // The driver answers inline with free text.
        step(&mut state, &registry, &surface, "axum");
        assert!(state.plan.pending_questions().is_empty());
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("Answer recorded"))
        );
    }

    #[test]
    fn free_text_outside_planning_is_a_normal_prompt() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "hello world");
        assert_eq!(state.phase(), PlanPhase::Normal);
        assert_eq!(state.transcript, vec!["user: hello world".to_string()]);
        assert!(state.status.is_none());
    }

    #[test]
    fn quit_short_circuits_dispatch_without_mutating_state() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/planmode PRD");
        let snapshot = state.clone();

        let outcome = step(&mut state, &registry, &surface, "/quit");
        assert_eq!(outcome, DispatchOutcome::Quit);
        // Quit does not touch the plan/override/transcript state.
        assert_eq!(state.plan, snapshot.plan);
        assert_eq!(state.overrides, snapshot.overrides);
        assert_eq!(state.transcript, snapshot.transcript);
    }

    #[test]
    fn stub_renderer_reflects_each_phase_banner() {
        // Directive 3: each phase must render with a clear visual indicator.
        // The StubRenderer records the ViewModel the binary would draw.
        let registry = registry();
        let surface = surface();
        let mut renderer = StubRenderer::new();
        let mut state = SessionState::new();

        let render = |state: &SessionState, renderer: &mut StubRenderer| {
            let model = ViewModel {
                plan: state.plan.clone(),
                superpowers: Some(surface.clone()),
                overrides: state.overrides.clone(),
                transcript: state.transcript.clone(),
                input: String::new(),
                status: state.status.clone(),
                ..ViewModel::default()
            };
            renderer.render(&model);
        };

        render(&state, &mut renderer);
        assert_eq!(
            renderer
                .last_model
                .as_ref()
                .expect("model recorded")
                .plan
                .phase(),
            PlanPhase::Normal
        );

        step(&mut state, &registry, &surface, "/planmode PRD");
        render(&state, &mut renderer);
        assert_eq!(
            renderer.last_model.as_ref().unwrap().plan.phase(),
            PlanPhase::Planning
        );

        let _ = apply_model_plan(&mut state, "body");
        render(&state, &mut renderer);
        assert_eq!(
            renderer.last_model.as_ref().unwrap().plan.phase(),
            PlanPhase::Review
        );

        step(&mut state, &registry, &surface, "/approve");
        render(&state, &mut renderer);
        assert_eq!(
            renderer.last_model.as_ref().unwrap().plan.phase(),
            PlanPhase::Executing
        );
    }

    #[test]
    fn empty_input_surfaces_an_error_in_status() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "   ");
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| !status.is_empty())
        );
    }

    #[test]
    fn version_command_surfaces_the_workspace_version() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/version");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("agent-vesper")),
            "version must surface the agent identity"
        );
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains(&env!("CARGO_PKG_VERSION").to_string()))
        );
    }

    #[test]
    fn clear_view_command_empties_the_transcript() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        // Seed some transcript + a status.
        step(&mut state, &registry, &surface, "hello");
        assert!(!state.transcript.is_empty());

        step(&mut state, &registry, &surface, "/clear-view");
        assert!(state.transcript.is_empty(), "transcript must be cleared");
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("cleared"))
        );
    }

    // ===================================================================
    // Phase 7 (ADR 0010) — full-surface dispatch integration tests.
    // ===================================================================

    #[test]
    fn phase7_clear_plan_resets_plan_mode_to_normal() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        // Enter PLANNING, then clear-plan should drop back to NORMAL.
        step(&mut state, &registry, &surface, "/planmode ship the matrix");
        assert_eq!(state.phase(), PlanPhase::Planning);
        step(&mut state, &registry, &surface, "/clear-plan");
        assert_eq!(state.phase(), PlanPhase::Normal);
        assert!(state.pending_prompt.is_none());
    }

    #[test]
    fn phase7_compact_keeps_only_the_last_n_lines() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        // Seed 5 transcript lines.
        for n in 1..=5 {
            step(&mut state, &registry, &surface, &format!("line {n}"));
        }
        assert_eq!(state.transcript.len(), 5);
        // /compact 2 keeps the last 2 user lines.
        step(&mut state, &registry, &surface, "/compact 2");
        // The compact itself pushes a "dropped N lines" notice, so the final
        // length is 2 (kept) + 1 (notice) = 3.
        assert!(
            state.transcript.len() <= 3,
            "compact must drop older lines; got {}",
            state.transcript.len()
        );
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("compact") && line.contains("dropped")),
            "compact must push a dropped-lines notice"
        );
    }

    #[test]
    fn phase7_context_views_push_a_summary_line() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        // Seed one line so the recap has something to summarize.
        step(&mut state, &registry, &surface, "hello");
        step(&mut state, &registry, &surface, "/recap");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.starts_with("recap:")),
            "/recap must push a recap line"
        );

        step(&mut state, &registry, &surface, "/status");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.starts_with("status:")),
            "/status must push a status line"
        );

        step(&mut state, &registry, &surface, "/max-iterations");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.starts_with("max-iterations:")),
            "/max-iterations must push its line"
        );
    }

    #[test]
    fn phase7_workflow_command_stashes_a_pending_prompt() {
        // The decisive Phase 7 contract for workflow commands: dispatch
        // must stash the constructed prompt on SessionState.pending_prompt
        // so the binary can drain it into a background AgentLoop turn.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        assert!(state.pending_prompt.is_none());
        step(&mut state, &registry, &surface, "/security-review");
        let prompt = state
            .pending_prompt
            .as_ref()
            .expect("/security-review must stash a pending prompt");
        assert!(
            prompt.to_lowercase().contains("security"),
            "the workflow prompt must mention security: {prompt}"
        );
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("workflow") && line.contains("security-review")),
            "/security-review must push a workflow display line"
        );
    }

    #[test]
    fn phase7_smart_pr_stashes_the_pr_prompt() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/smart pr");
        let prompt = state
            .pending_prompt
            .as_ref()
            .expect("/smart pr must stash a pending prompt");
        assert!(
            prompt.contains("gh pr create"),
            "/smart pr must expand to a gh-pr-create prompt: {prompt}"
        );
    }

    #[test]
    fn phase8_memory_command_stashes_a_pending_memory_op() {
        // Phase 8 (ADR 0011): /memory must resolve to a Memory(MemoryOp)
        // outcome that dispatch records on SessionState.pending_memory_op.
        // The binary owns the real store and drains it after dispatch.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/memory");
        let op = state
            .pending_memory_op
            .take()
            .expect("/memory must stash a pending MemoryOp");
        assert_eq!(op.command_name(), "memory");
        // The transcript must show the op was accepted (not a deferred notice).
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("memory:") && line.contains("accepted")),
            "/memory must push an acceptance notice, got: {:?}",
            state.transcript
        );
        // /memory must NOT trigger an agent turn (no pending_prompt).
        assert!(
            state.pending_prompt.is_none(),
            "/memory must not trigger an agent turn"
        );
    }

    #[test]
    fn phase8_goal_command_stashes_goaladd_op() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/goal ship stage 12");
        let op = state
            .pending_memory_op
            .as_ref()
            .expect("/goal must stash a pending MemoryOp");
        match op {
            crate::commands::MemoryOp::GoalAdd { summary } => {
                assert_eq!(summary, "ship stage 12");
            }
            other => panic!("expected GoalAdd, got {other:?}"),
        }
    }

    #[test]
    fn phase8_goal_without_argument_errors_instead_of_deferring() {
        // /goal with no argument must Error (clear usage hint), NOT resolve
        // to Deferred or to a silent no-op. This is the parity contract.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/goal");
        assert!(state.pending_memory_op.is_none());
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("Usage:")),
            "/goal with no arg must show a usage hint"
        );
    }

    #[test]
    fn phase8_all_thirteen_memory_commands_record_pending_ops() {
        // Sanity: every one of the 13 memory commands must produce a
        // pending_memory_op (or a usage Error for goal/subgoal without arg).
        // This is the structural guarantee the lead architect demanded.
        let registry = registry();
        let surface = surface();

        let bare = [
            "/memory",
            "/skills",
            "/profile",
            "/awareness",
            "/metacognition",
            "/deliberation",
            "/repository",
            "/meta-learning",
            "/observability",
            "/curator",
            "/journey",
        ];
        for command in bare {
            let mut state = SessionState::new();
            step(&mut state, &registry, &surface, command);
            assert!(
                state.pending_memory_op.is_some(),
                "{command} must stash a pending_memory_op"
            );
        }
        // goal + subgoal need an argument.
        for command in ["/goal ship", "/subgoal write tests"] {
            let mut state = SessionState::new();
            step(&mut state, &registry, &surface, command);
            assert!(
                state.pending_memory_op.is_some(),
                "{command} must stash a pending_memory_op"
            );
        }
    }

    #[test]
    fn phase9_checkpoint_command_stashes_a_pending_checkpoint_op() {
        // Phase 9 (ADR 0012): /checkpoint must resolve to a
        // Checkpoint(CheckpointOp) outcome that dispatch records on
        // SessionState.pending_checkpoint_op.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/checkpoint");
        let op = state
            .pending_checkpoint_op
            .take()
            .expect("/checkpoint must stash a pending CheckpointOp");
        assert_eq!(op.command_name(), "checkpoint");
        // The transcript must show the op was accepted (not a deferred notice).
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("checkpoint:") && line.contains("accepted")),
            "/checkpoint must push an acceptance notice, got: {:?}",
            state.transcript
        );
        // /checkpoint must NOT trigger an agent turn (no pending_prompt).
        assert!(
            state.pending_prompt.is_none(),
            "/checkpoint must not trigger an agent turn"
        );
    }

    #[test]
    fn phase9_rollback_requires_an_id_and_errors_clearly() {
        // /rollback with no argument must Error (clear usage hint), NOT
        // resolve to Deferred or to a silent no-op.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/rollback");
        assert!(state.pending_checkpoint_op.is_none());
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("Usage:")),
            "/rollback with no arg must show a usage hint"
        );
    }

    #[test]
    fn phase9_undo_with_no_argument_defaults_to_one() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/undo");
        let op = state
            .pending_checkpoint_op
            .as_ref()
            .expect("/undo must stash a pending CheckpointOp");
        let crate::commands::CheckpointOp::CheckpointUndo { count } = op else {
            panic!("expected CheckpointUndo, got {op:?}");
        };
        assert_eq!(*count, 1);
    }

    #[test]
    fn phase9_all_thirteen_checkpoint_commands_record_pending_ops() {
        // Sanity: every one of the 13 checkpoint commands must produce a
        // pending_checkpoint_op (or a usage Error for argument-required ones).
        let registry = registry();
        let surface = surface();

        let bare = [
            "/sessions",
            "/lineage",
            "/checkpoint",
            "/export",
            "/copy",
            "/ci",
        ];
        for command in bare {
            let mut state = SessionState::new();
            step(&mut state, &registry, &surface, command);
            assert!(
                state.pending_checkpoint_op.is_some(),
                "{command} must stash a pending_checkpoint_op"
            );
        }
        // Optional-argument commands resolve without an argument too.
        let optional = ["/sessions-new", "/branch", "/undo"];
        for command in optional {
            let mut state = SessionState::new();
            step(&mut state, &registry, &surface, command);
            assert!(
                state.pending_checkpoint_op.is_some(),
                "{command} must stash a pending_checkpoint_op"
            );
        }
        // Argument-required commands resolve with an argument.
        let arg_required = [
            ("/rollback ckpt-1", "rollback"),
            ("/rewind ckpt-1", "rewind"),
            ("/rename new-name", "rename"),
            ("/loop every 1h run tests", "loop"),
        ];
        for (command, label) in arg_required {
            let mut state = SessionState::new();
            step(&mut state, &registry, &surface, command);
            assert!(
                state.pending_checkpoint_op.is_some(),
                "{command} ({label}) must stash a pending_checkpoint_op"
            );
        }
    }

    #[test]
    fn keybinding_editor_command_routes_to_a_real_pending_operation() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/keybinds");
        assert!(state.pending_keybind_editor);
        assert!(state.pending_prompt.is_none());
        assert!(
            state.pending_prompt.is_none(),
            "/mobile must not trigger an agent turn"
        );
    }

    #[test]
    fn phase7_free_text_still_works_alongside_workflow_commands() {
        // Regression: free-text prompts must still flow through dispatch and
        // land in the transcript. The Phase 7 workflow path is additive.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "hello agent");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("user: hello agent")),
            "free text must still hit the transcript"
        );
        // Free text does not stash a workflow prompt (the binary treats it
        // as a free-text prompt directly).
        assert!(state.pending_prompt.is_none());
    }
}
