use openfang_types::config::RlmConfig;
use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DatasetProfile {
    pub row_count: usize,
    pub column_count: usize,
    pub numeric_columns: Vec<String>,
    pub null_cells: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RlmFrame {
    pub dataset_id: String,
    pub source_id: String,
    pub query_id: String,
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
    pub profile: DatasetProfile,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatasetLoadRequest {
    pub dataset_id: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub path: Option<String>,
    pub format: Option<String>,
    pub query: Option<String>,
    pub connection: Option<String>,
    pub sanitize: Option<bool>,
}

fn default_kind() -> String {
    "file".to_string()
}

pub async fn load_dataset(
    req: &DatasetLoadRequest,
    cfg: &RlmConfig,
    workspace_root: Option<&Path>,
) -> Result<RlmFrame, String> {
    let dataset_id = req
        .dataset_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("dataset_{}", chrono::Utc::now().timestamp_millis()));

    let mut frame = match req.kind.as_str() {
        "file" => load_file_dataset(&dataset_id, req, cfg, workspace_root).await?,
        "sqlite" => load_sqlite_dataset(&dataset_id, req, cfg, workspace_root).await?,
        "postgres" => load_postgres_dataset(&dataset_id, req, cfg).await?,
        other => return Err(format!("Unsupported dataset kind: {other}")),
    };

    let should_sanitize = req.sanitize.unwrap_or(cfg.pii_sanitize_default);
    if should_sanitize {
        sanitize_frame(&mut frame);
    }

    frame.profile = profile_frame(&frame);
    Ok(frame)
}

fn resolve_dataset_path(raw_path: &str, workspace_root: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(root) = workspace_root {
        crate::workspace_sandbox::resolve_sandbox_path(raw_path, root)
    } else {
        for component in Path::new(raw_path).components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err("Path traversal denied for dataset path".to_string());
            }
        }
        Ok(PathBuf::from(raw_path))
    }
}

async fn load_file_dataset(
    dataset_id: &str,
    req: &DatasetLoadRequest,
    cfg: &RlmConfig,
    workspace_root: Option<&Path>,
) -> Result<RlmFrame, String> {
    let raw_path = req
        .path
        .as_deref()
        .ok_or("Missing 'path' for file dataset")?;
    let path = resolve_dataset_path(raw_path, workspace_root)?;
    let source_id = format!("file:{}", path.display());
    let query_id = "load".to_string();

    let mut format = req
        .format
        .clone()
        .unwrap_or_else(|| infer_format_from_path(&path))
        .to_lowercase();
    if format.is_empty() {
        format = "csv".to_string();
    }

    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read dataset file: {e}"))?;

    let max_rows = cfg.max_rows_in_memory;
    let (columns, rows) = match format.as_str() {
        "csv" => parse_delimited(&text, ',', dataset_id, max_rows)?,
        "tsv" => parse_delimited(&text, '\t', dataset_id, max_rows)?,
        "json" => parse_json(&text, dataset_id, max_rows)?,
        "jsonl" => parse_jsonl(&text, dataset_id, max_rows)?,
        other => return Err(format!("Unsupported file format: {other}")),
    };

    Ok(RlmFrame {
        dataset_id: dataset_id.to_string(),
        source_id,
        query_id,
        columns,
        rows,
        profile: DatasetProfile::default(),
    })
}

fn infer_format_from_path(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase()
}

fn parse_delimited(
    text: &str,
    delimiter: char,
    dataset_id: &str,
    max_rows: usize,
) -> Result<(Vec<String>, Vec<Value>), String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines.next().ok_or("Delimited dataset is empty")?;
    let headers = parse_delimited_line(header_line, delimiter)
        .into_iter()
        .map(|h| h.trim().to_string())
        .collect::<Vec<_>>();

    let mut rows = Vec::new();
    for (idx, line) in lines.enumerate() {
        if idx >= max_rows {
            break;
        }
        let values = parse_delimited_line(line, delimiter);
        let mut obj = Map::new();
        obj.insert(
            "_row_id".to_string(),
            Value::String(format!("{dataset_id}:{}", idx + 1)),
        );
        for (i, h) in headers.iter().enumerate() {
            let cell = values.get(i).cloned().unwrap_or_default();
            obj.insert(h.clone(), parse_scalar(&cell));
        }
        rows.push(Value::Object(obj));
    }

    Ok((headers, rows))
}

fn parse_delimited_line(line: &str, delimiter: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    buf.push('"');
                    let _ = chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            c if c == delimiter && !in_quotes => {
                out.push(buf.clone());
                buf.clear();
            }
            _ => buf.push(ch),
        }
    }
    out.push(buf);
    out
}

fn parse_json(
    text: &str,
    dataset_id: &str,
    max_rows: usize,
) -> Result<(Vec<String>, Vec<Value>), String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {e}"))?;
    let arr = match value {
        Value::Array(arr) => arr,
        Value::Object(obj) => vec![Value::Object(obj)],
        _ => return Err("JSON dataset must be an array or object".to_string()),
    };

    let mut rows = Vec::new();
    let mut columns = Vec::new();
    for (idx, row) in arr.into_iter().enumerate() {
        if idx >= max_rows {
            break;
        }
        let mut obj = match row {
            Value::Object(obj) => obj,
            other => {
                let mut m = Map::new();
                m.insert("value".to_string(), other);
                m
            }
        };
        obj.insert(
            "_row_id".to_string(),
            Value::String(format!("{dataset_id}:{}", idx + 1)),
        );
        for k in obj.keys() {
            if !columns.iter().any(|c| c == k) {
                columns.push(k.clone());
            }
        }
        rows.push(Value::Object(obj));
    }
    Ok((columns, rows))
}

fn parse_jsonl(
    text: &str,
    dataset_id: &str,
    max_rows: usize,
) -> Result<(Vec<String>, Vec<Value>), String> {
    let mut rows = Vec::new();
    let mut columns = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if idx >= max_rows {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|e| format!("Invalid JSONL at line {}: {e}", idx + 1))?;
        let mut obj = match value {
            Value::Object(obj) => obj,
            other => {
                let mut m = Map::new();
                m.insert("value".to_string(), other);
                m
            }
        };
        obj.insert(
            "_row_id".to_string(),
            Value::String(format!("{dataset_id}:{}", idx + 1)),
        );
        for k in obj.keys() {
            if !columns.iter().any(|c| c == k) {
                columns.push(k.clone());
            }
        }
        rows.push(Value::Object(obj));
    }
    Ok((columns, rows))
}

async fn load_sqlite_dataset(
    dataset_id: &str,
    req: &DatasetLoadRequest,
    cfg: &RlmConfig,
    workspace_root: Option<&Path>,
) -> Result<RlmFrame, String> {
    let raw_path = req
        .path
        .as_deref()
        .ok_or("Missing 'path' for sqlite dataset")?;
    let query = req
        .query
        .as_deref()
        .ok_or("Missing 'query' for sqlite dataset")?;
    let path = resolve_dataset_path(raw_path, workspace_root)?;
    let path_for_thread = path.clone();
    let query = query.to_string();
    let max_rows = cfg.max_rows_in_memory;
    let dataset_id_owned = dataset_id.to_string();

    let (columns, rows) = tokio::task::spawn_blocking(move || {
        use rusqlite::{types::ValueRef, Connection, OpenFlags};

        let conn = Connection::open_with_flags(path_for_thread, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("Failed to open sqlite DB: {e}"))?;
        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| format!("Failed to prepare sqlite query: {e}"))?;
        let col_names = stmt
            .column_names()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        let mut rows = Vec::new();
        let mut q = stmt
            .query([])
            .map_err(|e| format!("Failed to run sqlite query: {e}"))?;
        let mut idx = 0usize;
        while let Some(row) = q.next().map_err(|e| format!("Sqlite row error: {e}"))? {
            if idx >= max_rows {
                break;
            }
            idx += 1;
            let mut obj = Map::new();
            obj.insert(
                "_row_id".to_string(),
                Value::String(format!("{}:{}", dataset_id_owned, idx)),
            );
            for (i, col) in col_names.iter().enumerate() {
                let cell = match row.get_ref(i) {
                    Ok(ValueRef::Null) => Value::Null,
                    Ok(ValueRef::Integer(v)) => json!(v),
                    Ok(ValueRef::Real(v)) => json!(v),
                    Ok(ValueRef::Text(v)) => Value::String(String::from_utf8_lossy(v).to_string()),
                    Ok(ValueRef::Blob(_)) => Value::String("[BLOB]".to_string()),
                    Err(_) => Value::Null,
                };
                obj.insert(col.clone(), cell);
            }
            rows.push(Value::Object(obj));
        }

        Ok::<(Vec<String>, Vec<Value>), String>((col_names, rows))
    })
    .await
    .map_err(|e| format!("Sqlite task join failed: {e}"))??;

    Ok(RlmFrame {
        dataset_id: dataset_id.to_string(),
        source_id: format!("sqlite:{}", path.display()),
        query_id: "q1".to_string(),
        columns,
        rows,
        profile: DatasetProfile::default(),
    })
}

async fn load_postgres_dataset(
    dataset_id: &str,
    req: &DatasetLoadRequest,
    cfg: &RlmConfig,
) -> Result<RlmFrame, String> {
    let query = req
        .query
        .as_deref()
        .ok_or("Missing 'query' for postgres dataset")?;
    let conn_name = req
        .connection
        .as_deref()
        .ok_or("Missing 'connection' for postgres dataset")?;

    let conn_cfg = cfg
        .postgres_connections
        .iter()
        .find(|c| c.dsn_env == conn_name)
        .ok_or_else(|| format!("Unknown postgres connection: {conn_name}"))?;

    enforce_read_only_query(query)?;

    let dsn = std::env::var(&conn_cfg.dsn_env)
        .map_err(|_| format!("Missing env var for Postgres DSN: {}", conn_cfg.dsn_env))?;
    let pg_cfg = dsn
        .parse::<tokio_postgres::Config>()
        .map_err(|e| format!("Invalid Postgres DSN in {}: {e}", conn_cfg.dsn_env))?;

    let rows = if conn_cfg.require_ssl {
        if dsn.to_ascii_lowercase().contains("sslmode=disable") {
            return Err(
                "Postgres connection requires SSL but DSN explicitly disables SSL".to_string(),
            );
        }
        let tls = native_tls::TlsConnector::builder()
            .build()
            .map_err(|e| format!("Failed to build TLS connector: {e}"))?;
        let tls = postgres_native_tls::MakeTlsConnector::new(tls);
        let (client, connection) = pg_cfg
            .connect(tls)
            .await
            .map_err(|e| format!("Postgres connect (TLS) failed: {e}"))?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::warn!(error = %e, "Postgres connection task ended with error");
            }
        });
        run_postgres_query(&client, query, conn_cfg.statement_timeout_ms).await?
    } else {
        let (client, connection) = pg_cfg
            .connect(tokio_postgres::NoTls)
            .await
            .map_err(|e| format!("Postgres connect failed: {e}"))?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::warn!(error = %e, "Postgres connection task ended with error");
            }
        });
        run_postgres_query(&client, query, conn_cfg.statement_timeout_ms).await?
    };

    let columns = rows
        .first()
        .map(|r| {
            r.columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut out_rows = Vec::new();
    for (idx, row) in rows.into_iter().enumerate() {
        if idx >= cfg.max_rows_in_memory {
            break;
        }
        let mut obj = Map::new();
        obj.insert(
            "_row_id".to_string(),
            Value::String(format!("{dataset_id}:{}", idx + 1)),
        );
        for (i, col) in row.columns().iter().enumerate() {
            obj.insert(col.name().to_string(), pg_cell_to_json(&row, i));
        }
        out_rows.push(Value::Object(obj));
    }

    Ok(RlmFrame {
        dataset_id: dataset_id.to_string(),
        source_id: format!("postgres:{}", conn_cfg.dsn_env),
        query_id: "q1".to_string(),
        columns,
        rows: out_rows,
        profile: DatasetProfile::default(),
    })
}

async fn run_postgres_query(
    client: &tokio_postgres::Client,
    query: &str,
    statement_timeout_ms: u64,
) -> Result<Vec<tokio_postgres::Row>, String> {
    client
        .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY")
        .await
        .map_err(|e| format!("Failed to enforce read-only session: {e}"))?;
    client
        .batch_execute(&format!(
            "SET statement_timeout = '{}ms'",
            statement_timeout_ms
        ))
        .await
        .map_err(|e| format!("Failed to set statement_timeout: {e}"))?;
    client
        .query(query, &[])
        .await
        .map_err(|e| format!("Postgres query failed: {e}"))
}

fn enforce_read_only_query(query: &str) -> Result<(), String> {
    let lowered = query.trim().to_ascii_lowercase();
    let forbidden = [
        "insert", "update", "delete", "drop", "alter", "create", "truncate",
    ];
    if forbidden
        .iter()
        .any(|kw| lowered.starts_with(kw) || lowered.contains(&format!(" {kw} ")))
    {
        return Err("Postgres query must be read-only".to_string());
    }
    Ok(())
}

fn pg_cell_to_json(row: &tokio_postgres::Row, idx: usize) -> Value {
    if let Ok(v) = row.try_get::<usize, Option<i64>>(idx) {
        return v.map_or(Value::Null, |x| json!(x));
    }
    if let Ok(v) = row.try_get::<usize, Option<f64>>(idx) {
        return v.map_or(Value::Null, |x| json!(x));
    }
    if let Ok(v) = row.try_get::<usize, Option<bool>>(idx) {
        return v.map_or(Value::Null, |x| json!(x));
    }
    if let Ok(v) = row.try_get::<usize, Option<String>>(idx) {
        return v.map_or(Value::Null, Value::String);
    }
    Value::String("[unsupported_type]".to_string())
}

fn parse_scalar(s: &str) -> Value {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    if let Ok(v) = trimmed.parse::<i64>() {
        return json!(v);
    }
    if let Ok(v) = trimmed.parse::<f64>() {
        return json!(v);
    }
    if matches!(trimmed.to_ascii_lowercase().as_str(), "true" | "false") {
        return json!(trimmed.eq_ignore_ascii_case("true"));
    }
    Value::String(trimmed.to_string())
}

fn profile_frame(frame: &RlmFrame) -> DatasetProfile {
    let mut numeric_counts = std::collections::HashMap::<String, usize>::new();
    let mut null_cells = 0usize;

    for row in &frame.rows {
        if let Value::Object(obj) = row {
            for (k, v) in obj {
                if k == "_row_id" {
                    continue;
                }
                if matches!(v, Value::Null) {
                    null_cells += 1;
                }
                if matches!(v, Value::Number(_)) {
                    *numeric_counts.entry(k.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut numeric_columns = numeric_counts
        .into_iter()
        .filter_map(|(k, c)| if c > 0 { Some(k) } else { None })
        .collect::<Vec<_>>();
    numeric_columns.sort();

    DatasetProfile {
        row_count: frame.rows.len(),
        column_count: frame.columns.len(),
        numeric_columns,
        null_cells,
    }
}

fn email_regex() -> &'static Regex {
    static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
    EMAIL_RE.get_or_init(|| {
        Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
            .expect("email regex must compile")
    })
}

fn phone_regex() -> &'static Regex {
    static PHONE_RE: OnceLock<Regex> = OnceLock::new();
    PHONE_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:\+?1[-.\s]?)?(?:\(?\d{3}\)?[-.\s]?)\d{3}[-.\s]?\d{4}\b")
            .expect("phone regex must compile")
    })
}

fn ssn_regex() -> &'static Regex {
    static SSN_RE: OnceLock<Regex> = OnceLock::new();
    SSN_RE.get_or_init(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("ssn regex must compile"))
}

pub fn sanitize_frame(frame: &mut RlmFrame) {
    for row in &mut frame.rows {
        sanitize_value(row, None);
    }
}

fn sanitize_value(value: &mut Value, key_hint: Option<&str>) {
    match value {
        Value::String(s) => {
            let mut out = s.clone();
            out = email_regex()
                .replace_all(&out, "[REDACTED_EMAIL]")
                .to_string();
            out = phone_regex()
                .replace_all(&out, "[REDACTED_PHONE]")
                .to_string();
            out = ssn_regex().replace_all(&out, "[REDACTED_SSN]").to_string();
            if let Some(key) = key_hint {
                let lowered = key.to_ascii_lowercase();
                if lowered.contains("email")
                    || lowered.contains("phone")
                    || lowered.contains("ssn")
                    || lowered.contains("token")
                    || lowered.contains("secret")
                {
                    out = "[REDACTED_FIELD]".to_string();
                }
            }
            *s = out;
        }
        Value::Array(arr) => {
            for item in arr {
                sanitize_value(item, key_hint);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                sanitize_value(v, Some(k));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pii_sanitizer_masks_patterns_and_sensitive_keys() {
        let mut frame = RlmFrame {
            dataset_id: "d1".to_string(),
            source_id: "file:test.csv".to_string(),
            query_id: "q1".to_string(),
            columns: vec!["email".to_string(), "note".to_string()],
            rows: vec![json!({
                "email": "alice@example.com",
                "note": "Call me at 555-123-4567 and SSN 123-45-6789"
            })],
            profile: DatasetProfile::default(),
        };

        sanitize_frame(&mut frame);
        let row = frame.rows[0].as_object().unwrap();
        assert_eq!(row["email"], "[REDACTED_FIELD]");
        assert_eq!(
            row["note"],
            "Call me at [REDACTED_PHONE] and SSN [REDACTED_SSN]"
        );
    }
}
