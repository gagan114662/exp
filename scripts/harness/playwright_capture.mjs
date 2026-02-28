#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

function writeJson(outPath, payload) {
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
}

const resultOut = process.env.PLAYWRIGHT_RESULT_OUT || "artifacts/playwright-evidence.json";
const manifestPath = process.env.PLAYWRIGHT_MANIFEST || "artifacts/browser-evidence-manifest.json";
const evidenceDir = process.env.PLAYWRIGHT_EVIDENCE_DIR || "artifacts/pr-review/evidence";
const baseUrl = (process.env.OPENFANG_UI_BASE_URL || "").trim();
const uiImpact = (process.env.OPENFANG_UI_IMPACT || "false").toLowerCase() === "true";
const routes = (process.env.OPENFANG_PLAYWRIGHT_ROUTES || "/app,/")
  .split(",")
  .map((item) => item.trim())
  .filter(Boolean);

const response = {
  ok: true,
  skipped: false,
  flows: [],
  assertions: [],
  artifacts: [],
};

if (!uiImpact) {
  response.skipped = true;
  response.assertions.push({
    name: "playwright_capture",
    status: "pass",
    details: "UI-impact not detected; playwright capture skipped",
  });
  writeJson(resultOut, response);
  process.exit(0);
}

if (!baseUrl) {
  response.skipped = true;
  response.assertions.push({
    name: "playwright_capture",
    status: "fail",
    details: "OPENFANG_UI_BASE_URL is missing for UI-impacting change",
  });
  writeJson(resultOut, response);
  process.exit(0);
}

let chromium;
try {
  ({ chromium } = await import("playwright"));
} catch {
  response.skipped = true;
  response.assertions.push({
    name: "playwright_capture",
    status: "fail",
    details: "playwright dependency is unavailable",
  });
  writeJson(resultOut, response);
  process.exit(0);
}

try {
  fs.mkdirSync(evidenceDir, { recursive: true });
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    recordVideo: { dir: evidenceDir, size: { width: 1280, height: 720 } },
    viewport: { width: 1280, height: 720 },
  });

  let firstPage = null;
  for (let i = 0; i < routes.length; i += 1) {
    const route = routes[i];
    const page = await context.newPage();
    if (!firstPage) firstPage = page;
    const url = new URL(route, baseUrl).toString();
    await page.goto(url, { waitUntil: "networkidle", timeout: 45000 });
    const screenshotName = `ui-route-${String(i + 1).padStart(2, "0")}.png`;
    const screenshotPath = path.join(evidenceDir, screenshotName);
    await page.screenshot({ path: screenshotPath, fullPage: true });
    response.artifacts.push({
      kind: "screenshot",
      path: path.posix.relative(path.dirname(manifestPath), screenshotPath.split(path.sep).join(path.posix.sep)),
    });
    response.flows.push(`playwright:${route}`);
  }

  await context.close();
  let movedVideo = false;
  if (firstPage && firstPage.video()) {
    const sourceVideoPath = await firstPage.video().path();
    const targetVideoPath = path.join(evidenceDir, "ui-playwright.mp4");
    fs.copyFileSync(sourceVideoPath, targetVideoPath);
    response.artifacts.push({
      kind: "video",
      path: path.posix.relative(path.dirname(manifestPath), targetVideoPath.split(path.sep).join(path.posix.sep)),
    });
    movedVideo = true;
  }
  await browser.close();

  response.assertions.push({
    name: "playwright_capture",
    status: movedVideo ? "pass" : "fail",
    details: movedVideo
      ? "Playwright screenshots and video captured for UI-impacting change"
      : "Playwright ran but video artifact was not produced",
  });
  writeJson(resultOut, response);
} catch (error) {
  response.ok = false;
  response.assertions.push({
    name: "playwright_capture",
    status: "fail",
    details: `Playwright capture failed: ${String(error)}`,
  });
  writeJson(resultOut, response);
}
