import readline from "node:readline";

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

let state = {};

function write(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function cloneJson(v) {
  try {
    return JSON.parse(JSON.stringify(v));
  } catch {
    return null;
  }
}

async function handle(cmd) {
  const id = cmd.id ?? 0;
  const op = cmd.command;

  if (op === "health") {
    write({ id, ok: true, result: { status: "ok", ts: Date.now() } });
    return;
  }

  if (op === "restore") {
    if (cmd.snapshot && typeof cmd.snapshot === "object") {
      state = cloneJson(cmd.snapshot) ?? {};
    } else {
      state = {};
    }
    write({ id, ok: true, result: { restored: true } });
    return;
  }

  if (op === "snapshot") {
    write({ id, ok: true, result: cloneJson(state) ?? {} });
    return;
  }

  if (op === "eval") {
    const code = typeof cmd.code === "string" ? cmd.code : "return null;";
    const input = cmd.input;
    try {
      const fn = new Function(
        "state",
        "input",
        `"use strict"; return (async () => {\n${code}\n})();`
      );
      const result = await fn(state, input);
      write({ id, ok: true, result: cloneJson(result) });
      return;
    } catch (error) {
      write({
        id,
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      });
      return;
    }
  }

  write({ id, ok: false, error: `unknown_command:${op}` });
}

rl.on("line", async (line) => {
  const raw = line.trim();
  if (!raw) {
    return;
  }
  let cmd;
  try {
    cmd = JSON.parse(raw);
  } catch {
    write({ id: 0, ok: false, error: "invalid_json" });
    return;
  }
  await handle(cmd);
});
