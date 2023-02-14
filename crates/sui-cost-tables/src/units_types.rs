// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::ops::{Add, Bound, Mul};

use anyhow::anyhow;
use move_core_types::gas_algebra::{
    GasQuantity, InternalGas, InternalGasUnit, ToUnit, ToUnitFractional, UnitDiv,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub enum GasUnit {}

pub type Gas = GasQuantity<GasUnit>;

impl ToUnit<InternalGasUnit> for GasUnit {
    const MULTIPLIER: u64 = 1;
}

impl ToUnitFractional<GasUnit> for InternalGasUnit {
    const NOMINATOR: u64 = 1;
    const DENOMINATOR: u64 = 1;
}

pub const INSTRUCTION_TIER_DEFAULT: u64 = 1;

pub const STACK_HEIGHT_TIER_DEFAULT: u64 = 1;
pub const STACK_SIZE_TIER_DEFAULT: u64 = 1;

// The cost table holds the tiers and curves for instruction costs.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, Deserialize)]
pub struct CostTable {
    pub instruction_tiers: BTreeMap<u64, u64>,
    pub stack_height_tiers: BTreeMap<u64, u64>,
    pub stack_size_tiers: BTreeMap<u64, u64>,
}

impl CostTable {
    pub fn instruction_tier(&self, instr_count: u64) -> (u64, Option<u64>) {
        let current_cost = *self
            .instruction_tiers
            .get(&instr_count)
            .or_else(|| {
                self.instruction_tiers
                    .range(..instr_count)
                    .next_back()
                    .map(|(_, v)| v)
            })
            .unwrap_or(&INSTRUCTION_TIER_DEFAULT);
        let next_tier_start = self
            .instruction_tiers
            .range::<u64, _>((Bound::Excluded(instr_count), Bound::Unbounded))
            .next()
            .map(|(next_tier_start, _)| *next_tier_start);
        (current_cost, next_tier_start)
    }

    pub fn stack_height_tier(&self, stack_height: u64) -> (u64, Option<u64>) {
        let current_cost = *self
            .stack_height_tiers
            .get(&stack_height)
            .or_else(|| {
                self.stack_height_tiers
                    .range(..stack_height)
                    .next_back()
                    .map(|(_, v)| v)
            })
            .unwrap_or(&STACK_HEIGHT_TIER_DEFAULT);
        let next_tier_start = self
            .stack_height_tiers
            .range::<u64, _>((Bound::Excluded(stack_height), Bound::Unbounded))
            .next()
            .map(|(next_tier_start, _)| *next_tier_start);
        (current_cost, next_tier_start)
    }

    pub fn stack_size_tier(&self, stack_size: u64) -> (u64, Option<u64>) {
        let current_cost = *self
            .stack_size_tiers
            .get(&stack_size)
            .or_else(|| {
                self.stack_size_tiers
                    .range(..stack_size)
                    .next_back()
                    .map(|(_, v)| v)
            })
            .unwrap_or(&STACK_SIZE_TIER_DEFAULT);
        let next_tier_start = self
            .stack_size_tiers
            .range::<u64, _>((Bound::Excluded(stack_size), Bound::Unbounded))
            .next()
            .map(|(next_tier_start, _)| *next_tier_start);
        (current_cost, next_tier_start)
    }
}

/// The  `GasCost` tracks:
/// - instruction cost: how much time/computational power is needed to perform the instruction
/// - memory cost: how much memory is required for the instruction, and storage overhead
/// - stack height: how high is the stack growing (regardless of size in bytes)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GasCost {
    pub instruction_gas: u64,
    pub memory_gas: u64,
    pub stack_height_gas: u64,
}

impl GasCost {
    pub fn new(instruction_gas: u64, memory_gas: u64, stack_height_gas: u64) -> Self {
        Self {
            instruction_gas,
            memory_gas,
            stack_height_gas,
        }
    }

    /// Convert a GasCost to a total gas charge in `InternalGas`.
    #[inline]
    pub fn total(&self) -> u64 {
        self.instruction_gas
            .add(self.memory_gas)
            .add(self.stack_height_gas)
    }

    #[inline]
    pub fn total_internal(&self) -> InternalGas {
        GasQuantity::new(
            self.instruction_gas
                .add(self.memory_gas)
                .add(self.stack_height_gas),
        )
    }
}

/// Linear equation for: Y = Mx + C
/// For example when calculating the price for publishing a package,
/// we may want to price per byte, with some offset
/// Hence: cost = package_cost_per_byte * num_bytes + base_cost
/// For consistency, the units must be defined as UNIT(package_cost_per_byte) = UnitDiv(UNIT(cost), UNIT(num_bytes))
pub struct LinearEquation<YUnit, XUnit> {
    offset: GasQuantity<YUnit>,
    slope: GasQuantity<UnitDiv<YUnit, XUnit>>,
    min: GasQuantity<YUnit>,
    max: GasQuantity<YUnit>,
}

impl<YUnit, XUnit> LinearEquation<YUnit, XUnit> {
    pub const fn new(
        slope: GasQuantity<UnitDiv<YUnit, XUnit>>,
        offset: GasQuantity<YUnit>,
        min: GasQuantity<YUnit>,
        max: GasQuantity<YUnit>,
    ) -> Self {
        Self {
            offset,
            slope,
            min,
            max,
        }
    }
    #[inline]
    pub fn calculate(&self, x: GasQuantity<XUnit>) -> anyhow::Result<GasQuantity<YUnit>> {
        let y = self.offset + self.slope.mul(x);

        if y < self.min {
            Err(anyhow!(
                "Value {} is below minimum allowed {}",
                u64::from(y),
                u64::from(self.min)
            ))
        } else if y > self.max {
            Err(anyhow!(
                "Value {} is above maximum allowed {}",
                u64::from(y),
                u64::from(self.max)
            ))
        } else {
            Ok(y)
        }
    }
}
