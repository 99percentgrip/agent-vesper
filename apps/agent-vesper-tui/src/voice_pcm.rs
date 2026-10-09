//! Bounded streaming conversion. Source frames are averaged into target time bins.
//! Conversion preserves time across callback boundaries and never relabels sample rates.

/// Rate conversion with constant memory; input is mono normalized PCM.
pub struct Resampler {
    source_rate: u32,
    target_rate: u32,
    remaining: u32,
    sum: f64,
}
impl Resampler {
    /// Both rates must be nonzero.
    pub fn new(source_rate: u32, target_rate: u32) -> Result<Self, &'static str> {
        if source_rate == 0 || target_rate == 0 {
            return Err("audio sample rate is zero");
        }
        Ok(Self {
            source_rate,
            target_rate,
            remaining: source_rate,
            sum: 0.0,
        })
    }
    /// Consume one source sample and emit zero or more target samples.
    pub fn sample(&mut self, value: f32, mut emit: impl FnMut(f32)) {
        let value = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let mut ticks = self.target_rate;
        while ticks != 0 {
            let used = ticks.min(self.remaining);
            self.sum += f64::from(value) * f64::from(used);
            self.remaining -= used;
            ticks -= used;
            if self.remaining == 0 {
                emit((self.sum / f64::from(self.source_rate)) as f32);
                self.sum = 0.0;
                self.remaining = self.source_rate;
            }
        }
    }
}

/// Canonical signed little-endian PCM sample, finite and saturating.
pub fn s16(value: f32) -> [u8; 2] {
    let value = if value.is_finite() { value } else { 0.0 };
    ((value * 32768.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rates_preserve_duration_and_signal_across_chunks() {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
            let mut resampler = Resampler::new(rate, 16_000).unwrap();
            let mut count = 0;
            for _ in 0..rate {
                resampler.sample(0.25, |v| {
                    assert!((v - 0.25).abs() < 0.0001);
                    count += 1;
                });
            }
            assert_eq!(count, 16_000);
        }
    }
    #[test]
    fn downsampling_averages_source_frames() {
        let mut r = Resampler::new(48_000, 16_000).unwrap();
        let mut values = vec![];
        for v in [1.0, -1.0, 0.0] {
            r.sample(v, |v| values.push(v));
        }
        assert_eq!(values, [0.0]);
    }
    #[test]
    fn canonical_samples_are_finite_saturated_and_little_endian() {
        assert_eq!(s16(0.25), 8192i16.to_le_bytes());
        assert_eq!(s16(-2.0), i16::MIN.to_le_bytes());
        assert_eq!(s16(2.0), i16::MAX.to_le_bytes());
        assert_eq!(s16(f32::NAN), [0, 0]);
    }
}
