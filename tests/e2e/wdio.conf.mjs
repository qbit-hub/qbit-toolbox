import { resolve } from "node:path";
import { writeFileSync } from "node:fs";
import { browser } from "@wdio/globals";
import {
  authorizeDesktop,
  validateBinary,
  validateRoot,
} from "./isolation.mjs";
import { reportCase } from "./evidence.mjs";
authorizeDesktop();
validateRoot(process.env.QBIT_E2E_DATA_ROOT);
validateBinary(process.env.QBIT_E2E_BINARY);
if (
  !process.env.QBIT_E2E_DATA_ROOT ||
  !process.env.QBIT_E2E_BINARY ||
  !process.env.QBIT_E2E_DRIVER ||
  !process.env.QBIT_E2E_NATIVE_DRIVER ||
  !process.env.EDGEDRIVER_VERSION ||
  !process.env.WEBVIEW2_BROWSER_EXECUTABLE_FOLDER
)
  throw new Error(
    "Use bun run e2e:desktop to ensure isolated data and binary.",
  );
export const config = {
  afterTest: async (test, _context, { passed, error }) => {
    reportCase(test.title, passed ? "PASS" : "FAIL", error);
    if (!passed) {
      try {
        writeFileSync(
          resolve(process.env.QBIT_E2E_REPORT_DIR, "e2e-failure.html"),
          await browser.getPageSource(),
        );
      } catch {
        /* The last-window-close protocol may already have closed the session. */
      }
    }
  },
  runner: "local",
  specs: [resolve("tests/e2e/hotkeys.e2e.mjs")],
  maxInstances: 1,
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  waitforTimeout: 15000,
  connectionRetryCount: 0,
  connectionRetryTimeout: 30000,
  mochaOpts: { timeout: 180000, bail: true },
  services: [
    [
      "@wdio/tauri-service",
      {
        driverProvider: "external",
        tauriDriverPath: process.env.QBIT_E2E_DRIVER,
        autoInstallTauriDriver: false,
        autoDownloadEdgeDriver: false,
        nativeDriverPath: process.env.QBIT_E2E_NATIVE_DRIVER,
        edgeDriverVersion: process.env.EDGEDRIVER_VERSION,
        captureBackendLogs: false,
        captureFrontendLogs: false,
        windowLabel: "control",
        env: {
          QBIT_E2E_DATA_ROOT: process.env.QBIT_E2E_DATA_ROOT,
          WEBVIEW2_BROWSER_EXECUTABLE_FOLDER:
            process.env.WEBVIEW2_BROWSER_EXECUTABLE_FOLDER,
        },
      },
    ],
  ],
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: process.env.QBIT_E2E_BINARY,
        args: ["--show-control"],
      },
    },
  ],
};
