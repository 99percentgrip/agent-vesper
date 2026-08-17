//! Manual and browser-E2E fixture for the native VesperLens review surface.

use std::path::PathBuf;
use std::time::Duration;

use vesper_agent::planning::{LensQuestion, VesperLens, render_interview_artifact};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argument = std::env::args_os()
        .nth(1)
        .ok_or("usage: vesper_lens_fixture <workspace HTML file|--interview>")?;
    let workspace = std::env::current_dir()?;
    let lens = VesperLens::with_timeout(Duration::from_secs(120));
    let announce = |url: &str| {
        println!("VESPER_LENS_URL={url}");
    };
    let feedback = if argument == "--interview" {
        let html = render_interview_artifact(
            "Release interview",
            &[LensQuestion {
                id: "scope".into(),
                prompt: "Choose a release scope".into(),
                description: "This required answer exercises the interview controls.".into(),
                options: vec!["Patch".into(), "Minor".into()],
                allow_multiple: false,
                required: true,
                recommended: "Patch".into(),
                allow_other: true,
            }],
        );
        lens.review_artifact(&html, announce).await?
    } else {
        let file = PathBuf::from(argument);
        lens.review_file(&file, &workspace, announce).await?
    };
    println!("VESPER_LENS_FEEDBACK={}", serde_json::to_string(&feedback)?);
    Ok(())
}
