use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ExtensionMap;

/// Provenance for one normalized usage value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsageProvenance {
    /// Reported exactly by the provider/runtime.
    Exact,
    /// Estimated by the harness or adapter.
    Estimated,
    /// Not available.
    Unavailable,
}

/// One optional normalized usage amount and its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageMeasurement {
    /// Unit count, absent when unavailable.
    pub value: Option<u64>,
    /// Source quality.
    pub provenance: UsageProvenance,
}

impl UsageMeasurement {
    /// Creates an exact measurement.
    #[must_use]
    pub const fn exact(value: u64) -> Self {
        Self {
            value: Some(value),
            provenance: UsageProvenance::Exact,
        }
    }

    /// Creates an unavailable measurement.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            value: None,
            provenance: UsageProvenance::Unavailable,
        }
    }

    /// Returns whether value/provenance form a valid pair.
    #[must_use]
    pub const fn is_consistent(self) -> bool {
        matches!(
            (self.value, self.provenance),
            (None, UsageProvenance::Unavailable)
                | (Some(_), UsageProvenance::Exact | UsageProvenance::Estimated)
        )
    }
}

/// Whether a usage update describes a delta or a cumulative provider total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsageMode {
    /// Additive delta.
    Delta,
    /// Cumulative total.
    Cumulative,
}

/// Estimated monetary cost represented without floating-point serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EstimatedCost {
    /// ISO-style currency code.
    pub currency: String,
    /// Millionths of one currency unit.
    pub micros: u64,
    /// Cost provenance.
    pub provenance: UsageProvenance,
}

/// Provider-neutral usage record. Provider fields remain namespaced metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedUsage {
    /// Delta versus cumulative semantics.
    pub mode: UsageMode,
    /// Input units.
    pub input: UsageMeasurement,
    /// Output units.
    pub output: UsageMeasurement,
    /// Total units.
    pub total: UsageMeasurement,
    /// Cached input units.
    pub cached_input: UsageMeasurement,
    /// Cache-write units.
    pub cache_write: UsageMeasurement,
    /// Reasoning units.
    pub reasoning: UsageMeasurement,
    /// Tool units.
    pub tool: UsageMeasurement,
    /// Image units.
    pub image: UsageMeasurement,
    /// Audio units.
    pub audio: UsageMeasurement,
    /// Optional estimated monetary cost.
    pub estimated_cost: Option<EstimatedCost>,
    /// Provider-specific usage values.
    #[serde(default)]
    pub provider_metadata: ExtensionMap,
}

/// Relationship between a reported total and known component values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsageTotalConsistency {
    /// Total equals known input plus output.
    Consistent,
    /// Provider total differs; raw values remain preserved.
    Inconsistent,
    /// One or more required values are unavailable.
    Indeterminate,
}

/// Checked usage arithmetic failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum UsageArithmeticError {
    /// Only delta records can be added to an aggregate.
    #[error("usage update is cumulative, not additive")]
    NotDelta,
    /// One counter overflowed.
    #[error("usage arithmetic overflow")]
    Overflow,
    /// Measurements had an invalid value/provenance pairing.
    #[error("usage measurement has inconsistent value and provenance")]
    InvalidMeasurement,
}

impl NormalizedUsage {
    /// Creates an all-unavailable usage record without converting absence to zero.
    #[must_use]
    pub fn unavailable(mode: UsageMode) -> Self {
        Self {
            mode,
            input: UsageMeasurement::unavailable(),
            output: UsageMeasurement::unavailable(),
            total: UsageMeasurement::unavailable(),
            cached_input: UsageMeasurement::unavailable(),
            cache_write: UsageMeasurement::unavailable(),
            reasoning: UsageMeasurement::unavailable(),
            tool: UsageMeasurement::unavailable(),
            image: UsageMeasurement::unavailable(),
            audio: UsageMeasurement::unavailable(),
            estimated_cost: None,
            provider_metadata: ExtensionMap::default(),
        }
    }

    /// Reports total consistency without repairing or discarding provider values.
    #[must_use]
    pub fn total_consistency(&self) -> UsageTotalConsistency {
        match (self.input.value, self.output.value, self.total.value) {
            (Some(input), Some(output), Some(total)) => match input.checked_add(output) {
                Some(computed) if computed == total => UsageTotalConsistency::Consistent,
                Some(_) | None => UsageTotalConsistency::Inconsistent,
            },
            _ => UsageTotalConsistency::Indeterminate,
        }
    }

    /// Adds a delta using checked arithmetic while retaining conservative provenance.
    pub fn checked_add_delta(&mut self, delta: &Self) -> Result<(), UsageArithmeticError> {
        if delta.mode != UsageMode::Delta {
            return Err(UsageArithmeticError::NotDelta);
        }
        for measurement in [
            delta.input,
            delta.output,
            delta.total,
            delta.cached_input,
            delta.cache_write,
            delta.reasoning,
            delta.tool,
            delta.image,
            delta.audio,
        ] {
            if !measurement.is_consistent() {
                return Err(UsageArithmeticError::InvalidMeasurement);
            }
        }
        add_measurement(&mut self.input, delta.input)?;
        add_measurement(&mut self.output, delta.output)?;
        add_measurement(&mut self.total, delta.total)?;
        add_measurement(&mut self.cached_input, delta.cached_input)?;
        add_measurement(&mut self.cache_write, delta.cache_write)?;
        add_measurement(&mut self.reasoning, delta.reasoning)?;
        add_measurement(&mut self.tool, delta.tool)?;
        add_measurement(&mut self.image, delta.image)?;
        add_measurement(&mut self.audio, delta.audio)?;
        Ok(())
    }
}

fn add_measurement(
    aggregate: &mut UsageMeasurement,
    delta: UsageMeasurement,
) -> Result<(), UsageArithmeticError> {
    let Some(delta_value) = delta.value else {
        return Ok(());
    };
    if aggregate.value.is_none() {
        *aggregate = delta;
        return Ok(());
    }
    let value = aggregate.value.expect("checked above");
    aggregate.value = Some(
        value
            .checked_add(delta_value)
            .ok_or(UsageArithmeticError::Overflow)?,
    );
    aggregate.provenance = match (aggregate.provenance, delta.provenance) {
        (UsageProvenance::Exact, UsageProvenance::Exact) => UsageProvenance::Exact,
        _ => UsageProvenance::Estimated,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_distinguishes_exact_estimated_and_unavailable() {
        assert!(UsageMeasurement::exact(4).is_consistent());
        assert!(UsageMeasurement::unavailable().is_consistent());
        assert!(
            !UsageMeasurement {
                value: Some(4),
                provenance: UsageProvenance::Unavailable,
            }
            .is_consistent()
        );
    }

    #[test]
    fn delta_and_cumulative_are_not_interchangeable_and_totals_are_observable() {
        let mut aggregate = NormalizedUsage::unavailable(UsageMode::Cumulative);
        let mut delta = NormalizedUsage::unavailable(UsageMode::Delta);
        delta.input = UsageMeasurement::exact(3);
        delta.output = UsageMeasurement::exact(2);
        delta.total = UsageMeasurement::exact(6);
        aggregate.checked_add_delta(&delta).unwrap();
        assert_eq!(
            aggregate.total_consistency(),
            UsageTotalConsistency::Inconsistent
        );
        assert_eq!(
            aggregate.checked_add_delta(&aggregate.clone()),
            Err(UsageArithmeticError::NotDelta)
        );
    }
}
