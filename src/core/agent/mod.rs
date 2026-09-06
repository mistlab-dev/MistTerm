//! 对话运维 Agent（v2）：AgentLoop / Gate / ExecutionBase 内部 API。
//!
//! 不对用户暴露 Skill 目录；命令由 Planner 动态提议。

mod explainer;
mod gate;
mod planner;
mod run;
mod summarize;

pub use explainer::{explain_policy_decision, PolicyExplanation};
pub use gate::{gate_decision, looks_like_mutate_command, GateDecision, GateLevel};
pub use planner::{
    build_planner_system_prompt, looks_like_host_ops_intent, parse_llm_plan_response,
    propose_step, propose_step_with_context, HostExecutionSummary, LastBatchContext, StepProposal,
};
pub use run::{AgentPhase, AgentRunSnapshot};
pub use summarize::{first_useful_line, host_result_summary, summarize_batch_rows, truncate_chars};
