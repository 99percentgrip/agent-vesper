//! Non-production audit evidence for fb14ea2 / f662519.
//! These tests assert the observed BUGS, not desired behavior; passing confirms gaps.
//! Not a workspace target. Build instructions and limitations: vro15-gap-audit.md.
use std::{sync::Arc, time::Duration};
use vesper_security::{
    CapabilityStatus::Available, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};
use vesper_swarm::{
    bus::*,
    ledger::hnsw::*,
    manager::*,
    pool::{PoolConfig, WorkerPool},
    sandbox::*,
    topology::*,
    worker::*,
};
struct Worker;
impl WorkerPort for Worker {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        t: &'a WorkerTask,
        _: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            Ok(TurnReceipt {
                task_id: t.id.clone(),
                output: "ok".into(),
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}
fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn pool(min: u32, max: u32) -> WorkerPool {
    WorkerPool::new(
        PoolConfig {
            min_workers: min,
            max_workers: max,
            ..Default::default()
        },
        Arc::new(Worker),
    )
    .unwrap()
}
#[test]
fn repeated_initialize_exceeds_max() {
    rt().block_on(async {
        let p = pool(2, 2);
        p.initialize().await.unwrap();
        p.initialize().await.unwrap();
        assert_eq!(p.live_workers(), 4);
    });
}
#[test]
fn replacement_at_capacity_stalls() {
    rt().block_on(async {
        let p = pool(1, 1);
        p.initialize().await.unwrap();
        p.test_freeze_last_seen(Duration::from_secs(3));
        p.health_tick(tokio::time::Instant::now());
        assert_eq!(p.replace_failed().await.unwrap(), 0);
        assert!(p.acquire(&[]).await.is_err());
    });
}
#[test]
fn acquire_creates_incapable_worker() {
    rt().block_on(async {
        let p = pool(1, 2);
        p.initialize().await.unwrap();
        let (_, caps) = p.acquire(&["definitely-missing".into()]).await.unwrap();
        assert!(!caps.supports(&["definitely-missing".into()]));
    });
}
#[test]
fn timeout_does_not_signal_worker() {
    rt().block_on(async {
        let flag = CancelFlag::new();
        let signal = flag.signal();
        let task = WorkerTask::new("t", "work");
        let (result, _) = vesper_swarm::hive::timeout::execute_bounded(
            &task,
            async move {
                while !signal.is_cancelled() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                Err(WorkerError::Cancelled("t".into()))
            },
            Duration::from_millis(2),
            Duration::from_millis(10),
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(result, Err(WorkerError::DeadlineExceeded(_))));
        assert!(!flag.signal().is_cancelled());
    });
}
#[test]
fn closed_bus_receiver_never_finishes() {
    rt().block_on(async {
        let b = MessageBus::new(1).unwrap();
        b.subscribe("w", &[]).unwrap();
        b.close();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), b.recv("w"))
                .await
                .is_err()
        );
    });
}
#[test]
fn broadcast_partial_on_error() {
    let b = MessageBus::new(1).unwrap();
    b.subscribe("a", &[]).unwrap();
    b.subscribe("b", &[]).unwrap();
    assert!(
        b.broadcast(OutgoingBroadcast::new("s").priority(MessagePriority::Urgent))
            .is_err()
    );
    assert_eq!(b.queued(), 1);
}
#[test]
fn expired_ack_leaks() {
    let b = MessageBus::new(1).unwrap();
    b.subscribe("w", &[]).unwrap();
    b.send(
        OutgoingMessage::new("s", "w")
            .ttl(Duration::ZERO)
            .requires_ack(),
    )
    .unwrap();
    assert!(b.try_recv("w").unwrap().is_none());
    assert_eq!(b.pending_ack_count(), 1);
    b.unsubscribe("w").unwrap();
    assert_eq!(b.pending_ack_count(), 1);
}
struct Backend {
    fail_release: bool,
}
impl SandboxLeasePort for Backend {
    fn acquire(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        Ok(())
    }
    fn release(&self, s: &LeaseSpec) -> Result<(), LeaseError> {
        if self.fail_release {
            Err(LeaseError::PortRefused {
                worker: s.worker_id.clone(),
                reason: "still alive".into(),
            })
        } else {
            Ok(())
        }
    }
}
fn book(fail: bool) -> LeaseBook {
    LeaseBook::new(
        1,
        IsolationRequirement::Full,
        SandboxCapabilities {
            backend: "audit".into(),
            process_tree: Available,
            filesystem: Available,
            network: Available,
            strength: SecurityStrength::Full,
        },
        Arc::new(Backend { fail_release: fail }),
    )
    .unwrap()
}
#[test]
fn shared_allows_pairwise_collision() {
    rt().block_on(async {
        let b = book(false);
        let spec = |w: &str| LeaseSpec::shared(w, IsolationRequirement::Full, "grant", "g");
        let _a = b.acquire(spec("a")).await.unwrap();
        let _b = b.acquire(spec("b")).await.unwrap();
        let _c = b.acquire(spec("b")).await.unwrap();
        assert_eq!(b.active_count(), 1);
    });
}
#[test]
fn shared_accepts_nested_paths() {
    let a = LeaseSpec::shared("a", IsolationRequirement::Full, "grant", "g");
    let mut b = LeaseSpec::shared("b", IsolationRequirement::Full, "grant", "g");
    b.write_path = "/w/a/nested/".into();
    assert!(a.can_share(&b));
}
#[test]
fn failed_release_reopens_capacity() {
    rt().block_on(async {
        let b = book(true);
        drop(
            b.acquire(LeaseSpec::isolated("a", IsolationRequirement::Full))
                .await
                .unwrap(),
        );
        assert_eq!(b.active_count(), 0);
        assert_eq!(b.releases(), 1);
        assert_eq!(b.release_errors().len(), 1);
        let _new = b
            .acquire(LeaseSpec::isolated("b", IsolationRequirement::Full))
            .await
            .unwrap();
        assert_eq!(b.acquisitions(), 2);
    });
}
#[test]
fn hnsw_own_level16_snapshot_rejected() {
    let c = HnswConfig::new(2).with_seed(0x9e3779b97f4a7c15);
    let mut i = HnswIndex::new(c.clone()).unwrap();
    i.add_point(1, &[1., 0.]).unwrap();
    assert!(HnswIndex::from_snapshot(&c, &i.to_snapshot()).is_err());
}
#[test]
fn hnsw_rng_not_restored() {
    let c = HnswConfig::new(2);
    let mut a = HnswIndex::new(c.clone()).unwrap();
    for n in 0..30 {
        a.add_point(n, &[1., n as f32]).unwrap();
    }
    let mut b = HnswIndex::from_snapshot(&c, &a.to_snapshot()).unwrap();
    for n in 30..40 {
        a.add_point(n, &[1., n as f32]).unwrap();
        b.add_point(n, &[1., n as f32]).unwrap();
    }
    assert_ne!(a.to_snapshot(), b.to_snapshot());
}
#[test]
fn hnsw_accepts_invalid_level_header_then_panics() {
    let c = HnswConfig::new(2);
    let mut i = HnswIndex::new(c.clone()).unwrap();
    i.add_point(1, &[1., 0.]).unwrap();
    let mut bytes = i.to_snapshot();
    bytes[44..48].copy_from_slice(&15u32.to_le_bytes());
    let loaded = HnswIndex::from_snapshot(&c, &bytes).unwrap();
    assert!(std::panic::catch_unwind(|| loaded.search(&[1., 0.], 1, 16)).is_err());
}
#[test]
fn nonfinite_vector_accepted() {
    let mut i = HnswIndex::new(HnswConfig::new(2)).unwrap();
    i.add_point(1, &[f32::NAN, 0.]).unwrap();
    assert!(!i.search(&[1., 0.], 1, 16)[0].similarity.is_finite());
}
#[test]
fn failover_disabled_is_bypassed() {
    let m = TopologyManager::new(
        TopologyKind::Mesh,
        TopologyConfig {
            auto_rebalance: true,
            failover_enabled: false,
            ..Default::default()
        },
    )
    .unwrap();
    let mut s = m.initial_state();
    for name in ["a", "b"] {
        let id = NodeId::new(name);
        m.add_node(&mut s, id.clone(), TopologyRole::Worker)
            .unwrap();
        m.update_node(
            &mut s,
            &id,
            NodeUpdate {
                status: Some(NodeStatus::Active),
                ..Default::default()
            },
        )
        .unwrap();
    }
    m.remove_node(&mut s, &NodeId::new("a")).unwrap();
    assert_eq!(s.leader, Some(NodeId::new("b")));
}
#[test]
fn failover_promotes_failed_worker() {
    let m = TopologyManager::new(
        TopologyKind::Mesh,
        TopologyConfig {
            failover_enabled: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut s = m.initial_state();
    for name in ["a", "b", "c"] {
        let id = NodeId::new(name);
        m.add_node(&mut s, id.clone(), TopologyRole::Worker)
            .unwrap();
        m.update_node(
            &mut s,
            &id,
            NodeUpdate {
                status: Some(if name == "b" {
                    NodeStatus::Failed
                } else {
                    NodeStatus::Active
                }),
                ..Default::default()
            },
        )
        .unwrap();
    }
    m.elect_leader(&mut s).unwrap();
    m.remove_node(&mut s, &NodeId::new("a")).unwrap();
    assert_eq!(s.leader, Some(NodeId::new("b")));
}
