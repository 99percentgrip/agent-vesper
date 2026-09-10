import assert from "node:assert/strict";
import { createRequire } from "node:module";

const { chromium } = createRequire(import.meta.url)(process.env.PLAYWRIGHT_MODULE);
const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROME_PATH || "/usr/bin/google-chrome" });
try {
  const page = await browser.newPage();
  await page.goto(process.argv[2], { waitUntil: "networkidle" });
  const artifact = page.frames().find(frame => frame.url().includes("/artifact/index.html"));
  assert.ok(artifact);
  await page.locator("#changes").click();
  await page.locator("#status").filter({ hasText: "Answer the required questions" }).waitFor();
  await artifact.locator('input[value="Patch"]').check();
  await page.locator("#notes").fill("native-browser-feedback-472");
  await page.locator("#changes").click();
  await page.locator("#status").filter({ hasText: "Feedback delivered" }).waitFor();
} finally {
  await browser.close();
}
