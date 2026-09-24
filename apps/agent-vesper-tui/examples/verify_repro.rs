// Standalone reproduction of the Verify path (same code as settings_host).
fn main() {
    use std::sync::Arc;
    use vesper_voice::audio::PcmFrame;
    use vesper_voice::cancel::VoiceCancel;
    use vesper_voice::composition::blocking::ThreadPoolExecutor;
    use vesper_voice::ports::VoiceStt;
    let pool = Arc::new(ThreadPoolExecutor::new(1));
    let erased: Arc<dyn vesper_voice::composition::blocking::ValueExecutor> = pool;
    let adapter = agent_vesper_tui::voice_flm::FlmNpuStt::new(erased);
    let segments = [
        (0.25_f64, 0.60_f64, 190.0_f64),
        (0.95, 0.70, 240.0),
        (2.10, 0.55, 210.0),
    ];
    let total = (4.0 * 16_000.0) as usize;
    let mut pcm: Vec<u8> = Vec::with_capacity(total * 2);
    for index in 0..total {
        let t = index as f64 / 16_000.0;
        let mut sample: f64 = 0.0;
        for &(start, duration, f0) in &segments {
            if (start..start + duration).contains(&t) {
                let local = t - start;
                let envelope = (std::f64::consts::PI * local / duration).sin().max(0.0);
                sample = envelope
                    * ((2.0 * std::f64::consts::PI * f0 * local).sin()
                        + 0.5 * (2.0 * std::f64::consts::PI * 2.0 * f0 * local).sin()
                        + 0.25 * (2.0 * std::f64::consts::PI * 3.0 * f0 * local).sin())
                    / 1.75;
            }
        }
        let quantized = (sample * 22_000.0).clamp(-32_768.0, 32_767.0) as i16;
        pcm.extend_from_slice(&quantized.to_le_bytes());
    }
    let audio: Vec<PcmFrame> = pcm
        .chunks(2)
        .map(|c| PcmFrame::from_aligned(c.to_vec()).unwrap())
        .collect();
    let cancel = VoiceCancel::default();
    let start = std::time::Instant::now();
    let future = adapter.transcribe(&audio, &cancel);
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let outcome = loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(r) => break r,
            std::task::Poll::Pending => {
                if std::time::Instant::now() > deadline {
                    println!("HUNG 180s");
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    };
    println!(
        "outcome in {:.1}s: {:?}",
        start.elapsed().as_secs_f64(),
        outcome.as_ref().map(|t| t.text.as_str().to_owned())
    );
    adapter.shutdown();
}
