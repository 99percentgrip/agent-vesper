//! Device-free playback diagnostics: no microphone, speaker or provider access.
#![cfg(all(unix, feature = "voice-conversation"))]

use agent_vesper_tui::voice_playback::PlaybackOwner;
use std::os::unix::fs::PermissionsExt;

#[test]
fn device_failure_is_actionable_without_exposing_raw_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let player = dir.path().join("player");
    std::fs::write(
        &player,
        "#!/bin/sh\ncat >/dev/null\nprintf 'private-canary: audio open error: Connection refused\\n' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&player, std::fs::Permissions::from_mode(0o755)).unwrap();
    let owner = PlaybackOwner::new(player, None);
    owner.begin_stream().unwrap();
    owner.push_pcm(&[0; 320]).unwrap();
    let error = owner.end_stream().unwrap_err().to_string();
    assert!(
        error.contains("audio service connection refused"),
        "{error}"
    );
    assert!(
        !error.contains("private-canary"),
        "raw stderr leaked: {error}"
    );
}

#[test]
fn noisy_stderr_cannot_block_player_drain() {
    let dir = tempfile::tempdir().unwrap();
    let player = dir.path().join("player");
    std::fs::write(
        &player,
        "#!/bin/sh\nhead -c 1048576 /dev/zero >&2\ncat >/dev/null\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&player, std::fs::Permissions::from_mode(0o755)).unwrap();
    let owner = PlaybackOwner::new(player, None);
    owner.begin_stream().unwrap();
    owner.push_pcm(&[0; 320]).unwrap();
    let started = std::time::Instant::now();
    let error = owner.end_stream().unwrap_err().to_string();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert!(error.contains("player exited"), "{error}");
}
