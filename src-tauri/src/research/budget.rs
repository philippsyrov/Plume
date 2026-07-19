//! Fixed Rust-side budgets for the Stage A research workflow.
//!
//! Logical workflow turns and physical provider calls are deliberately
//! separate. A turn gets one base call and, only after a typed recovery is
//! reserved, one retry. This makes the 13-turn / 26-call ceiling mechanical
//! instead of relying on the harness to count correctly.

#![allow(dead_code)]

use serde::Serialize;

pub const MAX_LOGICAL_TURNS: u32 = 13;
pub const MAX_RECOVERY_CALLS: u32 = 13;
pub const MAX_PROVIDER_CALLS: u32 = 26;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryReason {
    MalformedFraming,
    ContextOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetRefusal {
    LogicalTurnLimit,
    RecoveryLimit,
    ProviderCallLimit,
    NoActiveLogicalTurn,
    RecoveryBeforeBaseCall,
    RecoveryAlreadyUsed,
    ProviderCallNotAuthorized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchBudgetSnapshot {
    pub logical_turns: u32,
    pub recovery_calls: u32,
    pub provider_calls: u32,
}

#[derive(Debug, Default)]
pub struct ResearchBudget {
    logical_turns: u32,
    recovery_calls: u32,
    provider_calls: u32,
    active_turn: bool,
    recovery_used_in_turn: bool,
    provider_calls_in_turn: u8,
}

impl ResearchBudget {
    pub fn begin_logical_turn(&mut self) -> Result<(), BudgetRefusal> {
        if self.logical_turns >= MAX_LOGICAL_TURNS {
            return Err(BudgetRefusal::LogicalTurnLimit);
        }
        self.logical_turns = self.logical_turns.saturating_add(1);
        self.active_turn = true;
        self.recovery_used_in_turn = false;
        self.provider_calls_in_turn = 0;
        Ok(())
    }

    pub fn reserve_provider_call(&mut self) -> Result<(), BudgetRefusal> {
        if !self.active_turn {
            return Err(BudgetRefusal::NoActiveLogicalTurn);
        }
        if self.provider_calls >= MAX_PROVIDER_CALLS {
            return Err(BudgetRefusal::ProviderCallLimit);
        }
        let authorized = match self.provider_calls_in_turn {
            0 => true,
            1 => self.recovery_used_in_turn,
            _ => false,
        };
        if !authorized {
            return Err(BudgetRefusal::ProviderCallNotAuthorized);
        }
        self.provider_calls = self.provider_calls.saturating_add(1);
        self.provider_calls_in_turn = self.provider_calls_in_turn.saturating_add(1);
        Ok(())
    }

    pub fn reserve_recovery(&mut self, _reason: RecoveryReason) -> Result<(), BudgetRefusal> {
        if !self.active_turn {
            return Err(BudgetRefusal::NoActiveLogicalTurn);
        }
        if self.provider_calls_in_turn == 0 {
            return Err(BudgetRefusal::RecoveryBeforeBaseCall);
        }
        if self.recovery_used_in_turn {
            return Err(BudgetRefusal::RecoveryAlreadyUsed);
        }
        if self.recovery_calls >= MAX_RECOVERY_CALLS {
            return Err(BudgetRefusal::RecoveryLimit);
        }
        if self.provider_calls >= MAX_PROVIDER_CALLS {
            return Err(BudgetRefusal::ProviderCallLimit);
        }
        self.recovery_calls = self.recovery_calls.saturating_add(1);
        self.recovery_used_in_turn = true;
        Ok(())
    }

    pub fn snapshot(&self) -> ResearchBudgetSnapshot {
        ResearchBudgetSnapshot {
            logical_turns: self.logical_turns,
            recovery_calls: self.recovery_calls,
            provider_calls: self.provider_calls,
        }
    }
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
