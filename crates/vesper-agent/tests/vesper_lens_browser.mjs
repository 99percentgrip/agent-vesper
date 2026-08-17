import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";

const playwrightModule = process.env.PLAYWRIGHT_MODULE;
if (!playwrightModule) {
  throw new Error("Set PLAYWRIGHT_MODULE to the installed playwright package directory");
}
const { chromium } = createRequire(import.meta.url)(playwrightModule);
const fixture = "crates/vesper-agent/tests/fixtures/vesper-lens/index.html";
const helper = spawn("cargo", ["run", "-p", "vesper-agent", "--example", "vesper_lens_fixture", "--", fixture], {
  cwd: process.cwd(),
  stdio: ["ignore", "pipe", "pipe"],
});
const helperExit = new Promise((resolve) => helper.on("exit", resolve));
let helperOutput = "";
helper.stderr.on("data", (chunk) => { helperOutput += chunk; });

function outputLine(prefix) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`Timed out waiting for ${prefix}\n${helperOutput}`)), 30_000);
    let pending = "";
    helper.stdout.on("data", (chunk) => {
      pending += chunk;
      const lines = pending.split(/\r?\n/);
      pending = lines.pop() || "";
      for (const line of lines) {
        helperOutput += `${line}\n`;
        if (line.startsWith(prefix)) {
          clearTimeout(timer);
          resolve(line.slice(prefix.length));
        }
      }
    });
    helper.on("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`VesperLens helper exited ${code}\n${helperOutput}`));
    });
  });
}

const urlPromise = outputLine("VESPER_LENS_URL=");
const feedbackPromise = outputLine("VESPER_LENS_FEEDBACK=");
const url = await urlPromise;
const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROME_PATH || "/usr/bin/google-chrome" });
const page = await browser.newPage();
const consoleErrors = [];
const failedRequests = [];
const badResponses = [];
page.on("console", (message) => { if (message.type() === "error") consoleErrors.push(message.text()); });
page.on("requestfailed", (request) => failedRequests.push(`${request.url()}: ${request.failure()?.errorText}`));
page.on("response", (response) => { if (response.status() >= 400) badResponses.push(`${response.status()} ${response.url()}`); });

try {
  await page.goto(url, { waitUntil: "networkidle" });
  const artifact = page.frames().find((frame) => frame.url().includes("/artifact/index.html"));
  assert.ok(artifact, "sandboxed artifact frame must load");
  const sandbox = await page.locator("#artifact").getAttribute("sandbox");
  assert.equal(sandbox, "allow-scripts allow-forms allow-popups allow-downloads");

  await artifact.locator("#fixture-button").click();
  await assert.doesNotReject(() => artifact.locator("#fixture-button").filter({ hasText: "Control works" }).waitFor());
  assert.equal(await artifact.locator("body").evaluate((element) => getComputedStyle(element).backgroundColor), "rgb(16, 24, 39)");

  await page.locator("#annotate").click();
  await artifact.locator("#fixture-title").click({ position: { x: 20, y: 15 } });
  await page.locator(".annotation").waitFor();
  await page.locator(".annotation .comment").fill("Tighten this heading");
  await page.locator(".annotation .suggested").fill("<h1>Fleet status</h1>");
  await page.locator("#approve").click();
  await page.locator("#status").filter({ hasText: "Feedback delivered" }).waitFor();

  assert.deepEqual(consoleErrors, []);
  assert.deepEqual(failedRequests, []);
  assert.deepEqual(badResponses, []);
} finally {
  await browser.close();
}

const feedback = JSON.parse(await feedbackPromise);
assert.equal(feedback.action, "approve");
assert.equal(feedback.annotations.length, 1);
assert.equal(feedback.annotations[0].target.type, "element");
assert.equal(feedback.annotations[0].suggested_html, "<h1>Fleet status</h1>");
assert.equal(await helperExit, 0);
console.log("VesperLens browser E2E passed");
