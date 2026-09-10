//! Desired-behavior snapshot corruption regressions.
use vesper_swarm::ledger::hnsw::{HnswConfig, HnswIndex};

fn snapshot() -> (HnswConfig, Vec<u8>) {
    let config = HnswConfig::new(2);
    let mut index = HnswIndex::new(config.clone()).unwrap();
    index.add_point(7, &[1.0, 0.0]).unwrap();
    (config, index.to_snapshot())
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
