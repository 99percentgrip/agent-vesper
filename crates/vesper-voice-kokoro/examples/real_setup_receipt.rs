//! VRO-17 R3: the real-setup receipt runner (evidence helper, never
//! production code). Drives the production `VoicePackSetup` pipeline
//! into the real managed pack root and prints exact-byte accounting.
//!
//! Usage: `cargo run -p vesper-voice-kokoro --features ort --example
//! real_setup_receipt -- [--probe-only]`

use std::time::Instant;

use vesper_voice_kokoro::pack;
use vesper_voice_kokoro::setup::{StageProgress, VoicePackSetup};

fn main() {
    let probe_only = std::env::args().any(|arg| arg == "--probe-only");
    let root = pack::pack_root();
    println!("managed pack root: {}", root.display());

    let setup = VoicePackSetup::new(root.clone());
    let plan = match setup.plan() {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("PLAN FAILED: {}", error.message());
            std::process::exit(2);
        }
    };
    println!(
        "plan: transfer={} retained={} peak={} available={:?} revision={} runtime={} phonemizer={}",
        plan.transfer_bytes,
        plan.retained_bytes,
        plan.peak_bytes,
        plan.available_bytes,
        plan.revision,
        plan.runtime_version,
        plan.phonemizer_present
    );
    if probe_only {
        return;
    }

    let started = Instant::now();
    let stage_starts: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Instant>>> =
        std::sync::Arc::default();
    let stage_starts_writer = std::sync::Arc::clone(&stage_starts);
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(setup.run(
            |StageProgress {
                 stage,
                 bytes_done,
                 bytes_total,
             }| {
                let key = format!("{stage:?}");
                stage_starts_writer
                    .lock()
                    .expect("stage map")
                    .entry(key.clone())
                    .or_insert_with(Instant::now);
                if bytes_total > 0 {
                    println!("[stage] {:?}: {bytes_done}/{bytes_total} bytes", stage);
                } else {
                    println!("[stage] {:?} started", stage);
                }
            },
            || false,
            None,
        ));
    let elapsed = started.elapsed();
    match outcome {
        Ok(()) => {
            println!("SETUP OK in {:.1}s", elapsed.as_secs_f64());
            for (stage, start) in stage_starts.lock().expect("stage map").iter() {
                println!("[timing] {stage}: {:.2}s", start.elapsed().as_secs_f64());
            }
        }
        Err(error) => {
            eprintln!("SETUP FAILED: {}", error.message());
            std::process::exit(3);
        }
    }

    // Post-install accounting: exact bytes on disk per component.
    let components = [
        ("model", pack::ASSET_MODEL.installed_name),
        ("voice af_heart", pack::ASSET_VOICE_AF_HEART.installed_name),
        (
            "voice am_michael",
            pack::ASSET_VOICE_AM_MICHAEL.installed_name,
        ),
        ("vocab", pack::ASSET_VOCAB.installed_name),
        ("runtime lib", pack::RUNTIME_ASSET.installed_name),
    ];
    let mut total = 0u64;
    for (label, installed) in components {
        let path = root.join(installed);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        total += size;
        println!("[disk] {label}: {size} bytes ({})", path.display());
    }
    println!("[disk] total measured: {total} bytes");
    println!(
        "[disk] manifest retained: {} bytes",
        pack::RETAINED_PACK_BYTES
    );
    match pack::assess(&root) {
        Ok(None) => println!("POST-INSTALL ASSESSMENT: Ready — synthesis verified"),
        Ok(Some(problem)) => println!("POST-INSTALL ASSESSMENT: {}", problem.description()),
        Err(error) => println!("POST-INSTALL ASSESSMENT ERROR: {error}"),
    }
}
