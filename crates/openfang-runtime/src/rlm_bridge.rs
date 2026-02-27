use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const BRIDGE_SCRIPT: &str = include_str!("../assets/bun_rlm_bridge.mjs");

#[derive(Debug)]
pub struct BunBridge {
    bun_path: String,
    script_path: std::path::PathBuf,
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

#[derive(Debug, Deserialize)]
struct BridgeResponse {
    id: u64,
    ok: bool,
    result: Option<Value>,
    error: Option<String>,
}

impl BunBridge {
    pub async fn start(bun_path: &str) -> Result<Self, String> {
        let script_path = write_bridge_script()?;
        let (child, stdin, stdout) = spawn_child(bun_path, &script_path)?;
        let mut bridge = Self {
            bun_path: bun_path.to_string(),
            script_path,
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        bridge.health().await?;
        Ok(bridge)
    }

    pub async fn health(&mut self) -> Result<(), String> {
        let _ = self.request("health", None, None, None).await?;
        Ok(())
    }

    pub async fn eval(&mut self, code: &str, input: Option<Value>) -> Result<Value, String> {
        self.request("eval", Some(code), input, None).await
    }

    pub async fn snapshot(&mut self) -> Result<Value, String> {
        self.request("snapshot", None, None, None).await
    }

    pub async fn restore(&mut self, snapshot: Value) -> Result<(), String> {
        let _ = self.request("restore", None, None, Some(snapshot)).await?;
        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    pub async fn restart_with_snapshot(&mut self, snapshot: &Value) -> Result<(), String> {
        let _ = self.child.kill().await;
        let (child, stdin, stdout) = spawn_child(&self.bun_path, &self.script_path)?;
        self.child = child;
        self.stdin = stdin;
        self.stdout = stdout;
        self.next_id = 1;
        self.health().await?;
        self.restore(snapshot.clone()).await
    }

    async fn request(
        &mut self,
        command: &str,
        code: Option<&str>,
        input: Option<Value>,
        snapshot: Option<Value>,
    ) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        let payload = json!({
            "id": id,
            "command": command,
            "code": code,
            "input": input,
            "snapshot": snapshot,
        });

        let line = serde_json::to_string(&payload)
            .map_err(|e| format!("Failed to encode bridge payload: {e}"))?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to Bun bridge stdin: {e}"))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Failed to write newline to Bun bridge stdin: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush Bun bridge stdin: {e}"))?;

        loop {
            let mut out = String::new();
            let read = self
                .stdout
                .read_line(&mut out)
                .await
                .map_err(|e| format!("Failed reading Bun bridge stdout: {e}"))?;
            if read == 0 {
                return Err("Bun bridge exited unexpectedly".to_string());
            }
            if out.trim().is_empty() {
                continue;
            }

            let resp: BridgeResponse = serde_json::from_str(out.trim())
                .map_err(|e| format!("Invalid Bun bridge response: {e}"))?;
            if resp.id != id {
                continue;
            }
            if resp.ok {
                return Ok(resp.result.unwrap_or(Value::Null));
            }
            return Err(resp
                .error
                .unwrap_or_else(|| "Bun bridge returned unknown error".to_string()));
        }
    }
}

fn write_bridge_script() -> Result<std::path::PathBuf, String> {
    let path = std::env::temp_dir().join(format!(
        "openfang_bun_rlm_bridge_{}.mjs",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&path, BRIDGE_SCRIPT)
        .map_err(|e| format!("Failed to write Bun bridge script: {e}"))?;
    Ok(path)
}

fn spawn_child(
    bun_path: &str,
    script_path: &std::path::Path,
) -> Result<
    (
        tokio::process::Child,
        tokio::process::ChildStdin,
        tokio::io::BufReader<tokio::process::ChildStdout>,
    ),
    String,
> {
    let mut cmd = tokio::process::Command::new(bun_path);
    cmd.arg(script_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start Bun at '{}': {e}", bun_path))?;
    let stdin = child
        .stdin
        .take()
        .ok_or("Failed to open Bun bridge stdin")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to open Bun bridge stdout")?;

    Ok((child, stdin, BufReader::new(stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn protocol_round_trip_without_bun_is_error() {
        let err = BunBridge::start("bun-does-not-exist").await.unwrap_err();
        assert!(err.contains("Failed to start Bun"));
    }
}
