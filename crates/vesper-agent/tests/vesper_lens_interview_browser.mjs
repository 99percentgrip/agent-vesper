import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";

const playwrightModule = process.env.PLAYWRIGHT_MODULE;
if (!playwrightModule) {
  throw new Error("Set PLAYWRIGHT_MODULE to the installed playwright package directory");
}
const { chromium } = createRequire(import.meta.url)(playwrightModule);
const helper = spawn("cargo", ["run", "-p", "vesper-agent", "--example", "vesper_lens_fixture", "--", "--interview"], {
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
page.on("console", (message) => { if (message.type() === "error") consoleErrors.push(message.text()); });
page.on("requestfailed", (request) => failedRequests.push(`${request.url()}: ${request.failure()?.errorText}`));

try {
  await page.goto(url, { waitUntil: "networkidle" });
  const artifact = page.frames().find((frame) => frame.url().includes("/artifact/index.html"));
  assert.ok(artifact, "interview artifact frame must load");
  await page.locator("#changes").filter({ hasText: "Send answers" }).waitFor();
  assert.equal(await page.locator("#approve").isHidden(), true, "interviews must not expose generic approval");
  assert.equal(await page.locator("#annotate").isHidden(), true, "interviews must not expose annotation mode");

  await page.locator("#changes").click();
  await page.locator("#status").filter({ hasText: "Answer the required questions" }).waitFor();
  await artifact.locator('input[value="Patch"]').check();
  await page.locator("#changes").click();
  await page.locator("#status").filter({ hasText: "Feedback delivered" }).waitFor();

  assert.deepEqual(consoleErrors, []);
  assert.deepEqual(failedRequests, []);
} finally {
  await browser.close();
}

const feedback = JSON.parse(await feedbackPromise);
assert.equal(feedback.action, "modify");
assert.deepEqual(feedback.answers, [{ question: "scope", value: "Patch" }]);
assert.equal(await helperExit, 0);
console.log("VesperLens interview browser E2E passed");
