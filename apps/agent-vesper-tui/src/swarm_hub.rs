//! Native Swarm Save/Cancel editor over the shared workspace draft.
use ratatui::Frame;
use vesper_harness::swarm_settings::{GovernanceSetting, SwarmSettingsDraft};

pub struct SwarmHub {
    pub draft: SwarmSettingsDraft,
    pub selected: usize,
    pub notice: String,
}
impl SwarmHub {
    pub fn new(draft: SwarmSettingsDraft) -> Self {
        Self {
            draft,
            selected: 0,
            notice: "Every run checks embeddings, isolation and tool permissions.".into(),
        }
    }
    pub fn change(&mut self) {
        let settings = &mut self.draft.settings;
        match self.selected {
            0 => settings.enabled = !settings.enabled,
            1 => {
                settings.drivers = if settings.drivers == 8 {
                    3
                } else {
                    settings.drivers + 1
                }
            }
            2 => {
                let next = match format!("{:?}", settings.topology).as_str() {
                    "Mesh" => "hierarchical",
                    "Hierarchical" => "centralized",
                    "Centralized" => "hybrid",
                    _ => "mesh",
                };
                self.draft
                    .apply(&format!("topology {next}"))
                    .expect("known topology");
            }
            3 => settings.failover = !settings.failover,
            4 => settings.shared_scope = !settings.shared_scope,
            5 => {
                settings.governance = match settings.governance {
                    GovernanceSetting::Auto => GovernanceSetting::Gated,
                    GovernanceSetting::Gated => GovernanceSetting::Auto,
                };
            }
            _ => {}
        }
    }
}
pub fn render(frame: &mut Frame<'_>, hub: &SwarmHub, theme: &str) {
    let settings = &hub.draft.settings;
    let rows = [
        format!("Swarm: {}", if settings.enabled { "ON" } else { "OFF" }),
        format!("Drivers: {}", settings.drivers),
        format!("Topology: {:?}", settings.topology),
        format!("Failover: {}", if settings.failover { "ON" } else { "OFF" }),
        format!(
            "Scope: {}",
            if settings.shared_scope {
                "shared"
            } else {
                "isolated"
            }
        ),
        format!(
            "Governance: {}",
            match settings.governance {
                GovernanceSetting::Auto => "auto",
                GovernanceSetting::Gated => "gated",
            }
        ),
        "Save settings".into(),
        "Cancel".into(),
    ];
    crate::settings_menu::render_menu(
        frame,
        &rows,
        hub.selected,
        "Settings › Swarm",
        &hub.notice,
        "↑↓ select · Enter/Space change · S save · Esc cancel",
        theme,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_draft_renders_save_cancel_and_does_not_save_toggles() {
        let root = tempfile::tempdir().unwrap();
        let mut hub = SwarmHub::new(SwarmSettingsDraft::open(root.path()).unwrap());
        hub.change();
        assert!(hub.draft.settings.enabled);
        assert!(!root.path().join(".agent-vesper").exists());
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| render(frame, &hub, "chatgpt-black"))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for expected in [
            "Settings › Swarm",
            "Swarm: ON",
            "Save settings",
            "Cancel",
            "Drivers: 3",
        ] {
            assert!(text.contains(expected), "{expected}");
        }
    }
}
