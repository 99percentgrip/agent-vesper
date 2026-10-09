//! Hosted disposable-runner acceptance only; never an installed model tool.
use std::path::PathBuf;
use vesper_harness::dependency_setup::{
    VoiceSetupPaths, setup_voice_cancellable, voice_phonemizer,
};

#[tokio::main]
async fn main() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    let runner = PathBuf::from(std::env::var_os("RUNNER_TEMP").expect("hosted temporary root"))
        .canonicalize()
        .expect("existing runner root");
    let state = PathBuf::from(std::env::args_os().nth(1).expect("isolated voice root"))
        .canonicalize()
        .expect("existing private root");
    assert!(state.is_absolute() && state.starts_with(runner));
    let venv = state.join("voice venv");
    let python = venv.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    });
    let paths = VoiceSetupPaths {
        venv,
        python: python.clone(),
        tools: state.join("voice tools"),
        uv: None,
    };
    let result = setup_voice_cancellable(paths, |phase| println!("{phase}"), || false).await;
    println!("{}", result.expect("production voice dependency readiness"));
    println!(
        "NATIVE VOICE TOOLS {}",
        serde_json::json!({"python": python, "espeak": voice_phonemizer().expect("verified phonemizer")})
    );
    println!("NATIVE VOICE DEPENDENCIES VERIFIED");
}
