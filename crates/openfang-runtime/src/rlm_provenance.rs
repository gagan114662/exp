use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceRef {
    pub evidence_id: String,
    pub dataset_id: String,
    pub source_id: String,
    pub query_id: String,
    pub row_start: usize,
    pub row_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvenanceLedger {
    pub entries: Vec<EvidenceRef>,
}

impl ProvenanceLedger {
    pub fn register_span(
        &mut self,
        dataset_id: &str,
        source_id: &str,
        query_id: &str,
        row_start: usize,
        row_end: usize,
    ) -> String {
        let evidence_id = format!("evidence:{dataset_id}:{query_id}:r{row_start}-{row_end}");
        self.entries.push(EvidenceRef {
            evidence_id: evidence_id.clone(),
            dataset_id: dataset_id.to_string(),
            source_id: source_id.to_string(),
            query_id: query_id.to_string(),
            row_start,
            row_end,
        });
        evidence_id
    }

    pub fn contains(&self, evidence_id: &str) -> bool {
        self.entries.iter().any(|e| e.evidence_id == evidence_id)
    }

    pub fn validate_ids(&self, evidence_ids: &[String]) -> bool {
        if evidence_ids.is_empty() {
            return false;
        }
        evidence_ids.iter().all(|id| self.contains(id))
    }

    pub fn known_ids(&self) -> HashSet<String> {
        self.entries.iter().map(|e| e.evidence_id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_ids_rejects_missing() {
        let mut ledger = ProvenanceLedger::default();
        let good = ledger.register_span("dataset1", "file:test.csv", "q1", 1, 3);
        assert!(ledger.validate_ids(&[good]));
        assert!(!ledger.validate_ids(&["evidence:dataset1:q1:r999-1000".to_string()]));
        assert!(!ledger.validate_ids(&[]));
    }
}
