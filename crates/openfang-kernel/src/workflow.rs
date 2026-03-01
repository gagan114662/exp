//! Workflow engine — multi-step agent pipeline execution.
//!
//! A workflow defines a sequence of steps where each step routes
//! a task to a specific agent. Steps can:
//! - Pass their output as input to the next step
//! - Run in sequence (pipeline) or in parallel (fan-out)
//! - Conditionally skip based on previous output
//! - Loop until a condition is met
//! - Store outputs in named variables for later reference
//!
//! Workflows are defined as Rust structs or loaded from JSON.

use chrono::{DateTime, Utc};
use openfang_types::agent::AgentId;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Unique identifier for a workflow definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(pub Uuid);

impl WorkflowId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for WorkflowId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a running workflow instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowRunId(pub Uuid);

impl WorkflowRunId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for WorkflowRunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkflowRunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A workflow definition — a named sequence of steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Unique identifier.
    pub id: WorkflowId,
    /// Human-readable name.
    pub name: String,
    /// Description of what this workflow does.
    pub description: String,
    /// The steps in execution order.
    pub steps: Vec<WorkflowStep>,
    /// Created at.
    pub created_at: DateTime<Utc>,
}

/// A single step in a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// Step name for logging/display.
    pub name: String,
    /// Which agent to route this step to.
    pub agent: StepAgent,
    /// The prompt template. Use `{{input}}` for previous output, `{{var_name}}` for variables.
    pub prompt_template: String,
    /// Execution mode for this step.
    pub mode: StepMode,
    /// Maximum time for this step in seconds (default: 120).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Error handling mode for this step (default: Fail).
    #[serde(default)]
    pub error_mode: ErrorMode,
    /// Optional variable name to store this step's output in.
    #[serde(default)]
    pub output_var: Option<String>,
}

fn default_timeout() -> u64 {
    120
}

/// How to identify the agent for a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepAgent {
    /// Reference an agent by UUID.
    ById { id: String },
    /// Reference an agent by name (first match).
    ByName { name: String },
}

/// Execution mode for a workflow step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepMode {
    /// Execute sequentially — this step runs after the previous completes.
    #[default]
    Sequential,
    /// Fan-out — this step runs in parallel with subsequent FanOut steps until Collect.
    FanOut,
    /// Collect results from all preceding fan-out steps.
    Collect,
    /// Conditional — skip this step if previous output doesn't contain `condition` (case-insensitive).
    Conditional { condition: String },
    /// Loop — repeat this step until output contains `until` or `max_iterations` reached.
    Loop { max_iterations: u32, until: String },
}

/// Error handling mode for a workflow step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorMode {
    /// Abort the workflow on error (default).
    #[default]
    Fail,
    /// Skip this step on error and continue.
    Skip,
    /// Retry the step up to N times before failing.
    Retry { max_retries: u32 },
}

/// The current state of a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunState {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

impl WorkflowRunState {
    fn as_str(&self) -> &'static str {
        match self {
            WorkflowRunState::Pending => "pending",
            WorkflowRunState::Running => "running",
            WorkflowRunState::Paused => "paused",
            WorkflowRunState::Completed => "completed",
            WorkflowRunState::Failed => "failed",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "pending" => WorkflowRunState::Pending,
            "running" => WorkflowRunState::Running,
            "paused" => WorkflowRunState::Paused,
            "completed" => WorkflowRunState::Completed,
            "failed" => WorkflowRunState::Failed,
            _ => WorkflowRunState::Failed,
        }
    }
}

/// A running workflow instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    /// Run instance ID.
    pub id: WorkflowRunId,
    /// The workflow being run.
    pub workflow_id: WorkflowId,
    /// Workflow name (copied for quick access).
    pub workflow_name: String,
    /// Initial input to the workflow.
    pub input: String,
    /// Current state.
    pub state: WorkflowRunState,
    /// Results from each completed step.
    pub step_results: Vec<StepResult>,
    /// Final output (set when workflow completes).
    pub output: Option<String>,
    /// Error message if failed.
    pub error: Option<String>,
    /// Started at.
    pub started_at: DateTime<Utc>,
    /// Completed at.
    pub completed_at: Option<DateTime<Utc>>,
}

/// Result from a single workflow step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Step name.
    pub step_name: String,
    /// Agent that executed this step.
    pub agent_id: String,
    /// Agent name.
    pub agent_name: String,
    /// Output from this step.
    pub output: String,
    /// Token usage.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// The workflow engine — manages definitions and executes pipeline runs.
pub struct WorkflowEngine {
    /// Registered workflow definitions.
    workflows: Arc<RwLock<HashMap<WorkflowId, Workflow>>>,
    /// Active and completed workflow runs.
    runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>,
    /// Optional persistent store for durable workflow state.
    store: Option<Arc<Mutex<Connection>>>,
}

impl WorkflowEngine {
    /// Create a new workflow engine.
    pub fn new() -> Self {
        Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
            runs: Arc::new(RwLock::new(HashMap::new())),
            store: None,
        }
    }

    /// Create a workflow engine backed by SQLite persistence.
    pub fn new_persistent(conn: Arc<Mutex<Connection>>) -> Self {
        let engine = Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
            runs: Arc::new(RwLock::new(HashMap::new())),
            store: Some(conn),
        };
        engine.hydrate_from_store();
        engine
    }

    fn hydrate_from_store(&self) {
        let Some(store) = &self.store else {
            return;
        };
        let conn = match store.lock() {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to lock workflow store: {e}");
                return;
            }
        };

        let mut workflows = HashMap::new();
        if let Ok(mut stmt) = conn.prepare("SELECT workflow_json FROM workflow_defs") {
            if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                for row in rows.flatten() {
                    match serde_json::from_str::<Workflow>(&row) {
                        Ok(wf) => {
                            workflows.insert(wf.id, wf);
                        }
                        Err(e) => warn!("Failed to decode persisted workflow def: {e}"),
                    }
                }
            }
        }

        let mut runs = HashMap::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, workflow_id, workflow_name, input, state, output, error, started_at, completed_at
             FROM workflow_runs",
        ) {
            if let Ok(rows) = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let workflow_id: String = row.get(1)?;
                let workflow_name: String = row.get(2)?;
                let input: String = row.get(3)?;
                let state: String = row.get(4)?;
                let output: Option<String> = row.get(5)?;
                let error: Option<String> = row.get(6)?;
                let started_at: String = row.get(7)?;
                let completed_at: Option<String> = row.get(8)?;
                Ok((
                    id,
                    workflow_id,
                    workflow_name,
                    input,
                    state,
                    output,
                    error,
                    started_at,
                    completed_at,
                ))
            }) {
                for row in rows.flatten() {
                    let (
                        run_id_s,
                        workflow_id_s,
                        workflow_name,
                        input,
                        state_s,
                        output,
                        error,
                        started_at_s,
                        completed_at_s,
                    ) = row;
                    let Ok(run_id_uuid) = Uuid::parse_str(&run_id_s) else {
                        continue;
                    };
                    let Ok(workflow_id_uuid) = Uuid::parse_str(&workflow_id_s) else {
                        continue;
                    };
                    let started_at = DateTime::parse_from_rfc3339(&started_at_s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    let completed_at = completed_at_s.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });
                    runs.insert(
                        WorkflowRunId(run_id_uuid),
                        WorkflowRun {
                            id: WorkflowRunId(run_id_uuid),
                            workflow_id: WorkflowId(workflow_id_uuid),
                            workflow_name,
                            input,
                            state: WorkflowRunState::from_str(&state_s),
                            step_results: Vec::new(),
                            output,
                            error,
                            started_at,
                            completed_at,
                        },
                    );
                }
            }
        }

        for (run_id, run) in &mut runs {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT step_name, agent_id, agent_name, output, input_tokens, output_tokens, duration_ms
                 FROM workflow_step_runs WHERE run_id = ?1 ORDER BY step_index ASC",
            ) {
                let key = run_id.to_string();
                if let Ok(rows) = stmt.query_map(params![key], |row| {
                    Ok(StepResult {
                        step_name: row.get(0)?,
                        agent_id: row.get(1)?,
                        agent_name: row.get(2)?,
                        output: row.get(3)?,
                        input_tokens: row.get::<_, i64>(4)? as u64,
                        output_tokens: row.get::<_, i64>(5)? as u64,
                        duration_ms: row.get::<_, i64>(6)? as u64,
                    })
                }) {
                    run.step_results = rows.flatten().collect();
                }
            }
        }

        if let Ok(mut lock) = self.workflows.try_write() {
            *lock = workflows;
        }
        if let Ok(mut lock) = self.runs.try_write() {
            *lock = runs;
        }
    }

    fn persist_workflow_def(&self, workflow: &Workflow) {
        let Some(store) = &self.store else {
            return;
        };
        let Ok(conn) = store.lock() else {
            return;
        };
        let workflow_json = match serde_json::to_string(workflow) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to serialize workflow for persistence: {e}");
                return;
            }
        };
        if let Err(e) = conn.execute(
            "INSERT INTO workflow_defs (id, name, workflow_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               workflow_json = excluded.workflow_json,
               updated_at = excluded.updated_at",
            params![
                workflow.id.to_string(),
                workflow.name,
                workflow_json,
                workflow.created_at.to_rfc3339(),
                Utc::now().to_rfc3339()
            ],
        ) {
            warn!("Failed to persist workflow definition: {e}");
        }
    }

    fn persist_run(&self, run: &WorkflowRun) {
        let Some(store) = &self.store else {
            return;
        };
        let Ok(conn) = store.lock() else {
            return;
        };
        let _ = conn.execute(
            "INSERT INTO workflow_runs (
               id, workflow_id, workflow_name, input, state, output, error, started_at, completed_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
               state = excluded.state,
               output = excluded.output,
               error = excluded.error,
               completed_at = excluded.completed_at,
               updated_at = excluded.updated_at",
            params![
                run.id.to_string(),
                run.workflow_id.to_string(),
                run.workflow_name,
                run.input,
                run.state.as_str(),
                run.output,
                run.error,
                run.started_at.to_rfc3339(),
                run.completed_at.map(|d| d.to_rfc3339()),
                Utc::now().to_rfc3339(),
            ],
        );
    }

    fn persist_step_result(&self, run_id: WorkflowRunId, step_index: usize, result: &StepResult) {
        let Some(store) = &self.store else {
            return;
        };
        let Ok(conn) = store.lock() else {
            return;
        };
        let _ = conn.execute(
            "INSERT INTO workflow_step_runs (
               run_id, step_index, step_name, agent_id, agent_name, output, input_tokens, output_tokens, duration_ms, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(run_id, step_index) DO UPDATE SET
               output = excluded.output,
               input_tokens = excluded.input_tokens,
               output_tokens = excluded.output_tokens,
               duration_ms = excluded.duration_ms",
            params![
                run_id.to_string(),
                step_index as i64,
                result.step_name,
                result.agent_id,
                result.agent_name,
                result.output,
                result.input_tokens as i64,
                result.output_tokens as i64,
                result.duration_ms as i64,
                Utc::now().to_rfc3339(),
            ],
        );
        let _ = conn.execute(
            "INSERT INTO workflow_resume_cursor (run_id, next_step_index, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(run_id) DO UPDATE SET
               next_step_index = excluded.next_step_index,
               updated_at = excluded.updated_at",
            params![
                run_id.to_string(),
                (step_index + 1) as i64,
                Utc::now().to_rfc3339()
            ],
        );
    }

    fn load_resume_cursor(&self, run_id: WorkflowRunId) -> usize {
        let Some(store) = &self.store else {
            return 0;
        };
        let Ok(conn) = store.lock() else {
            return 0;
        };
        conn.query_row(
            "SELECT next_step_index FROM workflow_resume_cursor WHERE run_id = ?1",
            params![run_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| v.max(0) as usize)
        .unwrap_or(0)
    }

    fn set_run_state(&self, run_id: WorkflowRunId, state: WorkflowRunState) {
        let Some(store) = &self.store else {
            return;
        };
        let Ok(conn) = store.lock() else {
            return;
        };
        let _ = conn.execute(
            "UPDATE workflow_runs SET state = ?2, updated_at = ?3 WHERE id = ?1",
            params![run_id.to_string(), state.as_str(), Utc::now().to_rfc3339()],
        );
    }

    /// Register a new workflow definition.
    pub async fn register(&self, workflow: Workflow) -> WorkflowId {
        let id = workflow.id;
        self.persist_workflow_def(&workflow);
        self.workflows.write().await.insert(id, workflow);
        info!(workflow_id = %id, "Workflow registered");
        id
    }

    /// List all registered workflows.
    pub async fn list_workflows(&self) -> Vec<Workflow> {
        self.workflows.read().await.values().cloned().collect()
    }

    /// Get a specific workflow by ID.
    pub async fn get_workflow(&self, id: WorkflowId) -> Option<Workflow> {
        self.workflows.read().await.get(&id).cloned()
    }

    /// Remove a workflow definition.
    pub async fn remove_workflow(&self, id: WorkflowId) -> bool {
        let removed = self.workflows.write().await.remove(&id).is_some();
        if removed {
            if let Some(store) = &self.store {
                if let Ok(conn) = store.lock() {
                    let _ = conn.execute(
                        "DELETE FROM workflow_defs WHERE id = ?1",
                        params![id.to_string()],
                    );
                }
            }
        }
        removed
    }

    /// Maximum number of retained workflow runs. Oldest completed/failed
    /// runs are evicted when this limit is exceeded.
    const MAX_RETAINED_RUNS: usize = 200;

    /// Start a workflow run. Returns the run ID and a handle to check progress.
    ///
    /// The actual execution is driven externally by calling `execute_run()`
    /// with the kernel handle, since the workflow engine doesn't own the kernel.
    pub async fn create_run(
        &self,
        workflow_id: WorkflowId,
        input: String,
    ) -> Option<WorkflowRunId> {
        let workflow = self.workflows.read().await.get(&workflow_id)?.clone();
        let run_id = WorkflowRunId::new();

        let run = WorkflowRun {
            id: run_id,
            workflow_id,
            workflow_name: workflow.name,
            input,
            state: WorkflowRunState::Pending,
            step_results: Vec::new(),
            output: None,
            error: None,
            started_at: Utc::now(),
            completed_at: None,
        };

        let mut runs = self.runs.write().await;
        runs.insert(run_id, run);
        if let Some(r) = runs.get(&run_id) {
            self.persist_run(r);
            if let Some(store) = &self.store {
                if let Ok(conn) = store.lock() {
                    let _ = conn.execute(
                        "INSERT INTO workflow_resume_cursor (run_id, next_step_index, updated_at)
                         VALUES (?1, 0, ?2)
                         ON CONFLICT(run_id) DO UPDATE SET
                           next_step_index = excluded.next_step_index,
                           updated_at = excluded.updated_at",
                        params![run_id.to_string(), Utc::now().to_rfc3339()],
                    );
                }
            }
        }

        // Evict oldest completed/failed runs when we exceed the cap
        if runs.len() > Self::MAX_RETAINED_RUNS {
            let mut evictable: Vec<(WorkflowRunId, DateTime<Utc>)> = runs
                .iter()
                .filter(|(_, r)| {
                    matches!(
                        r.state,
                        WorkflowRunState::Completed | WorkflowRunState::Failed
                    )
                })
                .map(|(id, r)| (*id, r.started_at))
                .collect();

            // Sort oldest first
            evictable.sort_by_key(|(_, t)| *t);

            let to_remove = runs.len() - Self::MAX_RETAINED_RUNS;
            for (id, _) in evictable.into_iter().take(to_remove) {
                runs.remove(&id);
                debug!(run_id = %id, "Evicted old workflow run");
            }
        }

        Some(run_id)
    }

    /// Get the current state of a workflow run.
    pub async fn get_run(&self, run_id: WorkflowRunId) -> Option<WorkflowRun> {
        self.runs.read().await.get(&run_id).cloned()
    }

    /// List all workflow runs (optionally filtered by state).
    pub async fn list_runs(&self, state_filter: Option<&str>) -> Vec<WorkflowRun> {
        self.runs
            .read()
            .await
            .values()
            .filter(|r| {
                state_filter
                    .map(|f| match f {
                        "pending" => matches!(r.state, WorkflowRunState::Pending),
                        "running" => matches!(r.state, WorkflowRunState::Running),
                        "completed" => matches!(r.state, WorkflowRunState::Completed),
                        "paused" => matches!(r.state, WorkflowRunState::Paused),
                        "failed" => matches!(r.state, WorkflowRunState::Failed),
                        _ => true,
                    })
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// Pause a running workflow run.
    pub async fn pause_run(&self, run_id: WorkflowRunId) -> Result<(), String> {
        let mut runs = self.runs.write().await;
        let run = runs
            .get_mut(&run_id)
            .ok_or_else(|| "Workflow run not found".to_string())?;
        if matches!(
            run.state,
            WorkflowRunState::Completed | WorkflowRunState::Failed
        ) {
            return Err("Cannot pause completed/failed workflow run".to_string());
        }
        run.state = WorkflowRunState::Paused;
        self.persist_run(run);
        self.set_run_state(run_id, WorkflowRunState::Paused);
        Ok(())
    }

    /// Return the next step index for a paused/running run and mark it running.
    pub async fn resume_run(&self, run_id: WorkflowRunId) -> Result<usize, String> {
        let mut runs = self.runs.write().await;
        let run = runs
            .get_mut(&run_id)
            .ok_or_else(|| "Workflow run not found".to_string())?;
        if matches!(
            run.state,
            WorkflowRunState::Completed | WorkflowRunState::Failed
        ) {
            return Err("Cannot resume completed/failed workflow run".to_string());
        }
        run.state = WorkflowRunState::Running;
        self.persist_run(run);
        self.set_run_state(run_id, WorkflowRunState::Running);
        Ok(self.load_resume_cursor(run_id))
    }

    /// Find stale running runs and mark them paused for resume.
    pub async fn recover_stalled_runs(&self, stale_after_secs: u64) -> Vec<WorkflowRunId> {
        let now = Utc::now();
        let stale: Vec<WorkflowRunId> = self
            .runs
            .read()
            .await
            .values()
            .filter(|r| {
                matches!(r.state, WorkflowRunState::Running)
                    && (now - r.started_at).num_seconds() >= stale_after_secs as i64
            })
            .map(|r| r.id)
            .collect();

        for run_id in &stale {
            let _ = self.pause_run(*run_id).await;
        }
        stale
    }

    /// Replace `{{var_name}}` references in a template with stored variable values.
    fn expand_variables(template: &str, input: &str, vars: &HashMap<String, String>) -> String {
        let mut result = template.replace("{{input}}", input);
        for (key, value) in vars {
            result = result.replace(&format!("{{{{{key}}}}}"), value);
        }
        result
    }

    /// Execute a single step with error mode handling. Returns (output, input_tokens, output_tokens).
    async fn execute_step_with_error_mode<F, Fut>(
        step: &WorkflowStep,
        agent_id: AgentId,
        prompt: String,
        send_message: &F,
    ) -> Result<Option<(String, u64, u64)>, String>
    where
        F: Fn(AgentId, String) -> Fut,
        Fut: std::future::Future<Output = Result<(String, u64, u64), String>>,
    {
        let timeout_dur = std::time::Duration::from_secs(step.timeout_secs);

        match &step.error_mode {
            ErrorMode::Fail => {
                let result = tokio::time::timeout(timeout_dur, send_message(agent_id, prompt))
                    .await
                    .map_err(|_| {
                        format!(
                            "Step '{}' timed out after {}s",
                            step.name, step.timeout_secs
                        )
                    })?
                    .map_err(|e| format!("Step '{}' failed: {}", step.name, e))?;
                Ok(Some(result))
            }
            ErrorMode::Skip => {
                match tokio::time::timeout(timeout_dur, send_message(agent_id, prompt)).await {
                    Ok(Ok(result)) => Ok(Some(result)),
                    Ok(Err(e)) => {
                        warn!("Step '{}' failed (skipping): {e}", step.name);
                        Ok(None)
                    }
                    Err(_) => {
                        warn!(
                            "Step '{}' timed out (skipping) after {}s",
                            step.name, step.timeout_secs
                        );
                        Ok(None)
                    }
                }
            }
            ErrorMode::Retry { max_retries } => {
                let mut last_err = String::new();
                for attempt in 0..=*max_retries {
                    match tokio::time::timeout(timeout_dur, send_message(agent_id, prompt.clone()))
                        .await
                    {
                        Ok(Ok(result)) => return Ok(Some(result)),
                        Ok(Err(e)) => {
                            last_err = e.to_string();
                            if attempt < *max_retries {
                                warn!(
                                    "Step '{}' attempt {} failed: {e}, retrying",
                                    step.name,
                                    attempt + 1
                                );
                            }
                        }
                        Err(_) => {
                            last_err = format!("timed out after {}s", step.timeout_secs);
                            if attempt < *max_retries {
                                warn!(
                                    "Step '{}' attempt {} timed out, retrying",
                                    step.name,
                                    attempt + 1
                                );
                            }
                        }
                    }
                }
                Err(format!(
                    "Step '{}' failed after {} retries: {last_err}",
                    step.name, max_retries
                ))
            }
        }
    }

    /// Execute a workflow run step-by-step.
    ///
    /// This method takes a closure that sends messages to agents,
    /// so the workflow engine remains decoupled from the kernel.
    pub async fn execute_run<F, Fut>(
        &self,
        run_id: WorkflowRunId,
        agent_resolver: impl Fn(&StepAgent) -> Option<(AgentId, String)>,
        send_message: F,
    ) -> Result<String, String>
    where
        F: Fn(AgentId, String) -> Fut,
        Fut: std::future::Future<Output = Result<(String, u64, u64), String>>,
    {
        // Get the run and workflow
        let (workflow, input, prior_results, prior_output) = {
            let mut runs = self.runs.write().await;
            let run = runs.get_mut(&run_id).ok_or("Workflow run not found")?;
            if matches!(run.state, WorkflowRunState::Completed) {
                return Ok(run.output.clone().unwrap_or_default());
            }
            run.state = WorkflowRunState::Running;
            self.persist_run(run);
            self.set_run_state(run_id, WorkflowRunState::Running);

            let workflow = self
                .workflows
                .read()
                .await
                .get(&run.workflow_id)
                .ok_or("Workflow definition not found")?
                .clone();

            (
                workflow,
                run.input.clone(),
                run.step_results.clone(),
                run.output.clone(),
            )
        };

        info!(
            run_id = %run_id,
            workflow = %workflow.name,
            steps = workflow.steps.len(),
            "Starting workflow execution"
        );

        if let Some(output) = prior_output {
            return Ok(output);
        }

        let mut current_input = input.clone();
        let mut all_outputs: Vec<String> = Vec::new();
        let mut variables: HashMap<String, String> = HashMap::new();
        let mut i = self.load_resume_cursor(run_id);

        if !prior_results.is_empty() {
            if let Some(last) = prior_results.last() {
                current_input = last.output.clone();
            }
            all_outputs.extend(prior_results.iter().map(|r| r.output.clone()));
            for (idx, result) in prior_results.iter().enumerate() {
                if let Some(step) = workflow.steps.get(idx) {
                    if let Some(var) = &step.output_var {
                        variables.insert(var.clone(), result.output.clone());
                    }
                }
            }
        }

        while i < workflow.steps.len() {
            if let Some(r) = self.runs.read().await.get(&run_id) {
                if matches!(r.state, WorkflowRunState::Paused) {
                    info!(run_id = %run_id, "Workflow execution paused");
                    return Err("Workflow paused".to_string());
                }
            }
            let step = &workflow.steps[i];

            debug!(
                step = i + 1,
                name = %step.name,
                "Executing workflow step"
            );

            match &step.mode {
                StepMode::Sequential => {
                    let (agent_id, agent_name) = agent_resolver(&step.agent)
                        .ok_or_else(|| format!("Agent not found for step '{}'", step.name))?;

                    let prompt =
                        Self::expand_variables(&step.prompt_template, &current_input, &variables);

                    let start = std::time::Instant::now();
                    let result =
                        Self::execute_step_with_error_mode(step, agent_id, prompt, &send_message)
                            .await;
                    let duration_ms = start.elapsed().as_millis() as u64;

                    match result {
                        Ok(Some((output, input_tokens, output_tokens))) => {
                            let step_result = StepResult {
                                step_name: step.name.clone(),
                                agent_id: agent_id.to_string(),
                                agent_name,
                                output: output.clone(),
                                input_tokens,
                                output_tokens,
                                duration_ms,
                            };
                            if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                let persisted_index = r.step_results.len();
                                r.step_results.push(step_result.clone());
                                self.persist_step_result(run_id, persisted_index, &step_result);
                                self.persist_run(r);
                            }

                            if let Some(ref var) = step.output_var {
                                variables.insert(var.clone(), output.clone());
                            }

                            all_outputs.push(output.clone());
                            current_input = output;
                            info!(step = i + 1, name = %step.name, duration_ms, "Step completed");
                        }
                        Ok(None) => {
                            // Step was skipped (ErrorMode::Skip)
                            info!(step = i + 1, name = %step.name, "Step skipped");
                        }
                        Err(e) => {
                            if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                r.state = WorkflowRunState::Failed;
                                r.error = Some(e.clone());
                                r.completed_at = Some(Utc::now());
                                self.persist_run(r);
                            }
                            return Err(e);
                        }
                    }
                }

                StepMode::FanOut => {
                    // Collect consecutive FanOut steps and run them in parallel
                    let mut fan_out_steps = vec![(i, step)];
                    let mut j = i + 1;
                    while j < workflow.steps.len() {
                        if matches!(workflow.steps[j].mode, StepMode::FanOut) {
                            fan_out_steps.push((j, &workflow.steps[j]));
                            j += 1;
                        } else {
                            break;
                        }
                    }

                    // Build all futures
                    let mut futures = Vec::new();
                    let mut step_infos = Vec::new();

                    for (idx, fan_step) in &fan_out_steps {
                        let (agent_id, agent_name) =
                            agent_resolver(&fan_step.agent).ok_or_else(|| {
                                format!("Agent not found for step '{}'", fan_step.name)
                            })?;
                        let prompt = Self::expand_variables(
                            &fan_step.prompt_template,
                            &current_input,
                            &variables,
                        );
                        let timeout_dur = std::time::Duration::from_secs(fan_step.timeout_secs);

                        step_infos.push((*idx, fan_step.name.clone(), agent_id, agent_name));
                        futures.push(tokio::time::timeout(
                            timeout_dur,
                            send_message(agent_id, prompt),
                        ));
                    }

                    let start = std::time::Instant::now();
                    let results = futures::future::join_all(futures).await;
                    let duration_ms = start.elapsed().as_millis() as u64;

                    for (k, result) in results.into_iter().enumerate() {
                        let (_, ref step_name, agent_id, ref agent_name) = step_infos[k];
                        let fan_step = fan_out_steps[k].1;

                        match result {
                            Ok(Ok((output, input_tokens, output_tokens))) => {
                                let step_result = StepResult {
                                    step_name: step_name.clone(),
                                    agent_id: agent_id.to_string(),
                                    agent_name: agent_name.clone(),
                                    output: output.clone(),
                                    input_tokens,
                                    output_tokens,
                                    duration_ms,
                                };
                                if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                    let persisted_index = r.step_results.len();
                                    r.step_results.push(step_result.clone());
                                    self.persist_step_result(run_id, persisted_index, &step_result);
                                    self.persist_run(r);
                                }
                                if let Some(ref var) = fan_step.output_var {
                                    variables.insert(var.clone(), output.clone());
                                }
                                all_outputs.push(output.clone());
                                current_input = output;
                            }
                            Ok(Err(e)) => {
                                let error_msg =
                                    format!("FanOut step '{}' failed: {}", step_name, e);
                                warn!(%error_msg);
                                if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                    r.state = WorkflowRunState::Failed;
                                    r.error = Some(error_msg.clone());
                                    r.completed_at = Some(Utc::now());
                                    self.persist_run(r);
                                }
                                return Err(error_msg);
                            }
                            Err(_) => {
                                let error_msg = format!(
                                    "FanOut step '{}' timed out after {}s",
                                    step_name, fan_step.timeout_secs
                                );
                                warn!(%error_msg);
                                if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                    r.state = WorkflowRunState::Failed;
                                    r.error = Some(error_msg.clone());
                                    r.completed_at = Some(Utc::now());
                                    self.persist_run(r);
                                }
                                return Err(error_msg);
                            }
                        }
                    }

                    info!(
                        count = fan_out_steps.len(),
                        duration_ms, "FanOut steps completed"
                    );

                    // Skip past the fan-out steps we just processed
                    i = j;
                    continue;
                }

                StepMode::Collect => {
                    current_input = all_outputs.join("\n\n---\n\n");
                    all_outputs.clear();
                    all_outputs.push(current_input.clone());
                    if let Some(ref var) = step.output_var {
                        variables.insert(var.clone(), current_input.clone());
                    }
                }

                StepMode::Conditional { condition } => {
                    let prev_lower = current_input.to_lowercase();
                    let cond_lower = condition.to_lowercase();

                    if !prev_lower.contains(&cond_lower) {
                        info!(
                            step = i + 1,
                            name = %step.name,
                            condition,
                            "Conditional step skipped (condition not met)"
                        );
                        i += 1;
                        continue;
                    }

                    // Condition met — execute like sequential
                    let (agent_id, agent_name) = agent_resolver(&step.agent)
                        .ok_or_else(|| format!("Agent not found for step '{}'", step.name))?;

                    let prompt =
                        Self::expand_variables(&step.prompt_template, &current_input, &variables);

                    let start = std::time::Instant::now();
                    let result =
                        Self::execute_step_with_error_mode(step, agent_id, prompt, &send_message)
                            .await;
                    let duration_ms = start.elapsed().as_millis() as u64;

                    match result {
                        Ok(Some((output, input_tokens, output_tokens))) => {
                            let step_result = StepResult {
                                step_name: step.name.clone(),
                                agent_id: agent_id.to_string(),
                                agent_name,
                                output: output.clone(),
                                input_tokens,
                                output_tokens,
                                duration_ms,
                            };
                            if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                let persisted_index = r.step_results.len();
                                r.step_results.push(step_result.clone());
                                self.persist_step_result(run_id, persisted_index, &step_result);
                                self.persist_run(r);
                            }
                            if let Some(ref var) = step.output_var {
                                variables.insert(var.clone(), output.clone());
                            }
                            all_outputs.push(output.clone());
                            current_input = output;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                r.state = WorkflowRunState::Failed;
                                r.error = Some(e.clone());
                                r.completed_at = Some(Utc::now());
                                self.persist_run(r);
                            }
                            return Err(e);
                        }
                    }
                }

                StepMode::Loop {
                    max_iterations,
                    until,
                } => {
                    let (agent_id, agent_name) = agent_resolver(&step.agent)
                        .ok_or_else(|| format!("Agent not found for step '{}'", step.name))?;

                    let until_lower = until.to_lowercase();

                    for loop_iter in 0..*max_iterations {
                        let prompt = Self::expand_variables(
                            &step.prompt_template,
                            &current_input,
                            &variables,
                        );

                        let start = std::time::Instant::now();
                        let result = Self::execute_step_with_error_mode(
                            step,
                            agent_id,
                            prompt,
                            &send_message,
                        )
                        .await;
                        let duration_ms = start.elapsed().as_millis() as u64;

                        match result {
                            Ok(Some((output, input_tokens, output_tokens))) => {
                                let step_result = StepResult {
                                    step_name: format!("{} (iter {})", step.name, loop_iter + 1),
                                    agent_id: agent_id.to_string(),
                                    agent_name: agent_name.clone(),
                                    output: output.clone(),
                                    input_tokens,
                                    output_tokens,
                                    duration_ms,
                                };
                                if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                    let persisted_index = r.step_results.len();
                                    r.step_results.push(step_result.clone());
                                    self.persist_step_result(run_id, persisted_index, &step_result);
                                    self.persist_run(r);
                                }

                                current_input = output.clone();

                                if output.to_lowercase().contains(&until_lower) {
                                    info!(
                                        step = i + 1,
                                        name = %step.name,
                                        iterations = loop_iter + 1,
                                        "Loop terminated (until condition met)"
                                    );
                                    break;
                                }

                                if loop_iter + 1 == *max_iterations {
                                    info!(
                                        step = i + 1,
                                        name = %step.name,
                                        "Loop terminated (max iterations reached)"
                                    );
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                if let Some(r) = self.runs.write().await.get_mut(&run_id) {
                                    r.state = WorkflowRunState::Failed;
                                    r.error = Some(e.clone());
                                    r.completed_at = Some(Utc::now());
                                    self.persist_run(r);
                                }
                                return Err(e);
                            }
                        }
                    }

                    if let Some(ref var) = step.output_var {
                        variables.insert(var.clone(), current_input.clone());
                    }
                    all_outputs.push(current_input.clone());
                }
            }

            i += 1;
        }

        // Mark workflow as completed
        let final_output = current_input.clone();
        if let Some(r) = self.runs.write().await.get_mut(&run_id) {
            r.state = WorkflowRunState::Completed;
            r.output = Some(final_output.clone());
            r.completed_at = Some(Utc::now());
            self.persist_run(r);
        }

        info!(run_id = %run_id, "Workflow completed successfully");
        Ok(final_output)
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_workflow() -> Workflow {
        Workflow {
            id: WorkflowId::new(),
            name: "test-pipeline".to_string(),
            description: "A test pipeline".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "analyze".to_string(),
                    agent: StepAgent::ByName {
                        name: "analyst".to_string(),
                    },
                    prompt_template: "Analyze this: {{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 30,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "summarize".to_string(),
                    agent: StepAgent::ByName {
                        name: "writer".to_string(),
                    },
                    prompt_template: "Summarize this analysis: {{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 30,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        }
    }

    fn mock_resolver(agent: &StepAgent) -> Option<(AgentId, String)> {
        let _ = agent;
        Some((AgentId::new(), "mock-agent".to_string()))
    }

    #[tokio::test]
    async fn test_register_workflow() {
        let engine = WorkflowEngine::new();
        let wf = test_workflow();
        let id = engine.register(wf.clone()).await;
        assert_eq!(id, wf.id);

        let retrieved = engine.get_workflow(id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test-pipeline");
    }

    #[tokio::test]
    async fn test_create_run() {
        let engine = WorkflowEngine::new();
        let wf = test_workflow();
        let wf_id = engine.register(wf).await;

        let run_id = engine.create_run(wf_id, "test input".to_string()).await;
        assert!(run_id.is_some());

        let run = engine.get_run(run_id.unwrap()).await.unwrap();
        assert_eq!(run.input, "test input");
        assert!(matches!(run.state, WorkflowRunState::Pending));
    }

    #[tokio::test]
    async fn test_list_workflows() {
        let engine = WorkflowEngine::new();
        let wf = test_workflow();
        engine.register(wf).await;

        let list = engine.list_workflows().await;
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn test_remove_workflow() {
        let engine = WorkflowEngine::new();
        let wf = test_workflow();
        let id = engine.register(wf).await;

        assert!(engine.remove_workflow(id).await);
        assert!(engine.get_workflow(id).await.is_none());
    }

    #[tokio::test]
    async fn test_execute_pipeline() {
        let engine = WorkflowEngine::new();
        let wf = test_workflow();
        let wf_id = engine.register(wf).await;
        let run_id = engine
            .create_run(wf_id, "raw data".to_string())
            .await
            .unwrap();

        let sender = |_id: AgentId, msg: String| async move {
            Ok((format!("Processed: {msg}"), 100u64, 50u64))
        };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.contains("Processed:"));

        let run = engine.get_run(run_id).await.unwrap();
        assert!(matches!(run.state, WorkflowRunState::Completed));
        assert_eq!(run.step_results.len(), 2);
        assert!(run.output.is_some());
    }

    #[tokio::test]
    async fn test_conditional_skip() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "conditional-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "first".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "only-if-error".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "Fix: {{input}}".to_string(),
                    mode: StepMode::Conditional {
                        condition: "ERROR".to_string(),
                    },
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let wf_id = engine.register(wf).await;
        let run_id = engine
            .create_run(wf_id, "all good".to_string())
            .await
            .unwrap();

        let sender =
            |_id: AgentId, msg: String| async move { Ok((format!("OK: {msg}"), 10u64, 5u64)) };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());

        let run = engine.get_run(run_id).await.unwrap();
        // Only 1 step executed (conditional was skipped)
        assert_eq!(run.step_results.len(), 1);
    }

    #[tokio::test]
    async fn test_conditional_executes() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "conditional-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "first".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "only-if-error".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "Fix: {{input}}".to_string(),
                    mode: StepMode::Conditional {
                        condition: "ERROR".to_string(),
                    },
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let wf_id = engine.register(wf).await;
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        // This sender returns output containing "ERROR"
        let sender = |_id: AgentId, _msg: String| async move {
            Ok(("Found an ERROR in the data".to_string(), 10u64, 5u64))
        };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());

        let run = engine.get_run(run_id).await.unwrap();
        // Both steps executed
        assert_eq!(run.step_results.len(), 2);
    }

    #[tokio::test]
    async fn test_loop_until_condition() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "loop-test".to_string(),
            description: "".to_string(),
            steps: vec![WorkflowStep {
                name: "refine".to_string(),
                agent: StepAgent::ByName {
                    name: "a".to_string(),
                },
                prompt_template: "Refine: {{input}}".to_string(),
                mode: StepMode::Loop {
                    max_iterations: 5,
                    until: "DONE".to_string(),
                },
                timeout_secs: 10,
                error_mode: ErrorMode::Fail,
                output_var: None,
            }],
            created_at: Utc::now(),
        };
        let wf_id = engine.register(wf).await;
        let run_id = engine.create_run(wf_id, "draft".to_string()).await.unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();
        let sender = move |_id: AgentId, _msg: String| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n >= 2 {
                    Ok(("Result: DONE".to_string(), 10u64, 5u64))
                } else {
                    Ok(("Still working...".to_string(), 10u64, 5u64))
                }
            }
        };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("DONE"));
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_loop_max_iterations() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "loop-max-test".to_string(),
            description: "".to_string(),
            steps: vec![WorkflowStep {
                name: "refine".to_string(),
                agent: StepAgent::ByName {
                    name: "a".to_string(),
                },
                prompt_template: "{{input}}".to_string(),
                mode: StepMode::Loop {
                    max_iterations: 3,
                    until: "NEVER_MATCH".to_string(),
                },
                timeout_secs: 10,
                error_mode: ErrorMode::Fail,
                output_var: None,
            }],
            created_at: Utc::now(),
        };
        let wf_id = engine.register(wf).await;
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        let sender = |_id: AgentId, _msg: String| async move {
            Ok(("iteration output".to_string(), 10u64, 5u64))
        };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());

        let run = engine.get_run(run_id).await.unwrap();
        assert_eq!(run.step_results.len(), 3); // max_iterations
    }

    #[tokio::test]
    async fn test_error_mode_skip() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "skip-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "will-fail".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Skip,
                    output_var: None,
                },
                WorkflowStep {
                    name: "succeeds".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let wf_id = engine.register(wf).await;
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();
        let sender = move |_id: AgentId, _msg: String| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 0 {
                    Err("simulated error".to_string())
                } else {
                    Ok(("success".to_string(), 10u64, 5u64))
                }
            }
        };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());

        let run = engine.get_run(run_id).await.unwrap();
        // Only 1 step result (the first was skipped due to error)
        assert_eq!(run.step_results.len(), 1);
        assert!(matches!(run.state, WorkflowRunState::Completed));
    }

    #[tokio::test]
    async fn test_error_mode_retry() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "retry-test".to_string(),
            description: "".to_string(),
            steps: vec![WorkflowStep {
                name: "flaky".to_string(),
                agent: StepAgent::ByName {
                    name: "a".to_string(),
                },
                prompt_template: "{{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 10,
                error_mode: ErrorMode::Retry { max_retries: 2 },
                output_var: None,
            }],
            created_at: Utc::now(),
        };
        let wf_id = engine.register(wf).await;
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();
        let sender = move |_id: AgentId, _msg: String| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n < 2 {
                    Err("transient error".to_string())
                } else {
                    Ok(("finally worked".to_string(), 10u64, 5u64))
                }
            }
        };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "finally worked");
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_output_variables() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "vars-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "produce".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: Some("first_result".to_string()),
                },
                WorkflowStep {
                    name: "transform".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: Some("second_result".to_string()),
                },
                WorkflowStep {
                    name: "combine".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "First: {{first_result}} | Second: {{second_result}}"
                        .to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let wf_id = engine.register(wf).await;
        let run_id = engine.create_run(wf_id, "start".to_string()).await.unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();
        let sender = move |_id: AgentId, msg: String| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                match n {
                    0 => Ok(("alpha".to_string(), 10u64, 5u64)),
                    1 => Ok(("beta".to_string(), 10u64, 5u64)),
                    _ => Ok((format!("Combined: {msg}"), 10u64, 5u64)),
                }
            }
        };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        // The third step receives "First: alpha | Second: beta" as its prompt
        assert!(output.contains("First: alpha"));
        assert!(output.contains("Second: beta"));
    }

    #[tokio::test]
    async fn test_fan_out_parallel() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "fanout-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "task-a".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "Task A: {{input}}".to_string(),
                    mode: StepMode::FanOut,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "task-b".to_string(),
                    agent: StepAgent::ByName {
                        name: "b".to_string(),
                    },
                    prompt_template: "Task B: {{input}}".to_string(),
                    mode: StepMode::FanOut,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "collect".to_string(),
                    agent: StepAgent::ByName {
                        name: "c".to_string(),
                    },
                    prompt_template: "unused".to_string(),
                    mode: StepMode::Collect,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let wf_id = engine.register(wf).await;
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        let sender =
            |_id: AgentId, msg: String| async move { Ok((format!("Done: {msg}"), 10u64, 5u64)) };

        let result = engine.execute_run(run_id, mock_resolver, sender).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        // Collect step joins all outputs
        assert!(output.contains("Done: Task A"));
        assert!(output.contains("Done: Task B"));
        assert!(output.contains("---"));
    }

    #[tokio::test]
    async fn test_expand_variables() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        vars.insert("task".to_string(), "code review".to_string());

        let result = WorkflowEngine::expand_variables(
            "Hello {{name}}, please do {{task}} on {{input}}",
            "main.rs",
            &vars,
        );
        assert_eq!(result, "Hello Alice, please do code review on main.rs");
    }

    #[tokio::test]
    async fn test_error_mode_serialization() {
        let fail_json = serde_json::to_string(&ErrorMode::Fail).unwrap();
        assert_eq!(fail_json, "\"fail\"");

        let skip_json = serde_json::to_string(&ErrorMode::Skip).unwrap();
        assert_eq!(skip_json, "\"skip\"");

        let retry_json = serde_json::to_string(&ErrorMode::Retry { max_retries: 3 }).unwrap();
        let retry: ErrorMode = serde_json::from_str(&retry_json).unwrap();
        assert!(matches!(retry, ErrorMode::Retry { max_retries: 3 }));
    }

    #[tokio::test]
    async fn test_step_mode_conditional_serialization() {
        let mode = StepMode::Conditional {
            condition: "error".to_string(),
        };
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: StepMode = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, StepMode::Conditional { condition } if condition == "error"));
    }

    #[tokio::test]
    async fn test_step_mode_loop_serialization() {
        let mode = StepMode::Loop {
            max_iterations: 5,
            until: "done".to_string(),
        };
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: StepMode = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, StepMode::Loop { max_iterations: 5, until } if until == "done"));
    }
}
