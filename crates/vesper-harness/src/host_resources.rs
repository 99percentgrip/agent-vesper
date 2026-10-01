#![forbid(unsafe_code)]
//! RRC-owned admission, telemetry, and safety limits for local verification.
//!
//! This module is deliberately independent of providers and hosts.  It derives
//! limits from the live machine, preserves a desktop-safe reserve, and supplies
//! one inherited environment for every Cargo process launched by the Release
//! Recovery Controller (including nested Cargo launched by `cargo xtask`).

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use fs2::FileExt;
use serde::{Deserialize, Serialize};

const GIB: u64 = 1024 * 1024 * 1024;
const MIB: u64 = 1024 * 1024;
const KIB: u64 = 1024;

/// The severity of the observed resource state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePressure {
    /// The host has its reserve plus normal operating headroom.
    #[default]
    Normal,
    /// The host is usable, but RRC must not admit more expensive work.
    Pressure,
    /// The host reserve or swap safety floor has been crossed.
    Critical,
}

impl ResourcePressure {
    #[must_use]
    pub const fn as_label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Pressure => "MEMORY PRESSURE",
            Self::Critical => "CRITICAL",
        }
    }
}

/// Whether a controller-owned gate needs the exclusive expensive-work slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateCost {
    Cheap,
    Expensive,
}

/// Conservative, host-scaled RRC safety policy.
///
/// The defaults intentionally prioritise a responsive interactive workstation
/// over maximum build throughput.  At the observed 27 GiB host this reserves
/// 6.75 GiB (25%), caps Cargo at two jobs, and gives each job a 3 GiB memory
/// envelope for rustc/linker spikes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourcePolicy {
    pub fixed_reserve_bytes: u64,
    pub reserve_fraction_numerator: u64,
    pub reserve_fraction_denominator: u64,
    pub normal_headroom_min_bytes: u64,
    pub normal_headroom_fraction_numerator: u64,
    pub normal_headroom_fraction_denominator: u64,
    pub estimated_rustc_bytes: u64,
    pub max_cargo_jobs: u32,
    pub pressure_swap_growth_bytes_per_minute: u64,
    pub critical_swap_growth_bytes_per_minute: u64,
    pub pressure_psi_some_avg10_bps: u32,
    pub pressure_psi_full_avg10_bps: u32,
    pub critical_psi_full_avg10_bps: u32,
    pub disk_reserve_min_bytes: u64,
    pub disk_reserve_fraction_numerator: u64,
    pub disk_reserve_fraction_denominator: u64,
    pub expected_gate_growth_bytes: u64,
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self {
            // Desktop, browser, editor, Vesper, kernel and page-cache reserve.
            fixed_reserve_bytes: 4 * GIB,
            reserve_fraction_numerator: 1,
            reserve_fraction_denominator: 4,
            // A normal admission needs reserve plus headroom for a sudden
            // linker/rustc burst.  Pressure starts before the reserve itself.
            normal_headroom_min_bytes: 2 * GIB,
            normal_headroom_fraction_numerator: 1,
            normal_headroom_fraction_denominator: 10,
            // Deliberately pessimistic: large feature-complete workspace
            // crates and linkers can exceed an ordinary rustc process.
            estimated_rustc_bytes: 3 * GIB,
            // Never let a 20+ CPU workstation convert itself into 20 rustc
            // processes merely because Cargo's default is CPU-count based.
            max_cargo_jobs: 2,
            pressure_swap_growth_bytes_per_minute: 512 * MIB,
            critical_swap_growth_bytes_per_minute: 2 * GIB,
            pressure_psi_some_avg10_bps: 200,
            pressure_psi_full_avg10_bps: 50,
            critical_psi_full_avg10_bps: 500,
            // Keep room for filesystem metadata and user work; the estimated
            // growth is separately required before a cold local gate begins.
            disk_reserve_min_bytes: 20 * GIB,
            disk_reserve_fraction_numerator: 1,
            disk_reserve_fraction_denominator: 10,
            expected_gate_growth_bytes: 30 * GIB,
        }
    }
}

impl ResourcePolicy {
    #[must_use]
    pub fn reserve_bytes(self, effective_memory_bytes: u64) -> u64 {
        self.fixed_reserve_bytes.max(
            effective_memory_bytes.saturating_mul(self.reserve_fraction_numerator)
                / self.reserve_fraction_denominator,
        )
    }

    #[must_use]
    pub fn normal_headroom_bytes(self, effective_memory_bytes: u64) -> u64 {
        self.normal_headroom_min_bytes.max(
            effective_memory_bytes.saturating_mul(self.normal_headroom_fraction_numerator)
                / self.normal_headroom_fraction_denominator,
        )
    }

    #[must_use]
    pub fn disk_reserve_bytes(self, filesystem_total_bytes: u64) -> u64 {
        self.disk_reserve_min_bytes.max(
            filesystem_total_bytes.saturating_mul(self.disk_reserve_fraction_numerator)
                / self.disk_reserve_fraction_denominator,
        )
    }
}

/// Linux current-pressure signals that are independent of historical logical
/// swap occupancy. PSI values are basis points (100 = 1.00%).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinuxPressureSignals {
    pub memory_psi_some_avg10_bps: Option<u32>,
    pub memory_psi_full_avg10_bps: Option<u32>,
    pub swap_growth_bytes_per_minute: Option<u64>,
    pub zram_logical_used_bytes: Option<u64>,
    pub zram_compressed_bytes: Option<u64>,
    pub zram_physical_used_bytes: Option<u64>,
    pub zram_physical_limit_bytes: Option<u64>,
}

impl LinuxPressureSignals {
    fn with_swap_growth(mut self, growth: Option<u64>) -> Self {
        self.swap_growth_bytes_per_minute = growth;
        self
    }
}

#[derive(Debug, Clone, Copy)]
struct SwapHistory {
    observed_at: Instant,
    used_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PressureDecision {
    pressure: ResourcePressure,
    swap_used_percent: u8,
    reason: &'static str,
}

/// Memory values after physical-memory and cgroup reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapacity {
    pub host_memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
    pub cgroup_memory_limit_bytes: Option<u64>,
    pub cgroup_memory_current_bytes: Option<u64>,
    pub logical_cpus: u32,
    pub linux: LinuxPressureSignals,
}

impl HostCapacity {
    #[must_use]
    pub fn effective_memory_bytes(&self) -> u64 {
        self.cgroup_memory_limit_bytes
            .map_or(self.host_memory_total_bytes, |limit| {
                self.host_memory_total_bytes.min(limit)
            })
    }

    #[must_use]
    pub fn effective_available_bytes(&self) -> u64 {
        let cgroup_available = self
            .cgroup_memory_limit_bytes
            .zip(self.cgroup_memory_current_bytes)
            .map(|(limit, current)| limit.saturating_sub(current));
        cgroup_available.map_or(self.memory_available_bytes, |available| {
            self.memory_available_bytes.min(available)
        })
    }
}

/// RSS and process count for the complete controller-owned descendant tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcessTreeUsage {
    pub rss_bytes: u64,
    pub process_count: u32,
    pub rustc_count: u32,
}

/// Filesystem capacity for RRC's managed target cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskCapacity {
    pub available_bytes: u64,
    pub total_bytes: u64,
    pub target_size_bytes: u64,
}

/// Persistable and renderable observation. Values are live observations, never
/// quotas or estimates presented as measurements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceTelemetry {
    pub platform: String,
    pub host_memory_total_bytes: u64,
    pub effective_memory_bytes: u64,
    pub memory_available_bytes: u64,
    pub cgroup_memory_limit_bytes: Option<u64>,
    pub cgroup_memory_current_bytes: Option<u64>,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
    pub process_tree_rss_bytes: u64,
    pub process_count: u32,
    pub rustc_count: u32,
    pub logical_cpus: u32,
    pub cargo_jobs: u32,
    pub test_threads: u32,
    pub reserve_bytes: u64,
    pub normal_headroom_bytes: u64,
    /// Estimated memory burst required by the admitted Cargo gate.
    #[serde(default)]
    pub required_gate_headroom_bytes: u64,
    /// Available RAM required before a new expensive gate may start.
    pub normal_admission_available_bytes: u64,
    #[serde(default)]
    pub swap_used_percent: u8,
    #[serde(default)]
    pub swap_growth_bytes_per_minute: Option<u64>,
    #[serde(default)]
    pub memory_psi_some_avg10_bps: Option<u32>,
    #[serde(default)]
    pub memory_psi_full_avg10_bps: Option<u32>,
    #[serde(default)]
    pub zram_logical_used_bytes: Option<u64>,
    #[serde(default)]
    pub zram_compressed_bytes: Option<u64>,
    #[serde(default)]
    pub zram_physical_used_bytes: Option<u64>,
    #[serde(default)]
    pub zram_physical_limit_bytes: Option<u64>,
    #[serde(default)]
    pub pressure_reason: String,
    pub pressure: ResourcePressure,
    pub disk_available_bytes: u64,
    pub disk_total_bytes: u64,
    pub disk_reserve_bytes: u64,
    pub expected_gate_growth_bytes: u64,
    pub target_size_bytes: u64,
    pub action: String,
}

impl Default for ResourceTelemetry {
    fn default() -> Self {
        Self {
            platform: std::env::consts::OS.into(),
            host_memory_total_bytes: 0,
            effective_memory_bytes: 0,
            memory_available_bytes: 0,
            cgroup_memory_limit_bytes: None,
            cgroup_memory_current_bytes: None,
            swap_total_bytes: 0,
            swap_free_bytes: 0,
            process_tree_rss_bytes: 0,
            process_count: 0,
            rustc_count: 0,
            logical_cpus: 0,
            cargo_jobs: 0,
            test_threads: 0,
            reserve_bytes: 0,
            normal_headroom_bytes: 0,
            required_gate_headroom_bytes: 0,
            normal_admission_available_bytes: 0,
            swap_used_percent: 0,
            swap_growth_bytes_per_minute: None,
            memory_psi_some_avg10_bps: None,
            memory_psi_full_avg10_bps: None,
            zram_logical_used_bytes: None,
            zram_compressed_bytes: None,
            zram_physical_used_bytes: None,
            zram_physical_limit_bytes: None,
            pressure_reason: "not sampled".into(),
            pressure: ResourcePressure::Normal,
            disk_available_bytes: 0,
            disk_total_bytes: 0,
            disk_reserve_bytes: 0,
            expected_gate_growth_bytes: 0,
            target_size_bytes: 0,
            action: "resource telemetry not sampled".into(),
        }
    }
}

impl ResourceTelemetry {
    #[must_use]
    pub fn render(&self) -> String {
        let cgroup = self
            .cgroup_memory_limit_bytes
            .map(|limit| {
                format!(
                    "{} / {} GiB used",
                    gib(self.cgroup_memory_current_bytes.unwrap_or(0)),
                    gib(limit),
                )
            })
            .unwrap_or_else(|| "unrestricted / unavailable".into());
        let psi = match (
            self.memory_psi_some_avg10_bps,
            self.memory_psi_full_avg10_bps,
        ) {
            (Some(some), Some(full)) => format!(
                "some {:.2}% / full {:.2}% avg10",
                f64::from(some) / 100.0,
                f64::from(full) / 100.0
            ),
            _ => "unavailable".into(),
        };
        let zram = self.zram_physical_used_bytes.map_or_else(
            || "not active / unavailable".into(),
            |physical| {
                format!(
                    "{} GiB physical / {} GiB compressed for {} GiB logical",
                    gib(physical),
                    gib(self.zram_compressed_bytes.unwrap_or(0)),
                    gib(self.zram_logical_used_bytes.unwrap_or(0))
                )
            },
        );
        format!(
            "RESOURCES\nRAM avail   {} / {} GiB\nCgroup      {}\nRRC RSS     {} GiB\nSwap        {} / {} GiB logical ({}%)\nSwap trend  {}\nZram        {}\nMemory PSI  {}\nReserve     {} GiB\nGate need   {} GiB\nDisk free   {} / {} GiB\nTarget      {} GiB\nCargo jobs  {}\nrustc       {}\nPressure    {} — {}\nAction      {}",
            gib(self.memory_available_bytes),
            gib(self.effective_memory_bytes),
            cgroup,
            gib(self.process_tree_rss_bytes),
            gib(self.swap_total_bytes.saturating_sub(self.swap_free_bytes)),
            gib(self.swap_total_bytes),
            self.swap_used_percent,
            self.swap_growth_bytes_per_minute.map_or_else(
                || "collecting baseline".into(),
                |growth| format!("{} MiB/min growth", growth / MIB),
            ),
            zram,
            psi,
            gib(self.reserve_bytes),
            gib(self.required_gate_headroom_bytes),
            gib(self.disk_available_bytes),
            gib(self.disk_total_bytes),
            gib(self.target_size_bytes),
            self.cargo_jobs,
            self.rustc_count,
            self.pressure.as_label(),
            self.pressure_reason,
            self.action,
        )
    }

    #[must_use]
    pub fn memory_headroom_bytes(&self) -> u64 {
        self.memory_available_bytes
            .saturating_sub(self.reserve_bytes)
    }

    #[must_use]
    pub fn expensive_gate_condition(&self) -> String {
        format!(
            "{} GiB available RAM, quiet PSI, stable swap growth, and safe zram backing",
            gib(self.normal_admission_available_bytes),
        )
    }

    #[must_use]
    pub fn materially_recovered_from(&self, prior: &Self) -> bool {
        self.pressure == ResourcePressure::Normal && prior.pressure != ResourcePressure::Normal
    }
}

/// The inherited process policy. It is intentionally environment-based so
/// Cargo launched by `xtask`, `rustup run`, or test helpers cannot escape it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoResourcePolicy {
    pub cargo_jobs: u32,
    pub test_threads: u32,
    pub target_dir: PathBuf,
}

impl CargoResourcePolicy {
    #[must_use]
    pub fn environment(&self) -> Vec<(String, String)> {
        vec![
            ("CARGO_BUILD_JOBS".into(), self.cargo_jobs.to_string()),
            ("RUST_TEST_THREADS".into(), self.test_threads.to_string()),
            (
                "CARGO_TARGET_DIR".into(),
                self.target_dir.to_string_lossy().into_owned(),
            ),
        ]
    }
}

/// A held exclusive expensive-gate slot. Releasing it permits the next RRC
/// epoch to use the managed target cache. It never deletes artifacts.
#[derive(Debug)]
pub struct ExpensiveGateLease {
    _lock: File,
}

/// A single repository-scoped scheduler. File locking makes the serialization
/// survive controller restarts and prevents two epochs sharing a target cache.
#[derive(Debug, Clone)]
pub struct ExpensiveGateScheduler {
    lock_path: PathBuf,
}

impl ExpensiveGateScheduler {
    pub fn new(cache_root: impl Into<PathBuf>) -> io::Result<Self> {
        let cache_root = cache_root.into();
        fs::create_dir_all(&cache_root)?;
        Ok(Self {
            lock_path: cache_root.join("expensive-gate.lock"),
        })
    }

    pub fn try_acquire(&self) -> io::Result<Option<ExpensiveGateLease>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.lock_path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(ExpensiveGateLease { _lock: file })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// A successful admission. The lease remains alive until the child settles.
#[derive(Debug)]
pub struct ResourceAdmission {
    pub cargo: CargoResourcePolicy,
    pub telemetry: ResourceTelemetry,
    pub expensive_lease: Option<ExpensiveGateLease>,
}

/// Why a local gate was withheld or stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceGovernorError {
    Pressure(Box<ResourceTelemetry>),
    Critical(Box<ResourceTelemetry>),
    Disk(Box<ResourceTelemetry>),
    Busy(Box<ResourceTelemetry>),
    Discovery(String),
}

impl ResourceGovernorError {
    #[must_use]
    pub fn telemetry(&self) -> Option<&ResourceTelemetry> {
        match self {
            Self::Pressure(telemetry)
            | Self::Critical(telemetry)
            | Self::Disk(telemetry)
            | Self::Busy(telemetry) => Some(telemetry.as_ref()),
            Self::Discovery(_) => None,
        }
    }
}

impl fmt::Display for ResourceGovernorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pressure(telemetry) => {
                write!(formatter, "host memory pressure: {}", telemetry.action)
            }
            Self::Critical(telemetry) => write!(
                formatter,
                "critical host memory pressure: {}",
                telemetry.action
            ),
            Self::Disk(telemetry) => write!(
                formatter,
                "unsafe target filesystem headroom: {}",
                telemetry.action
            ),
            Self::Busy(_) => {
                formatter.write_str("another RRC expensive gate owns the resource slot")
            }
            Self::Discovery(message) => {
                write!(formatter, "resource discovery unavailable: {message}")
            }
        }
    }
}

impl std::error::Error for ResourceGovernorError {}

/// Platform-extensible resource governor. Linux is implemented through procfs
/// plus cgroup v2/v1; other platforms must add a truthful backend rather than
/// fabricating host capacity.
#[derive(Debug, Clone)]
pub struct HostResourceGovernor {
    policy: ResourcePolicy,
    scheduler: ExpensiveGateScheduler,
    target_dir: PathBuf,
    swap_history: Arc<Mutex<Option<SwapHistory>>>,
}

impl HostResourceGovernor {
    pub fn new(
        policy: ResourcePolicy,
        cache_root: impl Into<PathBuf>,
        target_dir: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        let target_dir = target_dir.into();
        fs::create_dir_all(&target_dir)?;
        Ok(Self {
            policy,
            scheduler: ExpensiveGateScheduler::new(cache_root)?,
            target_dir,
            swap_history: Arc::new(Mutex::new(None)),
        })
    }

    #[must_use]
    pub const fn policy(&self) -> ResourcePolicy {
        self.policy
    }

    pub fn preflight(&self, cost: GateCost) -> Result<ResourceAdmission, ResourceGovernorError> {
        let telemetry = self
            .snapshot(None)
            .map_err(|error| ResourceGovernorError::Discovery(error.to_string()))?;
        self.preflight_telemetry(cost, telemetry)
    }

    /// Applies the admission policy to one already-observed snapshot. Keeping
    /// this separate from procfs discovery gives regression tests a truthful
    /// constrained-host seam without mutable global environment or cgroups.
    fn preflight_telemetry(
        &self,
        cost: GateCost,
        telemetry: ResourceTelemetry,
    ) -> Result<ResourceAdmission, ResourceGovernorError> {
        if telemetry.disk_available_bytes
            < telemetry
                .disk_reserve_bytes
                .saturating_add(telemetry.expected_gate_growth_bytes)
        {
            let mut telemetry = telemetry;
            telemetry.action = format!(
                "local verification paused: insufficient target filesystem headroom; waiting for {} GiB free",
                gib(telemetry
                    .disk_reserve_bytes
                    .saturating_add(telemetry.expected_gate_growth_bytes),),
            );
            return Err(ResourceGovernorError::Disk(Box::new(telemetry)));
        }
        match (cost, telemetry.pressure) {
            (GateCost::Expensive, ResourcePressure::Critical) => {
                let mut telemetry = telemetry;
                telemetry.action = format!(
                    "local verification stopping safely: critical memory or swap pressure; retry only after {}",
                    telemetry.expensive_gate_condition(),
                );
                Err(ResourceGovernorError::Critical(Box::new(telemetry)))
            }
            (GateCost::Expensive, ResourcePressure::Pressure) => {
                let mut telemetry = telemetry;
                telemetry.action = format!(
                    "no new expensive work admitted while host memory pressure persists; waiting for {}",
                    telemetry.expensive_gate_condition(),
                );
                Err(ResourceGovernorError::Pressure(Box::new(telemetry)))
            }
            (GateCost::Expensive, ResourcePressure::Normal) => {
                let Some(lease) = self
                    .scheduler
                    .try_acquire()
                    .map_err(|error| ResourceGovernorError::Discovery(error.to_string()))?
                else {
                    let mut telemetry = telemetry;
                    telemetry.action =
                        "another RRC epoch owns the exclusive expensive-resource slot".into();
                    return Err(ResourceGovernorError::Busy(Box::new(telemetry)));
                };
                Ok(ResourceAdmission {
                    cargo: CargoResourcePolicy {
                        cargo_jobs: telemetry.cargo_jobs,
                        test_threads: telemetry.test_threads,
                        target_dir: self.target_dir.clone(),
                    },
                    telemetry,
                    expensive_lease: Some(lease),
                })
            }
            (GateCost::Cheap, _) => Ok(ResourceAdmission {
                cargo: CargoResourcePolicy {
                    cargo_jobs: telemetry.cargo_jobs,
                    test_threads: telemetry.test_threads,
                    target_dir: self.target_dir.clone(),
                },
                telemetry,
                expensive_lease: None,
            }),
        }
    }

    /// Test-only constrained-host seam. Production admission always reads the
    /// live host through [`Self::preflight`].
    #[cfg(test)]
    pub(crate) fn preflight_observed_for_test(
        &self,
        cost: GateCost,
        capacity: HostCapacity,
        process: ProcessTreeUsage,
        disk: DiskCapacity,
    ) -> Result<ResourceAdmission, ResourceGovernorError> {
        self.preflight_telemetry(cost, self.telemetry_from(capacity, process, disk))
    }

    /// Samples the live host and, when supplied, the owned root process tree.
    pub fn snapshot(&self, root_pid: Option<u32>) -> io::Result<ResourceTelemetry> {
        let mut capacity = discover_host_capacity()?;
        let swap_used = capacity
            .swap_total_bytes
            .saturating_sub(capacity.swap_free_bytes);
        let now = Instant::now();
        let growth = self.swap_history.lock().ok().and_then(|mut history| {
            let growth = history.and_then(|prior| {
                let elapsed_ms = now.saturating_duration_since(prior.observed_at).as_millis();
                if elapsed_ms == 0 || swap_used <= prior.used_bytes {
                    return (elapsed_ms > 0).then_some(0);
                }
                let delta = u128::from(swap_used - prior.used_bytes);
                u64::try_from(delta.saturating_mul(60_000) / elapsed_ms).ok()
            });
            *history = Some(SwapHistory {
                observed_at: now,
                used_bytes: swap_used,
            });
            growth
        });
        capacity.linux = capacity.linux.with_swap_growth(growth);
        let process = root_pid.map_or(Ok(ProcessTreeUsage::default()), process_tree_usage)?;
        let disk = disk_capacity(&self.target_dir)?;
        Ok(self.telemetry_from(capacity, process, disk))
    }

    /// Deterministic policy seam for regression fixtures and constrained-host
    /// tests; production always obtains its inputs from the methods above.
    #[must_use]
    pub fn telemetry_from(
        &self,
        capacity: HostCapacity,
        process: ProcessTreeUsage,
        disk: DiskCapacity,
    ) -> ResourceTelemetry {
        let effective_memory_bytes = capacity.effective_memory_bytes();
        let memory_available_bytes = capacity.effective_available_bytes();
        let reserve_bytes = self.policy.reserve_bytes(effective_memory_bytes);
        let normal_headroom_bytes = self.policy.normal_headroom_bytes(effective_memory_bytes);
        let affordable_jobs = memory_available_bytes
            .saturating_sub(reserve_bytes)
            .checked_div(self.policy.estimated_rustc_bytes)
            .unwrap_or(0)
            .clamp(1, u64::from(self.policy.max_cargo_jobs));
        let required_gate_headroom_bytes = normal_headroom_bytes.max(
            self.policy
                .estimated_rustc_bytes
                .saturating_mul(affordable_jobs),
        );
        let normal_admission_available_bytes =
            reserve_bytes.saturating_add(required_gate_headroom_bytes);
        let decision = classify_pressure(
            &capacity,
            &process,
            memory_available_bytes,
            reserve_bytes,
            required_gate_headroom_bytes,
            self.policy,
        );
        let pressure = decision.pressure;
        let cargo_jobs = cargo_jobs_for(
            &capacity,
            memory_available_bytes,
            reserve_bytes,
            pressure,
            self.policy,
        );
        ResourceTelemetry {
            platform: std::env::consts::OS.into(),
            host_memory_total_bytes: capacity.host_memory_total_bytes,
            effective_memory_bytes,
            memory_available_bytes,
            cgroup_memory_limit_bytes: capacity.cgroup_memory_limit_bytes,
            cgroup_memory_current_bytes: capacity.cgroup_memory_current_bytes,
            swap_total_bytes: capacity.swap_total_bytes,
            swap_free_bytes: capacity.swap_free_bytes,
            process_tree_rss_bytes: process.rss_bytes,
            process_count: process.process_count,
            rustc_count: process.rustc_count,
            logical_cpus: capacity.logical_cpus,
            cargo_jobs,
            test_threads: cargo_jobs.clamp(1, 2),
            reserve_bytes,
            normal_headroom_bytes,
            required_gate_headroom_bytes,
            normal_admission_available_bytes,
            swap_used_percent: decision.swap_used_percent,
            swap_growth_bytes_per_minute: capacity.linux.swap_growth_bytes_per_minute,
            memory_psi_some_avg10_bps: capacity.linux.memory_psi_some_avg10_bps,
            memory_psi_full_avg10_bps: capacity.linux.memory_psi_full_avg10_bps,
            zram_logical_used_bytes: capacity.linux.zram_logical_used_bytes,
            zram_compressed_bytes: capacity.linux.zram_compressed_bytes,
            zram_physical_used_bytes: capacity.linux.zram_physical_used_bytes,
            zram_physical_limit_bytes: capacity.linux.zram_physical_limit_bytes,
            pressure_reason: decision.reason.into(),
            pressure,
            disk_available_bytes: disk.available_bytes,
            disk_total_bytes: disk.total_bytes,
            disk_reserve_bytes: self.policy.disk_reserve_bytes(disk.total_bytes),
            expected_gate_growth_bytes: self.policy.expected_gate_growth_bytes,
            target_size_bytes: disk.target_size_bytes,
            action: match pressure {
                ResourcePressure::Normal => format!(
                    "verification admitted: current headroom, PSI, swap trend, and zram backing are safe ({})",
                    decision.reason,
                ),
                ResourcePressure::Pressure => format!(
                    "waiting for safe host capacity: {} (need {} GiB available including reserve and gate headroom)",
                    decision.reason,
                    gib(normal_admission_available_bytes),
                ),
                ResourcePressure::Critical => format!(
                    "local verification stopping safely: {} (need {} GiB available including reserve and gate headroom)",
                    decision.reason,
                    gib(normal_admission_available_bytes),
                ),
            },
        }
    }
}

fn classify_pressure(
    capacity: &HostCapacity,
    process: &ProcessTreeUsage,
    available: u64,
    reserve: u64,
    required_gate_headroom: u64,
    policy: ResourcePolicy,
) -> PressureDecision {
    let swap_used = capacity
        .swap_total_bytes
        .saturating_sub(capacity.swap_free_bytes);
    let swap_percent = swap_used
        .saturating_mul(100)
        .checked_div(capacity.swap_total_bytes)
        .unwrap_or(0)
        .min(100) as u8;
    let normal_floor = reserve.saturating_add(required_gate_headroom);
    let full_psi = capacity.linux.memory_psi_full_avg10_bps.unwrap_or(0);
    let some_psi = capacity.linux.memory_psi_some_avg10_bps.unwrap_or(0);
    let growth = capacity.linux.swap_growth_bytes_per_minute.unwrap_or(0);
    let effective = capacity.effective_memory_bytes();
    let zram_used = capacity.linux.zram_physical_used_bytes.unwrap_or(0);
    let zram_limit = capacity.linux.zram_physical_limit_bytes.unwrap_or(0);
    let zram_critical = zram_used >= effective.saturating_mul(30) / 100
        || (zram_limit > 0 && zram_used >= zram_limit.saturating_mul(90) / 100);
    let zram_pressure = zram_used >= effective.saturating_mul(20) / 100
        || (zram_limit > 0 && zram_used >= zram_limit.saturating_mul(75) / 100);

    let (pressure, reason) = if available < reserve {
        (
            ResourcePressure::Critical,
            "MemAvailable is below the configured reserve",
        )
    } else if full_psi >= policy.critical_psi_full_avg10_bps {
        (
            ResourcePressure::Critical,
            "memory PSI full contention is critical",
        )
    } else if growth >= policy.critical_swap_growth_bytes_per_minute {
        (
            ResourcePressure::Critical,
            "swap consumption is growing critically fast",
        )
    } else if zram_critical {
        (
            ResourcePressure::Critical,
            "zram physical backing is at a critical bound",
        )
    } else if process.rss_bytes >= effective / 2 && effective > 0 {
        (
            ResourcePressure::Critical,
            "the RRC-owned process tree consumes at least half of effective memory",
        )
    } else if available < normal_floor {
        (
            ResourcePressure::Pressure,
            "MemAvailable cannot cover reserve plus gate headroom",
        )
    } else if full_psi >= policy.pressure_psi_full_avg10_bps
        || some_psi >= policy.pressure_psi_some_avg10_bps
    {
        (
            ResourcePressure::Pressure,
            "memory PSI reports current contention",
        )
    } else if growth >= policy.pressure_swap_growth_bytes_per_minute {
        (
            ResourcePressure::Pressure,
            "swap consumption is growing rapidly",
        )
    } else if zram_pressure {
        (
            ResourcePressure::Pressure,
            "zram physical backing is above its safe bound",
        )
    } else if process.rss_bytes > required_gate_headroom
        && available < normal_floor.saturating_add(process.rss_bytes / 2)
    {
        (
            ResourcePressure::Pressure,
            "the RRC-owned process tree leaves insufficient burst margin",
        )
    } else if swap_percent >= 95
        && available < reserve.saturating_add(required_gate_headroom.saturating_mul(2))
    {
        (
            ResourcePressure::Pressure,
            "swap is nearly exhausted while RAM burst margin is narrow",
        )
    } else {
        (
            ResourcePressure::Normal,
            "current RAM headroom is healthy; PSI is quiet; swap is stable; zram backing is safe",
        )
    };

    PressureDecision {
        pressure,
        swap_used_percent: swap_percent,
        reason,
    }
}

fn cargo_jobs_for(
    capacity: &HostCapacity,
    available: u64,
    reserve: u64,
    pressure: ResourcePressure,
    policy: ResourcePolicy,
) -> u32 {
    let memory_jobs = available
        .saturating_sub(reserve)
        .checked_div(policy.estimated_rustc_bytes)
        .unwrap_or(0)
        .max(1);
    let cpu_jobs = u64::from(capacity.logical_cpus.max(1));
    let pressure_cap = if pressure == ResourcePressure::Normal {
        memory_jobs
    } else {
        1
    };
    pressure_cap
        .min(cpu_jobs)
        .min(u64::from(policy.max_cargo_jobs))
        .max(1) as u32
}

#[cfg(target_os = "linux")]
fn discover_host_capacity() -> io::Result<HostCapacity> {
    let meminfo = read_meminfo(Path::new("/proc/meminfo"))?;
    let host_memory_total_bytes = meminfo_value(&meminfo, "MemTotal")?;
    let memory_available_bytes = meminfo_value(&meminfo, "MemAvailable")?;
    let swap_total_bytes = meminfo_value(&meminfo, "SwapTotal")?;
    let swap_free_bytes = meminfo_value(&meminfo, "SwapFree")?;
    let (cgroup_memory_limit_bytes, cgroup_memory_current_bytes) = linux_cgroup_memory()?;
    let linux = discover_linux_pressure_signals(
        Path::new("/proc/pressure/memory"),
        Path::new("/proc/swaps"),
        Path::new("/sys/block"),
    );
    let logical_cpus = std::thread::available_parallelism()
        .map(|count| u32::try_from(count.get()).unwrap_or(u32::MAX))
        .unwrap_or(1);
    Ok(HostCapacity {
        host_memory_total_bytes,
        memory_available_bytes,
        swap_total_bytes,
        swap_free_bytes,
        cgroup_memory_limit_bytes,
        cgroup_memory_current_bytes,
        logical_cpus,
        linux,
    })
}

/// Windows and macOS are explicit extension points. Their implementations must
/// provide available physical memory plus Job Object/memory-pressure accounting
/// before RRC admits expensive local work on those platforms.
#[cfg(not(target_os = "linux"))]
fn discover_host_capacity() -> io::Result<HostCapacity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "host resource discovery backend is not implemented for this platform",
    ))
}

#[cfg(target_os = "linux")]
fn read_meminfo(path: &Path) -> io::Result<HashMap<String, u64>> {
    let text = fs::read_to_string(path)?;
    let mut values = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Some(number) = value.split_whitespace().next() else {
            continue;
        };
        if let Ok(number) = number.parse::<u64>() {
            values.insert(key.to_owned(), number.saturating_mul(KIB));
        }
    }
    Ok(values)
}

#[cfg(target_os = "linux")]
fn meminfo_value(values: &HashMap<String, u64>, key: &str) -> io::Result<u64> {
    values.get(key).copied().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("missing {key} in /proc/meminfo"),
        )
    })
}

#[cfg(target_os = "linux")]
fn discover_linux_pressure_signals(
    psi_path: &Path,
    swaps_path: &Path,
    sys_block_root: &Path,
) -> LinuxPressureSignals {
    let psi = fs::read_to_string(psi_path).unwrap_or_default();
    let swaps = fs::read_to_string(swaps_path).unwrap_or_default();
    let mut logical = 0_u64;
    let mut compressed = 0_u64;
    let mut physical = 0_u64;
    let mut limit = 0_u64;
    let mut found_zram = false;
    let mut zram_stats_complete = true;
    for line in swaps.lines().skip(1) {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 5 {
            continue;
        }
        let device = Path::new(columns[0]);
        let Some(name) = device.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("zram") {
            continue;
        }
        found_zram = true;
        logical =
            logical.saturating_add(columns[3].parse::<u64>().unwrap_or(0).saturating_mul(KIB));
        match fs::read_to_string(sys_block_root.join(name).join("mm_stat")) {
            Ok(mm_stat) => {
                let values = mm_stat
                    .split_whitespace()
                    .filter_map(|value| value.parse::<u64>().ok())
                    .collect::<Vec<_>>();
                // Kernel ABI: orig_data_size compr_data_size mem_used_total
                // mem_limit mem_used_max same_pages pages_compacted ...
                if values.len() < 3 {
                    zram_stats_complete = false;
                    continue;
                }
                compressed = compressed.saturating_add(values[1]);
                physical = physical.saturating_add(values[2]);
                limit = limit.saturating_add(values.get(3).copied().unwrap_or(0));
            }
            Err(_) => zram_stats_complete = false,
        }
    }
    LinuxPressureSignals {
        memory_psi_some_avg10_bps: parse_psi_avg10_bps(&psi, "some"),
        memory_psi_full_avg10_bps: parse_psi_avg10_bps(&psi, "full"),
        swap_growth_bytes_per_minute: None,
        zram_logical_used_bytes: found_zram.then_some(logical),
        zram_compressed_bytes: (found_zram && zram_stats_complete).then_some(compressed),
        zram_physical_used_bytes: (found_zram && zram_stats_complete).then_some(physical),
        zram_physical_limit_bytes: (found_zram && zram_stats_complete && limit > 0)
            .then_some(limit),
    }
}

#[cfg(target_os = "linux")]
fn parse_psi_avg10_bps(text: &str, class: &str) -> Option<u32> {
    let line = text.lines().find(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|value| value == class)
    })?;
    let value = line
        .split_whitespace()
        .find_map(|column| column.strip_prefix("avg10="))?
        .parse::<f64>()
        .ok()?;
    if !value.is_finite() || value.is_sign_negative() {
        return None;
    }
    Some((value * 100.0).round().min(f64::from(u32::MAX)) as u32)
}

#[cfg(target_os = "linux")]
fn linux_cgroup_memory() -> io::Result<(Option<u64>, Option<u64>)> {
    let cgroup = fs::read_to_string("/proc/self/cgroup")?;
    if let Some(path) = cgroup.lines().find_map(|line| line.strip_prefix("0::")) {
        return cgroup_v2_memory(Path::new("/sys/fs/cgroup"), path);
    }
    cgroup_v1_memory(&cgroup)
}

#[cfg(target_os = "linux")]
fn cgroup_v2_memory(root: &Path, relative: &str) -> io::Result<(Option<u64>, Option<u64>)> {
    let mut current = root.join(relative.trim_start_matches('/'));
    let mut limit = None;
    let mut usage = None;
    loop {
        let candidate = read_cgroup_limit(&current.join("memory.max"))?;
        if candidate.is_some_and(|value| limit.is_none_or(|existing| value < existing)) {
            limit = candidate;
            usage = read_cgroup_limit(&current.join("memory.current"))?;
        }
        if current == root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }
    Ok((limit, usage))
}

#[cfg(target_os = "linux")]
fn cgroup_v1_memory(cgroup: &str) -> io::Result<(Option<u64>, Option<u64>)> {
    let Some(relative) = cgroup.lines().find_map(|line| {
        let mut parts = line.splitn(3, ':');
        let _hierarchy = parts.next()?;
        let controllers = parts.next()?;
        let path = parts.next()?;
        controllers
            .split(',')
            .any(|controller| controller == "memory")
            .then_some(path)
    }) else {
        return Ok((None, None));
    };
    let root = Path::new("/sys/fs/cgroup/memory");
    let mut current = root.join(relative.trim_start_matches('/'));
    let mut limit = None;
    let mut usage = None;
    loop {
        let candidate = read_cgroup_limit(&current.join("memory.limit_in_bytes"))?;
        if candidate.is_some_and(|value| limit.is_none_or(|existing| value < existing)) {
            limit = candidate;
            usage = read_cgroup_limit(&current.join("memory.usage_in_bytes"))?;
        }
        if current == root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }
    Ok((limit, usage))
}

#[cfg(target_os = "linux")]
fn read_cgroup_limit(path: &Path) -> io::Result<Option<u64>> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value = text.trim();
            if value == "max" {
                return Ok(None);
            }
            let parsed = value.parse::<u64>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: {error}", path.display()),
                )
            })?;
            // cgroup v1 uses a huge sentinel to represent no limit.
            Ok((parsed < (1_u64 << 60)).then_some(parsed))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn process_tree_usage(root_pid: u32) -> io::Result<ProcessTreeUsage> {
    let mut pending = vec![root_pid];
    let mut visited = BTreeSet::new();
    let mut usage = ProcessTreeUsage::default();
    while let Some(pid) = pending.pop() {
        if !visited.insert(pid) {
            continue;
        }
        let proc_root = PathBuf::from("/proc").join(pid.to_string());
        let status = match fs::read_to_string(proc_root.join("status")) {
            Ok(status) => status,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        usage.process_count = usage.process_count.saturating_add(1);
        usage.rss_bytes = usage.rss_bytes.saturating_add(status_rss_bytes(&status));
        if is_rustc(&proc_root) {
            usage.rustc_count = usage.rustc_count.saturating_add(1);
        }
        let children = fs::read_to_string(
            proc_root
                .join("task")
                .join(pid.to_string())
                .join("children"),
        )
        .unwrap_or_default();
        pending.extend(
            children
                .split_whitespace()
                .filter_map(|child| child.parse::<u32>().ok()),
        );
    }
    Ok(usage)
}

#[cfg(not(target_os = "linux"))]
fn process_tree_usage(_: u32) -> io::Result<ProcessTreeUsage> {
    Ok(ProcessTreeUsage::default())
}

#[cfg(target_os = "linux")]
fn status_rss_bytes(status: &str) -> u64 {
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
        .saturating_mul(KIB)
}

#[cfg(target_os = "linux")]
fn is_rustc(proc_root: &Path) -> bool {
    let comm = fs::read_to_string(proc_root.join("comm")).unwrap_or_default();
    if comm.trim() == "rustc" {
        return true;
    }
    fs::read(proc_root.join("cmdline"))
        .ok()
        .is_some_and(|bytes| {
            bytes
                .windows(b"rustc".len())
                .any(|window| window == b"rustc")
        })
}

fn disk_capacity(target_dir: &Path) -> io::Result<DiskCapacity> {
    let path = existing_ancestor(target_dir)?;
    Ok(DiskCapacity {
        available_bytes: fs2::available_space(&path)?,
        total_bytes: fs2::total_space(&path)?,
        target_size_bytes: directory_size(target_dir)?,
    })
}

fn existing_ancestor(path: &Path) -> io::Result<PathBuf> {
    let mut current = path;
    loop {
        if current.exists() {
            return Ok(current.to_path_buf());
        }
        current = current.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no existing target filesystem ancestor",
            )
        })?;
    }
}

fn directory_size(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0_u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

fn gib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / GIB as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capacity(available: u64, swap_used_percent: u64, cpus: u32) -> HostCapacity {
        let swap_total = 8 * GIB;
        HostCapacity {
            host_memory_total_bytes: 8 * GIB,
            memory_available_bytes: available,
            swap_total_bytes: swap_total,
            swap_free_bytes: swap_total.saturating_sub(swap_total * swap_used_percent / 100),
            cgroup_memory_limit_bytes: None,
            cgroup_memory_current_bytes: None,
            logical_cpus: cpus,
            linux: LinuxPressureSignals::default(),
        }
    }

    fn disk(available: u64) -> DiskCapacity {
        DiskCapacity {
            available_bytes: available,
            total_bytes: 500 * GIB,
            target_size_bytes: 7 * GIB,
        }
    }

    #[test]
    fn memory_not_cpu_derives_cargo_budget() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let telemetry = governor.telemetry_from(
            capacity(7 * GIB, 0, 96),
            ProcessTreeUsage::default(),
            disk(100 * GIB),
        );
        assert_eq!(telemetry.cargo_jobs, 1, "limited RAM must beat CPU count");
        assert_eq!(telemetry.test_threads, 1);
        assert_eq!(telemetry.pressure, ResourcePressure::Normal);
    }

    #[test]
    fn cgroup_limit_reduces_effective_capacity() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let telemetry = governor.telemetry_from(
            HostCapacity {
                host_memory_total_bytes: 64 * GIB,
                memory_available_bytes: 40 * GIB,
                swap_total_bytes: 0,
                swap_free_bytes: 0,
                cgroup_memory_limit_bytes: Some(8 * GIB),
                cgroup_memory_current_bytes: Some(3 * GIB),
                logical_cpus: 64,
                linux: LinuxPressureSignals::default(),
            },
            ProcessTreeUsage::default(),
            disk(100 * GIB),
        );
        assert_eq!(telemetry.effective_memory_bytes, 8 * GIB);
        assert_eq!(telemetry.memory_available_bytes, 5 * GIB);
        assert_eq!(telemetry.pressure, ResourcePressure::Pressure);
        assert_eq!(telemetry.cargo_jobs, 1);
    }

    #[test]
    fn rapid_swap_growth_and_true_low_ram_are_current_pressure() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let mut growing = capacity(7 * GIB, 30, 4);
        growing.linux.swap_growth_bytes_per_minute = Some(GIB);
        let pressure =
            governor.telemetry_from(growing, ProcessTreeUsage::default(), disk(100 * GIB));
        assert_eq!(pressure.pressure, ResourcePressure::Pressure);
        assert!(pressure.pressure_reason.contains("growing rapidly"));

        let critical = governor.telemetry_from(
            capacity(3 * GIB, 90, 4),
            ProcessTreeUsage::default(),
            disk(100 * GIB),
        );
        assert_eq!(critical.pressure, ResourcePressure::Critical);
        assert!(critical.pressure_reason.contains("configured reserve"));
    }

    #[test]
    fn live_incident_stale_logical_swap_with_healthy_ram_is_not_pressure_by_itself() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let swap_total = 8_589_930_496;
        let incident_swap_used = 3_227_684_352;
        let telemetry = governor.telemetry_from(
            HostCapacity {
                host_memory_total_bytes: 27 * GIB,
                memory_available_bytes: 20 * GIB + 900 * 1024 * 1024,
                swap_total_bytes: swap_total,
                swap_free_bytes: swap_total - incident_swap_used,
                cgroup_memory_limit_bytes: None,
                cgroup_memory_current_bytes: None,
                logical_cpus: 20,
                linux: LinuxPressureSignals {
                    memory_psi_some_avg10_bps: Some(0),
                    memory_psi_full_avg10_bps: Some(0),
                    zram_logical_used_bytes: Some(incident_swap_used),
                    zram_compressed_bytes: Some(860 * MIB),
                    zram_physical_used_bytes: Some(900 * MIB),
                    ..LinuxPressureSignals::default()
                },
            },
            ProcessTreeUsage::default(),
            disk(100 * GIB),
        );

        assert_eq!(
            telemetry.pressure,
            ResourcePressure::Normal,
            "the exact incident's historical logical swap must not override 20.9 GiB MemAvailable"
        );
        assert_eq!(telemetry.swap_used_percent, 37);
        assert_eq!(telemetry.zram_logical_used_bytes, Some(incident_swap_used));
        assert_eq!(telemetry.zram_compressed_bytes, Some(860 * MIB));
        assert_eq!(telemetry.zram_physical_used_bytes, Some(900 * MIB));
    }

    #[test]
    fn zram_logical_occupancy_and_physical_backing_are_independent() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let mut safe = capacity(7 * GIB, 75, 4);
        safe.linux.zram_logical_used_bytes = Some(6 * GIB);
        safe.linux.zram_physical_used_bytes = Some(700 * MIB);
        let safe = governor.telemetry_from(safe, ProcessTreeUsage::default(), disk(100 * GIB));
        assert_eq!(safe.pressure, ResourcePressure::Normal);

        let mut unsafe_backing = capacity(7 * GIB, 10, 4);
        unsafe_backing.linux.zram_logical_used_bytes = Some(800 * MIB);
        unsafe_backing.linux.zram_physical_used_bytes = Some(2 * GIB);
        let unsafe_backing =
            governor.telemetry_from(unsafe_backing, ProcessTreeUsage::default(), disk(100 * GIB));
        assert_eq!(unsafe_backing.pressure, ResourcePressure::Pressure);
        assert!(
            unsafe_backing
                .pressure_reason
                .contains("zram physical backing")
        );
    }

    #[test]
    fn memory_psi_detects_current_contention_with_healthy_memavailable() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let mut capacity = capacity(7 * GIB, 0, 4);
        capacity.linux.memory_psi_some_avg10_bps = Some(250);
        capacity.linux.memory_psi_full_avg10_bps = Some(0);
        let telemetry =
            governor.telemetry_from(capacity, ProcessTreeUsage::default(), disk(100 * GIB));
        assert_eq!(telemetry.pressure, ResourcePressure::Pressure);
        assert!(telemetry.pressure_reason.contains("PSI"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_zram_mm_stat_reports_physical_not_logical_consumption() {
        let temp = tempfile::tempdir().unwrap();
        let sys_block = temp.path().join("sys-block");
        fs::create_dir_all(sys_block.join("zram0")).unwrap();
        let psi = temp.path().join("memory.pressure");
        let swaps = temp.path().join("swaps");
        fs::write(
            &psi,
            "some avg10=0.01 avg60=0.00 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
        )
        .unwrap();
        fs::write(
            &swaps,
            "Filename Type Size Used Priority\n/dev/zram0 partition 8388608 2883584 100\n",
        )
        .unwrap();
        fs::write(
            sys_block.join("zram0/mm_stat"),
            "2883584000 901775360 943718400 2147483648 943718400 0 0 0\n",
        )
        .unwrap();

        let signals = discover_linux_pressure_signals(&psi, &swaps, &sys_block);
        assert_eq!(signals.zram_logical_used_bytes, Some(2_883_584 * KIB));
        assert_eq!(signals.zram_compressed_bytes, Some(901_775_360));
        assert_eq!(signals.zram_physical_used_bytes, Some(943_718_400));
        assert_eq!(signals.zram_physical_limit_bytes, Some(2_147_483_648));
        assert_eq!(signals.memory_psi_some_avg10_bps, Some(1));
        assert_eq!(signals.memory_psi_full_avg10_bps, Some(0));

        fs::remove_file(sys_block.join("zram0/mm_stat")).unwrap();
        let unavailable = discover_linux_pressure_signals(&psi, &swaps, &sys_block);
        assert_eq!(unavailable.zram_logical_used_bytes, Some(2_883_584 * KIB));
        assert_eq!(unavailable.zram_compressed_bytes, None);
        assert_eq!(unavailable.zram_physical_used_bytes, None);
    }

    #[test]
    fn rrc_process_tree_rss_participates_in_current_risk_classification() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let telemetry = governor.telemetry_from(
            capacity(7 * GIB, 0, 4),
            ProcessTreeUsage {
                rss_bytes: 5 * GIB,
                process_count: 9,
                rustc_count: 2,
            },
            disk(100 * GIB),
        );

        assert_eq!(telemetry.pressure, ResourcePressure::Critical);
        assert!(telemetry.pressure_reason.contains("RRC-owned process tree"));
    }

    #[test]
    fn expensive_slot_serializes_runnable_gates() {
        let temp = tempfile::tempdir().unwrap();
        let scheduler = ExpensiveGateScheduler::new(temp.path()).unwrap();
        let first = scheduler.try_acquire().unwrap();
        assert!(first.is_some());
        assert!(scheduler.try_acquire().unwrap().is_none());
        drop(first);
        assert!(scheduler.try_acquire().unwrap().is_some());
    }

    #[test]
    fn disk_headroom_requires_reserve_and_growth_budget() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let telemetry = governor.telemetry_from(
            capacity(7 * GIB, 0, 4),
            ProcessTreeUsage::default(),
            disk(45 * GIB),
        );
        assert!(
            telemetry.disk_available_bytes
                < telemetry.disk_reserve_bytes + telemetry.expected_gate_growth_bytes
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cgroup_v2_discovery_uses_the_strictest_ancestor_limit_and_its_usage() {
        let temp = tempfile::tempdir().unwrap();
        let leaf = temp.path().join("user.slice/app.scope");
        fs::create_dir_all(&leaf).unwrap();
        fs::write(leaf.join("memory.max"), (8 * GIB).to_string()).unwrap();
        fs::write(leaf.join("memory.current"), (3 * GIB).to_string()).unwrap();
        let parent = leaf.parent().unwrap();
        fs::write(parent.join("memory.max"), (6 * GIB).to_string()).unwrap();
        fs::write(parent.join("memory.current"), (4 * GIB).to_string()).unwrap();
        fs::write(temp.path().join("memory.max"), "max").unwrap();

        let (limit, current) = cgroup_v2_memory(temp.path(), "/user.slice/app.scope").unwrap();
        assert_eq!(limit, Some(6 * GIB));
        assert_eq!(current, Some(4 * GIB));
    }

    #[test]
    fn resource_telemetry_contains_live_process_fields() {
        let temp = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy::default(),
            temp.path().join("locks"),
            temp.path().join("target"),
        )
        .unwrap();
        let telemetry = governor.telemetry_from(
            capacity(7 * GIB, 0, 4),
            ProcessTreeUsage {
                rss_bytes: 123 * 1024 * 1024,
                process_count: 4,
                rustc_count: 2,
            },
            disk(100 * GIB),
        );
        assert_eq!(telemetry.process_tree_rss_bytes, 123 * 1024 * 1024);
        assert_eq!(telemetry.process_count, 4);
        assert_eq!(telemetry.rustc_count, 2);
        assert!(telemetry.render().contains("rustc       2"));
    }
}
