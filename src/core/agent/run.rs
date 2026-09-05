//! AgentRun 快照（UI 侧可持有可变状态）。

use super::planner::StepProposal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentPhase {
    Idle,
    AwaitingL1,
    AwaitingL2,
    Executing,
    Done,
}

#[derive(Debug, Clone)]
pub struct AgentRunSnapshot {
    pub phase: AgentPhase,
    pub intent: String,
    pub proposal: StepProposal,
    pub target_count: usize,
    pub gate_message: String,
    pub needs_l2: bool,
    pub l1_confirmed: bool,
}
