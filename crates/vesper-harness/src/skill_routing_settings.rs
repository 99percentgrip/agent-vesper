//! Workspace-scoped routing choices. Reads and drafts never write skill sources.
use std::{
    io::Read,
    path::{Path, PathBuf},
};
pub use vesper_memory::routing_quality::{
    RoutingMode, RoutingOptions, RoutingPreferences, RoutingTask,
};
use vesper_memory::{SkillRoutingQuery, SkillRoutingReport, SkillStore};

pub fn path(root: &Path) -> PathBuf {
    root.join(".agent-vesper/skill-routing.json")
}
fn location(root: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute() || !root.is_dir() {
        return Err("Skills settings require an existing workspace".into());
    }
    for item in [root.join(".agent-vesper"), path(root)] {
        match item.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("Skills settings symlinks are refused".into());
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err("Cannot inspect Skills settings".into());
            }
            _ => {}
        }
    }
    Ok(path(root))
}
pub fn load(root: &Path) -> Result<RoutingPreferences, String> {
    if !root.is_absolute() || !root.exists() {
        return Ok(RoutingPreferences::default());
    }
    let file = match std::fs::File::open(location(root)?) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RoutingPreferences::default());
        }
        Err(_) => return Err("Cannot read Skills settings".into()),
    };
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read Skills settings")?;
    if bytes.len() > 65536 {
        return Err("Skills settings exceed 64 KiB".into());
    }
    let preferences: RoutingPreferences =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid Skills settings")?;
    preferences.validate()?;
    Ok(preferences)
}
pub fn save(root: &Path, preferences: &RoutingPreferences) -> Result<(), String> {
    preferences.validate()?;
    let target = location(root)?;
    let parent = target.parent().ok_or("Invalid Skills settings path")?;
    std::fs::create_dir_all(parent).map_err(|_| "Cannot create Skills settings directory")?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Cannot stage Skills settings")?;
    serde_json::to_writer(&mut file, preferences).map_err(|_| "Cannot encode Skills settings")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Cannot sync Skills settings")?;
    file.persist(target)
        .map_err(|_| "Cannot save Skills settings")?;
    Ok(())
}

/// Both native hosts and all execution paths call this same bridge.
pub fn route(
    root: &Path,
    store: &SkillStore,
    query: &SkillRoutingQuery<'_>,
    task: RoutingTask,
) -> SkillRoutingReport {
    match load(root) {
        Ok(preferences) => {
            store.orchestrate_with_options(query, &RoutingOptions { preferences, task })
        }
        Err(_) => {
            // Do not ignore a potentially disabled skill in corrupt preferences.
            let mut report = SkillRoutingReport::default();
            report.routing_trace.outcome = vesper_memory::routing_quality::RoutingOutcome::Fallback;
            report.routing_trace.reason = "Skills settings unreadable; skill activation withheld. Repair in Settings → Skills.".into();
            if store.has_explicit_request(query.prompt, query.explicit_skill) {
                report.explicit_error = Some(report.routing_trace.reason.clone());
            }
            report
        }
    }
}

/// Explicit text equivalent for ACP and TUI. Mutation requires the `save` verb.
pub fn command(root: &Path, argument: &str) -> Result<String, String> {
    let mut settings = load(root)?;
    use vesper_domain::slash_commands::{SkillRoutingControl, parse_skill_routing_control};
    match parse_skill_routing_control(argument)? {
        SkillRoutingControl::Status => {
            return Ok(format!(
                "Skill routing: {:?}. Disabled: {}. Enhanced is a preview; Standard remains the default.\n/skills settings save mode standard|enhanced\n/skills settings save enable|disable <skill>",
                settings.mode,
                settings
                    .disabled
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        SkillRoutingControl::SaveMode { enhanced } => {
            settings.mode = if enhanced {
                RoutingMode::Enhanced
            } else {
                RoutingMode::Standard
            }
        }
        SkillRoutingControl::SaveEnabled { slug, enabled } => {
            vesper_memory::SkillSlug::new(&slug).map_err(|_| "Invalid skill name")?;
            if enabled {
                settings.disabled.remove(&slug);
            } else {
                settings.disabled.insert(slug);
            }
        }
    }
    save(root, &settings)?;
    Ok("Skills settings saved for the next turn. Skill files are preserved.".into())
}

/// One initial lookup and at most one refinement after validated task information
/// arrives. It stores no prompt/body and cannot execute or replay tools.
#[derive(Debug, Default)]
pub struct RoutingTransition {
    searches: u8,
}
impl RoutingTransition {
    pub fn search(
        &mut self,
        root: &Path,
        store: &SkillStore,
        query: &SkillRoutingQuery<'_>,
        task: RoutingTask,
        has_new_task_information: bool,
    ) -> Result<SkillRoutingReport, &'static str> {
        if self.searches >= 2 || (self.searches == 1 && !has_new_task_information) {
            return Err("routing refinement requires new task information and is limited to one");
        }
        self.searches += 1;
        Ok(route(root, store, query, task))
    }
}

/// Execution choices restrict relevance; they never create execution authority.
pub fn task_for_controls(
    mode: vesper_domain::SessionOperatingMode,
    permission: vesper_domain::SessionPermissionMode,
) -> RoutingTask {
    RoutingTask {
        maximum_effect: (mode == vesper_domain::SessionOperatingMode::Plan
            || permission == vesper_domain::SessionPermissionMode::ReadOnly)
            .then_some(vesper_memory::routing_quality::RoutingEffect::ReadOnly),
        ..RoutingTask::default()
    }
}
