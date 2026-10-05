use std::process::Command;

/// Pins xAI's native credential store to the existing signed-out fixture.
///
/// ACP composition constructs the native xAI adapter at startup even when a
/// fixture does not select xAI. HOME/XDG isolation alone is insufficient for
/// tests that intentionally clear those variables, so every spawned ACP
/// process must receive this explicit store path.
#[allow(dead_code)]
pub fn set_signed_out_xai_credentials(command: &mut Command, fixture: &tempfile::NamedTempFile) {
    command.env("AGENT_VESPER_XAI_CREDENTIALS_PATH", fixture.path());
}
