use crate::rlm_dataset::RlmFrame;
use crate::rlm_fanout::FanoutResponse;
use crate::rlm_provenance::ProvenanceLedger;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlmMirrorState {
    pub js_snapshot: Value,
    pub datasets: HashMap<String, RlmFrame>,
    pub provenance: ProvenanceLedger,
    pub last_fanout: Option<FanoutResponse>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Default for RlmMirrorState {
    fn default() -> Self {
        Self {
            js_snapshot: serde_json::json!({}),
            datasets: HashMap::new(),
            provenance: ProvenanceLedger::default(),
            last_fanout: None,
            updated_at: chrono::Utc::now(),
        }
    }
}

impl RlmMirrorState {
    pub fn upsert_dataset(&mut self, frame: RlmFrame) {
        let row_count = frame.rows.len();
        let dataset_id = frame.dataset_id.clone();
        let source_id = frame.source_id.clone();
        let query_id = frame.query_id.clone();

        self.datasets.insert(dataset_id.clone(), frame);
        if row_count > 0 {
            self.provenance
                .register_span(&dataset_id, &source_id, &query_id, 1, row_count);
        }
        self.updated_at = chrono::Utc::now();
    }

    pub fn set_snapshot(&mut self, snapshot: Value) {
        self.js_snapshot = snapshot;
        self.updated_at = chrono::Utc::now();
    }

    pub fn set_fanout(&mut self, fanout: FanoutResponse) {
        self.last_fanout = Some(fanout);
        self.updated_at = chrono::Utc::now();
    }
}

pub fn session_memory_key(agent_id: &str, session_id: &str) -> String {
    format!("shared.rlm.{agent_id}.{session_id}")
}
