//! Measured-behavior guard tests (2z): the MPRIS adapter must know how
//! real players actually behave — not forward calls blindly.
//!
//! Every guard here encodes a fact measured live in session 2y on
//! Elisa/KDE and recorded in the compatibility manifest.

use std::sync::{Arc, Mutex};

use vesper_bridge::adapter::AdapterPort;
use vesper_bridge::error::BridgeError;
use vesper_bridge::operation::{OperationRequestId, OperationSpec};

use crate::bridge_adapters::{MprisAdapter, MprisBus};

/// Shared scripted fake bus: canned responses keyed by call type so the
/// script can never desync from the adapter's call order (get-property
/// responses vs call acknowledgments are consumed from separate queues).
struct FakeBus {
    properties: Mutex<Vec<String>>,
    call_acks: Mutex<Vec<String>>,
    calls: Mutex<Vec<String>>,
}

impl FakeBus {
    fn new(properties: Vec<&str>) -> Arc<Self> {
        Arc::new(Self {
            properties: Mutex::new(properties.iter().map(|s| s.to_string()).collect()),
            call_acks: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
        })
    }
    fn with_acks(properties: Vec<&str>, acks: usize) -> Arc<Self> {
        Arc::new(Self {
            properties: Mutex::new(properties.iter().map(|s| s.to_string()).collect()),
            call_acks: Mutex::new(vec![String::new(); acks]),
            calls: Mutex::new(Vec::new()),
        })
    }
}

/// Arc-based proxy so the same scripted state serves the adapter.
struct BusProxy(Arc<FakeBus>);

impl MprisBus for BusProxy {
    fn busctl(&self, args: &[&str]) -> Result<String, BridgeError> {
        self.0.calls.lock().unwrap().push(args.join(" "));
        let is_get_property = args.first().is_some_and(|a| *a == "get-property");
        if is_get_property {
            let mut q = self.0.properties.lock().unwrap();
            if q.is_empty() {
                return Ok(String::new());
            }
            return Ok(q.remove(0));
        }
        let mut q = self.0.call_acks.lock().unwrap();
        if q.is_empty() {
            return Ok(String::new());
        }
        Ok(q.remove(0))
    }
}

fn spec(op: &str) -> OperationSpec {
    OperationSpec {
        capability: vesper_bridge::capability::CapabilityId::new(format!("mpris.player.{op}"))
            .unwrap(),
        arguments: serde_json::json!({}),
    }
}

const PAUSED: &str = "s \"Paused\"";
const PLAYING: &str = "s \"Playing\"";
const META_TRACK2: &str =
    "a{sv} 5 \"mpris:trackid\" o \"/org/kde/elisa/playlist/1\" \"xesam:title\" s \"B\"";
const META_TRACK3: &str =
    "a{sv} 5 \"mpris:trackid\" o \"/org/kde/elisa/playlist/2\" \"xesam:title\" s \"C\"";

#[test]
fn previous_while_paused_is_refused_not_blindly_sent() {
    // Measured: Previous-while-paused RESTARTS the track (unwanted
    // mutation). The adapter must refuse with guidance, never send it.
    let bus = FakeBus::new(vec![PAUSED]);
    let adapter = MprisAdapter::with_bus("test.player", Box::new(BusProxy(bus)));
    let out = adapter.dispatch(&OperationRequestId("r1".into()), &spec("previous"));
    let err = out.expect_err("must refuse");
    let msg = err.to_string();
    assert!(
        msg.contains("previous while paused") && msg.contains("restarts"),
        "guidance must explain the measured behavior: {msg}"
    );
}

#[test]
fn rapid_direction_inversion_is_cooled_down() {
    // Measured: rapid Next/Previous inversion CRASHES the player.
    // Sequence: Next (ok) -> immediate Previous must be refused.
    let bus = FakeBus::with_acks(
        vec![
            META_TRACK2, // trackid before next
            META_TRACK3, // trackid after next (hop verified)
            PLAYING,     // status for the previous-while-paused guard
        ],
        1, // one call ack (Next)
    );
    let adapter = MprisAdapter::with_bus("test.player", Box::new(BusProxy(bus)));
    let next = adapter
        .dispatch(&OperationRequestId("r2".into()), &spec("next"))
        .expect("first next is fine");
    assert!(
        next.evidence
            .contains("playlist/1 -> /org/kde/elisa/playlist/2"),
        "hop evidence: {}",
        next.evidence
    );

    let prev = adapter.dispatch(&OperationRequestId("r3".into()), &spec("previous"));
    let err = prev.expect_err("inversion within cooldown must refuse");
    assert!(
        err.to_string().contains("cooldown"),
        "must name the cooldown: {err}"
    );
}

#[test]
fn navigation_settles_only_when_trackid_changed() {
    // §7: a call ack is not a hop. Same trackid after Next = failed
    // navigation, never applied.
    let bus = FakeBus::with_acks(
        vec![
            META_TRACK2, // before
            META_TRACK2, // after — UNCHANGED
        ],
        1, // one call ack (Next)
    );
    let adapter = MprisAdapter::with_bus("test.player", Box::new(BusProxy(bus)));
    let out = adapter
        .dispatch(&OperationRequestId("r4".into()), &spec("next"))
        .expect("dispatch succeeds");
    assert!(
        matches!(
            out.outcome,
            vesper_bridge::operation::OperationOutcome::Failed(_)
        ),
        "same-trackid navigation must fail, got {:?}",
        out.outcome
    );
    assert!(out.summary.contains("trackid unchanged"));
}
