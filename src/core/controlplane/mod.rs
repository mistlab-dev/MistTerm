//! 受控执行控制面客户端（Plan → Lease → Run）。
//!
//! 执行出口在 mist-server Runner；本模块只提交 Plan / 轮询 / 启动 Run / 映射结果。

use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::core::batch_exec::BatchExecRow;
use crate::core::team::{normalize_api_base, TeamApiError};

#[derive(Debug, Clone, Serialize)]
pub struct CreatePlanRequest {
    pub intent: String,
    pub environment: String,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanStep {
    pub effector: String,
    pub targets: Vec<String>,
    pub command: String,
    pub readonly: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Plan {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub decision_status: String,
    #[serde(default)]
    pub risk_level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lease {
    pub id: String,
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePlanResponse {
    pub plan: Plan,
    #[serde(default)]
    pub lease: Option<Lease>,
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetPlanResponse {
    pub plan: Plan,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunHost {
    #[serde(default)]
    pub host_id: String,
    #[serde(default)]
    pub host_name: String,
    #[serde(default)]
    pub host_address: String,
    pub ok: bool,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Run {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub hosts: Vec<RunHost>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartRunResponse {
    pub run: Run,
}

pub struct ControlPlaneClient {
    base_url: String,
    http: Client,
}

impl ControlPlaneClient {
    pub fn new(api_base: &str) -> Result<Self, String> {
        let base_url = normalize_api_base(api_base);
        if base_url.is_empty() {
            return Err("team API base URL is empty".into());
        }
        // Runs can take up to ~60s on the server; allow headroom.
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { base_url, http })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn decode_err(status: reqwest::StatusCode, body: String) -> TeamApiError {
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        v.get("code")
                            .and_then(|c| c.as_str())
                            .map(|s| s.to_string())
                    })
            })
            .unwrap_or(body);
        TeamApiError {
            status: status.as_u16(),
            message,
            conflict_fragment: None,
        }
    }

    fn decode<T: for<'de> Deserialize<'de>>(
        resp: reqwest::blocking::Response,
    ) -> Result<T, TeamApiError> {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            return Err(Self::decode_err(status, text));
        }
        serde_json::from_str(&text).map_err(|e| TeamApiError {
            status: status.as_u16(),
            message: format!("decode: {e}; body={text}"),
            conflict_fragment: None,
        })
    }

    pub fn create_plan(
        &self,
        team_id: &str,
        bearer: &str,
        req: &CreatePlanRequest,
        idempotency_key: Option<&str>,
    ) -> Result<CreatePlanResponse, TeamApiError> {
        let path = format!("/v1/teams/{team_id}/cp/plans");
        let mut builder = self.http.post(self.url(&path)).bearer_auth(bearer).json(req);
        if let Some(k) = idempotency_key {
            if !k.is_empty() {
                builder = builder.header("Idempotency-Key", k);
            }
        }
        let resp = builder.send().map_err(|e| TeamApiError {
            status: 0,
            message: e.to_string(),
            conflict_fragment: None,
        })?;
        Self::decode(resp)
    }

    pub fn get_plan(
        &self,
        team_id: &str,
        bearer: &str,
        plan_id: &str,
    ) -> Result<Plan, TeamApiError> {
        let path = format!("/v1/teams/{team_id}/cp/plans/{plan_id}");
        let resp = self
            .http
            .get(self.url(&path))
            .bearer_auth(bearer)
            .send()
            .map_err(|e| TeamApiError {
                status: 0,
                message: e.to_string(),
                conflict_fragment: None,
            })?;
        Ok(Self::decode::<GetPlanResponse>(resp)?.plan)
    }

    pub fn approve_plan(
        &self,
        team_id: &str,
        bearer: &str,
        plan_id: &str,
        targets: &[String],
    ) -> Result<CreatePlanResponse, TeamApiError> {
        let path = format!("/v1/teams/{team_id}/cp/plans/{plan_id}/approve");
        let body = serde_json::json!({ "targets": targets });
        let resp = self
            .http
            .post(self.url(&path))
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .map_err(|e| TeamApiError {
                status: 0,
                message: e.to_string(),
                conflict_fragment: None,
            })?;
        Self::decode(resp)
    }

    pub fn start_run(
        &self,
        team_id: &str,
        bearer: &str,
        plan_id: &str,
        lease_token: &str,
    ) -> Result<Run, TeamApiError> {
        let path = format!("/v1/teams/{team_id}/cp/plans/{plan_id}/runs");
        let resp = self
            .http
            .post(self.url(&path))
            .bearer_auth(bearer)
            .header("X-Mist-Lease", lease_token)
            .json(&serde_json::json!({}))
            .send()
            .map_err(|e| TeamApiError {
                status: 0,
                message: e.to_string(),
                conflict_fragment: None,
            })?;
        Ok(Self::decode::<StartRunResponse>(resp)?.run)
    }

    /// Submit plan; if pending and `auto_approve`, approve as Admin; then StartRun.
    pub fn execute_plan_blocking(
        &self,
        team_id: &str,
        bearer: &str,
        req: &CreatePlanRequest,
        target_labels: &[(String, String)], // (host:id, label)
        auto_approve: bool,
        poll_secs: u64,
    ) -> Result<(String, Vec<BatchExecRow>), String> {
        let created = self
            .create_plan(team_id, bearer, req, None)
            .map_err(|e| format!("create plan: {e}"))?;
        let mut plan = created.plan;
        let mut lease = created.lease;

        if plan.status == "denied" {
            return Err(format!(
                "plan denied ({})",
                created.code.unwrap_or_else(|| "denied.policy".into())
            ));
        }

        if plan.status == "pending_approval" {
            if auto_approve {
                let targets: Vec<String> = req
                    .steps
                    .iter()
                    .flat_map(|s| s.targets.iter().cloned())
                    .collect();
                let approved = self
                    .approve_plan(team_id, bearer, &plan.id, &targets)
                    .map_err(|e| format!("approve plan: {e}"))?;
                plan = approved.plan;
                lease = approved.lease;
            } else {
                let deadline = std::time::Instant::now() + Duration::from_secs(poll_secs);
                loop {
                    if std::time::Instant::now() > deadline {
                        return Ok((
                            req.steps
                                .first()
                                .map(|s| s.command.clone())
                                .unwrap_or_default(),
                            waiting_rows(
                                target_labels,
                                &format!(
                                    "waiting for approval (plan {}) — approve in mistlab Console",
                                    plan.id
                                ),
                            ),
                        ));
                    }
                    std::thread::sleep(Duration::from_secs(3));
                    plan = self
                        .get_plan(team_id, bearer, &plan.id)
                        .map_err(|e| format!("poll plan: {e}"))?;
                    if plan.status == "allow" || plan.status == "allow_auto" {
                        // Lease only returned on approve response; non-admin waiters stop here.
                        return Ok((
                            req.steps
                                .first()
                                .map(|s| s.command.clone())
                                .unwrap_or_default(),
                            waiting_rows(
                                target_labels,
                                &format!(
                                    "plan {} approved — re-confirm to run with a fresh lease, or run from Console",
                                    plan.id
                                ),
                            ),
                        ));
                    }
                    if plan.status == "denied" {
                        return Err("plan denied by approver".into());
                    }
                }
            }
        }

        let Some(lease) = lease.take().filter(|l| !l.token.is_empty()) else {
            return Err(format!("no lease for plan {} (status={})", plan.id, plan.status));
        };

        let run = self
            .start_run(team_id, bearer, &plan.id, &lease.token)
            .map_err(|e| format!("start run: {e}"))?;

        let label_by_id: std::collections::HashMap<&str, &str> = target_labels
            .iter()
            .map(|(id, l)| (id.as_str(), l.as_str()))
            .collect();

        let rows: Vec<BatchExecRow> = run
            .hosts
            .into_iter()
            .map(|h| {
                let host_ref = if h.host_id.is_empty() {
                    String::new()
                } else if h.host_id.starts_with("host:") {
                    h.host_id.clone()
                } else {
                    format!("host:{}", h.host_id)
                };
                let label = label_by_id
                    .get(host_ref.as_str())
                    .copied()
                    .or_else(|| label_by_id.get(h.host_id.as_str()).copied())
                    .unwrap_or(h.host_name.as_str())
                    .to_string();
                let target_id = if host_ref.is_empty() {
                    format!("team:{}", h.host_id)
                } else {
                    format!(
                        "team:{}",
                        host_ref.strip_prefix("host:").unwrap_or(&host_ref)
                    )
                };
                BatchExecRow {
                    target_id,
                    label: if label.is_empty() {
                        h.host_address.clone()
                    } else {
                        label
                    },
                    ok: h.ok,
                    exit_code: h.exit_code,
                    output: if h.output.is_empty() {
                        h.summary.clone()
                    } else {
                        h.output.clone()
                    },
                    error: if h.error.is_empty() {
                        None
                    } else {
                        Some(h.error)
                    },
                    duration_ms: h.duration_ms,
                }
            })
            .collect();

        let cmd = req
            .steps
            .first()
            .map(|s| s.command.clone())
            .unwrap_or_default();
        Ok((cmd, rows))
    }
}

fn waiting_rows(target_labels: &[(String, String)], message: &str) -> Vec<BatchExecRow> {
    target_labels
        .iter()
        .map(|(id, label)| BatchExecRow {
            target_id: id.clone(),
            label: label.clone(),
            ok: false,
            exit_code: None,
            output: String::new(),
            error: Some(message.to_string()),
            duration_ms: 0,
        })
        .collect()
}

/// Infer environment label from team server tags / group strings.
pub fn infer_environment(tags: &[String]) -> String {
    let lower: Vec<String> = tags.iter().map(|t| t.to_ascii_lowercase()).collect();
    let has = |want: &str| {
        lower.iter().any(|t| {
            t == want
                || t == &format!("env={want}")
                || t.ends_with(&format!("/{want}"))
                || t.contains(want)
        })
    };
    if has("prod") || has("production") {
        return "prod".into();
    }
    if has("staging") || has("stage") {
        return "staging".into();
    }
    if has("dev") || has("development") {
        return "dev".into();
    }
    "staging".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_env_prod() {
        assert_eq!(
            infer_environment(&["env=prod".into(), "db".into()]),
            "prod"
        );
    }

    #[test]
    fn infer_env_staging_default() {
        assert_eq!(infer_environment(&["web".into()]), "staging");
    }
}
