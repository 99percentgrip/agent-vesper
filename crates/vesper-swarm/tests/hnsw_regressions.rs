//! Desired-behavior snapshot corruption regressions.
use vesper_swarm::ledger::hnsw::{HnswConfig, HnswIndex};

fn snapshot() -> (HnswConfig, Vec<u8>) {
    let config = HnswConfig::new(2);
    let mut index = HnswIndex::new(config.clone()).unwrap();
    index.add_point(7, &[1.0, 0.0]).unwrap();
    (config, index.to_snapshot())
}

#[test]
fn every_truncation_and_seeded_corruption_is_bounded_and_reloadable_or_refused() {
    for seed in 1..=8 {
        let config = HnswConfig::new(3).with_seed(seed);
        let mut index = HnswIndex::new(config.clone()).unwrap();
        for id in 0..16 {
            index.add_point(id, &[id as f32 + 1.0, -2.0, 3.0]).unwrap();
        }
        let bytes = index.to_snapshot();
        for end in 0..bytes.len() {
            assert!(HnswIndex::from_snapshot(&config, &bytes[..end]).is_err());
        }
        // Mutations may describe another valid graph: acceptance requires stable
        // serialization, finite queries and deterministic continued insertion.
        for offset in (0..bytes.len()).step_by(7) {
            let mut changed = bytes.clone();
            changed[offset] ^= 0xff;
            if let Ok(mut loaded) = HnswIndex::from_snapshot(&config, &changed) {
                let canonical = loaded.to_snapshot();
                let mut copy = HnswIndex::from_snapshot(&config, &canonical).unwrap();
                for hit in loaded.search(&[1.0, 2.0, 3.0], 8, 32) {
                    assert!(hit.similarity.is_finite());
                }
                loaded.add_point(1000, &[3.0, 2.0, 1.0]).unwrap();
                copy.add_point(1000, &[3.0, 2.0, 1.0]).unwrap();
                assert_eq!(loaded.to_snapshot(), copy.to_snapshot());
            }
        }
    }
}

#[test]
fn header_level_must_match_the_graph() {
    let (config, mut bytes) = snapshot();
    bytes[44..48].copy_from_slice(&15u32.to_le_bytes());
    assert!(HnswIndex::from_snapshot(&config, &bytes).is_err());
}

#[test]
fn caller_capacity_and_seed_are_enforced() {
    let (mut config, bytes) = snapshot();
    config.max_elements = 1;
    assert!(HnswIndex::from_snapshot(&config, &bytes).is_err());
    let (mut config, bytes) = snapshot();
    config.seed ^= 1;
    assert!(HnswIndex::from_snapshot(&config, &bytes).is_err());
}

#[test]
fn nonfinite_snapshot_vector_is_refused_at_load() {
    let (config, mut bytes) = snapshot();
    let offset = bytes.len() - 4;
    bytes[offset..].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(HnswIndex::from_snapshot(&config, &bytes).is_err());
}

#[test]
fn count_must_fit_input_before_allocation() {
    let (config, mut bytes) = snapshot();
    bytes[36..40].copy_from_slice(&1_000_000u32.to_le_bytes());
    assert!(HnswIndex::from_snapshot(&config, &bytes).is_err());
}

#[test]
fn continued_insertion_is_byte_identical_after_reload() {
    let config = HnswConfig::new(2);
    let mut direct = HnswIndex::new(config.clone()).unwrap();
    for id in 0..20 {
        direct.add_point(id, &[id as f32 + 1.0, 3.0]).unwrap();
    }
    let mut reloaded = HnswIndex::from_snapshot(&config, &direct.to_snapshot()).unwrap();
    for id in 20..40 {
        let vector = [id as f32 + 1.0, 3.0];
        direct.add_point(id, &vector).unwrap();
        reloaded.add_point(id, &vector).unwrap();
    }
    assert_eq!(direct.to_snapshot(), reloaded.to_snapshot());
}

#[test]
fn semantic_configuration_is_not_silently_replaced() {
    let (mut config, bytes) = snapshot();
    config.over_fetch_factor += 1;
    assert!(HnswIndex::from_snapshot(&config, &bytes).is_err());
}

#[test]
fn finite_extreme_vectors_remain_normalized_and_nonfinite_is_rejected() {
    let mut index = HnswIndex::new(HnswConfig::new(2)).unwrap();
    index.add_point(1, &[f32::MAX, f32::MAX]).unwrap();
    assert!((index.search(&[1.0, 1.0], 1, 10)[0].similarity - 1.0).abs() < 1e-5);
    index.add_point(2, &[1e-30, 0.0]).unwrap();
    assert_eq!(index.search(&[1.0, 0.0], 1, 10)[0].id, 2);
    assert!(index.add_point(3, &[f32::NAN, 0.0]).is_err());
}

#[test]
fn mixed_zero_seed_and_raw_values_round_trip() {
    let config = HnswConfig::new(2).with_seed(0x9e37_79b9_7f4a_7c15);
    let mut index = HnswIndex::new(config.clone()).unwrap();
    index.add_point(1, &[17.5, -0.0]).unwrap();
    let bytes = index.to_snapshot();
    assert_eq!(
        &bytes[bytes.len() - 8..bytes.len() - 4],
        &17.5f32.to_le_bytes()
    );
    assert_eq!(&bytes[bytes.len() - 4..], &(-0.0f32).to_le_bytes());
    assert_eq!(
        HnswIndex::from_snapshot(&config, &bytes)
            .unwrap()
            .to_snapshot(),
        bytes
    );
}

#[test]
fn bounded_snapshot_checks_exact_size_and_preserves_format() {
    let mut index = HnswIndex::new(HnswConfig::new(2)).unwrap();
    for id in 1..32 {
        index.add_point(id, &[id as f32, 1.0]).unwrap();
        let bytes = index.to_snapshot();
        assert!(index.to_snapshot_bounded(bytes.len() - 1).is_err());
        assert_eq!(index.to_snapshot_bounded(bytes.len()).unwrap(), bytes);
    }
    let empty = HnswIndex::new(HnswConfig::new(2)).unwrap();
    assert!(empty.to_snapshot_bounded(59).is_err());
    assert_eq!(empty.to_snapshot_bounded(60).unwrap(), empty.to_snapshot());
}

#[test]
fn cloned_graph_mutations_preserve_original_adjacency_vectors_and_rng() {
    let config = HnswConfig::new(2);
    let mut original = HnswIndex::new(config.clone()).unwrap();
    for id in 0..128 {
        original.add_point(id, &[id as f32, 1.0]).unwrap();
    }
    let before = original.to_snapshot();
    let mut cloned = original.clone();
    for id in 128..256 {
        cloned.add_point(id, &[id as f32, 1.0]).unwrap();
    }
    assert_eq!(original.to_snapshot(), before);
    let mut restored = HnswIndex::from_snapshot(&config, &before).unwrap();
    for id in 128..256 {
        restored.add_point(id, &[id as f32, 1.0]).unwrap();
    }
    assert_eq!(cloned.to_snapshot(), restored.to_snapshot());
}

#[test]
fn portable_little_endian_fixture_continues_identically_on_every_target() {
    fn decode(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    // Independently encoded v2 wire records: seed=1, raw [1,0], then [0,2].
    // These constants are shared by all five target runners, not generated by
    // each platform's implementation during its test.
    let first = decode(
        "565357484e535731020000000200000010000000c800000040420f0001000000000000000100000000000000020000000a000000db3ecbcb9e53d744070000000000000002000000030000000000000000000000000000000000803f00000000",
    );
    let second = decode(
        "565357484e535731020000000200000010000000c800000040420f0001000000000000000200000000000000020000000a00000089de13847e51888307000000000000000200000003000000010000000100000001000000010000000000000008000000000000000100000002000000010000000000000001000000000000000000803f000000000000000000000040",
    );
    let config = HnswConfig::new(2).with_seed(1);
    let mut index = HnswIndex::from_snapshot(&config, &first).unwrap();
    assert_eq!(index.search(&[1.0, 0.0], 1, 16)[0].id, 7);
    assert_eq!(index.to_snapshot(), first);
    index.add_point(8, &[0.0, 2.0]).unwrap();
    assert_eq!(index.to_snapshot(), second);
    assert_eq!(index.search(&[0.0, 1.0], 1, 16)[0].id, 8);
}
