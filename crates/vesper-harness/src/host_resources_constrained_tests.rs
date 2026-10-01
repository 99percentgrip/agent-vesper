//! Deterministic constrained-host acceptance for the RRC Host Resource Governor.
//!
//! These checks deliberately supply observations rather than mutating the
//! developer machine's cgroup, swap state, or target filesystem.

use super::host_resources::{
    DiskCapacity, GateCost, HostCapacity, HostResourceGovernor, ProcessTreeUsage, ResourcePolicy,
};

const GIB: u64 = 1024 * 1024 * 1024;

fn capacity(available: u64, swap_used_percent: u64) -> HostCapacity {
    let swap_total = 8 * GIB;
    HostCapacity {
        host_memory_total_bytes: 16 * GIB,
        memory_available_bytes: available,
        swap_total_bytes: swap_total,
        swap_free_bytes: swap_total.saturating_sub(swap_total * swap_used_percent / 100),
        cgroup_memory_limit_bytes: Some(12 * GIB),
        cgroup_memory_current_bytes: Some(12 * GIB - available),
        logical_cpus: 64,
    }
}

fn safe_disk() -> DiskCapacity {
    DiskCapacity {
        available_bytes: 100 * GIB,
        total_bytes: 500 * GIB,
        target_size_bytes: 11 * GIB,
    }
}

#[test]
fn constrained_memory_acceptance_withholds_expensive_cargo_before_spawn() {
    let temp = tempfile::tempdir().expect("temporary governor root");
    let governor = HostResourceGovernor::new(
        ResourcePolicy::default(),
        temp.path().join("locks"),
        temp.path().join("target"),
    )
    .expect("governor");

    let error = governor
        .preflight_observed_for_test(
            GateCost::Expensive,
            capacity(2 * GIB, 82),
            ProcessTreeUsage::default(),
            safe_disk(),
        )
        .expect_err("critical RAM/swap pressure must refuse a new compiler gate");
    let telemetry = error.telemetry().expect("safety refusal carries telemetry");

    assert_eq!(telemetry.cargo_jobs, 1, "memory must override 64 CPUs");
    assert_eq!(telemetry.pressure.as_label(), "CRITICAL");
    assert_eq!(telemetry.normal_admission_available_bytes, 6 * GIB);
    assert!(
        telemetry
            .action
            .contains("critical memory or swap pressure"),
        "{}",
        telemetry.action
    );
    assert!(
        telemetry.action.contains("6.0 GiB available RAM"),
        "{}",
        telemetry.action
    );
    let rendered = telemetry.render();
    assert!(rendered.contains("Cgroup"), "{rendered}");
    assert!(rendered.contains("Headroom"), "{rendered}");
    assert!(rendered.contains("Swap gate"), "{rendered}");
}

#[test]
fn constrained_disk_acceptance_preserves_target_cache_and_withholds_gate() {
    let temp = tempfile::tempdir().expect("temporary governor root");
    let governor = HostResourceGovernor::new(
        ResourcePolicy::default(),
        temp.path().join("locks"),
        temp.path().join("target"),
    )
    .expect("governor");
    let disk = DiskCapacity {
        available_bytes: 60 * GIB,
        total_bytes: 500 * GIB,
        target_size_bytes: 41 * GIB,
    };

    let error = governor
        .preflight_observed_for_test(
            GateCost::Expensive,
            capacity(10 * GIB, 0),
            ProcessTreeUsage::default(),
            disk,
        )
        .expect_err("insufficient reserve plus estimated growth must refuse the gate");
    let telemetry = error.telemetry().expect("disk refusal carries telemetry");

    assert!(telemetry.action.contains("filesystem headroom"));
    assert_eq!(telemetry.target_size_bytes, 41 * GIB);
    assert!(
        telemetry.disk_available_bytes
            < telemetry.disk_reserve_bytes + telemetry.expected_gate_growth_bytes
    );
}
