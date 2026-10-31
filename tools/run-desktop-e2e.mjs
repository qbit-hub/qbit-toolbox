import { mkdtempSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join, dirname } from "node:path";
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import {
  authorizeDesktop,
  assertNoQbit,
  powershell,
  validateBinary,
  validateRoot,
} from "../tests/e2e/isolation.mjs";
import { completeReport, classifyFailure } from "../tests/e2e/evidence.mjs";
import { runOwnedDesktop } from "../tests/e2e/runner-control.mjs";

const reportDir = resolve(
  ".ai-bridge/desktop-lifecycle",
  new Date().toISOString().replace(/[:.]/g, "-"),
);
mkdirSync(reportDir, { recursive: true });
process.env.QBIT_E2E_REPORT_DIR = reportDir;
function artifact(name, value) {
  writeFileSync(join(reportDir, name), JSON.stringify(value, null, 2));
}

async function sample(child) {
  const text = await powershell(
    `
    $all = @(Get-CimInstance Win32_Process)
    $byId = @{}; foreach ($entry in $all) { $byId[[int]$entry.ProcessId] = $entry }
    $ids = [Collections.Generic.HashSet[int]]::new(); [void]$ids.Add([int]$env:QBIT_E2E_RUNNER_PID)
    do { $added = $false; foreach ($p in $all) {
      $parent = $byId[[int]$p.ParentProcessId]
      if ($null -ne $parent -and $p.CreationDate -ge $parent.CreationDate -and $ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added=$true }
    } } while ($added)
    @($all | Where-Object { $ids.Contains([int]$_.ProcessId) -and $_.Name -in @('node.exe','bun.exe','qbit-desktop.exe','tauri-driver.exe','msedgedriver.exe') } | ForEach-Object { @{ id=$_.ProcessId; path=$_.ExecutablePath; start=$_.CreationDate.ToUniversalTime().ToString('o') } }) | ConvertTo-Json -Compress
  `,
    { QBIT_E2E_RUNNER_PID: String(child.pid) },
  );
  const records = JSON.parse(text || "[]");
  return Array.isArray(records) ? records : [records];
}
async function cleanup(identities) {
  artifact("owned-processes.json", identities);
  const text = await powershell(
    `
    $records = $env:QBIT_E2E_OWNED | ConvertFrom-Json
    $reaped = @()
    foreach ($record in ($records | Sort-Object { if ([IO.Path]::GetFileName($_.path) -in @('node.exe','bun.exe')) { 0 } else { 1 } })) {
      $p = Get-CimInstance Win32_Process -Filter ("ProcessId=" + [int]$record.id)
      if ($null -eq $p) { continue }
      if ($p.ExecutablePath -ne $record.path -or $p.CreationDate.ToUniversalTime().ToString('o') -ne $record.start) { throw 'BLOCKED_CLEANUP: process identity mismatch' }
      Stop-Process -Id ([int]$record.id) -Force -ErrorAction Stop
      Wait-Process -Id ([int]$record.id) -Timeout 10 -ErrorAction SilentlyContinue
      if (Get-Process -Id ([int]$record.id) -ErrorAction SilentlyContinue) { throw 'Owned process cleanup did not complete' }
      $reaped += $record
    }
    @{status='owned-cleanup-complete';reaped=@($reaped)} | ConvertTo-Json -Depth 4 -Compress
  `,
    { QBIT_E2E_OWNED: JSON.stringify(identities) },
  );
  const evidence = JSON.parse(text);
  artifact("cleanup.json", evidence);
  try {
    await assertNoQbit();
  } catch (error) {
    throw new Error("BLOCKED_CLEANUP_UNKNOWN_OWNERSHIP: " + error.message, {
      cause: error,
    });
  }
  if (
    evidence.reaped.some(
      (p) => p.path.toLowerCase() === process.env.QBIT_E2E_BINARY.toLowerCase(),
    )
  )
    throw new Error("Owned host leaked beyond graceful teardown");
  return evidence;
}
let failure, result;
try {
  authorizeDesktop();
  if (process.platform !== "win32")
    throw new Error("BLOCKED_E2E_ENVIRONMENT: Windows and WebView2 required");
  await assertNoQbit();
  const driver = resolve(".ai-bridge/e2e-tools/bin/tauri-driver.exe");
  const binary = resolve("target/e2e/release/qbit-desktop.exe");
  if (!existsSync(driver) || !existsSync(binary))
    throw new Error(
      "BLOCKED_CI_SETUP: build isolated E2E binary and install pinned tauri-driver first",
    );
  // Read-only environment evidence is repeated immediately before launch to
  // catch evergreen runtime drift after the CI setup step. No driver downloads.
  let environment;
  try {
    const { stdout } = await promisify(execFile)(
      "powershell",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "tools/desktop-e2e-environment.ps1",
        "-ReportDirectory",
        reportDir,
      ],
      { windowsHide: true, timeout: 45000, env: process.env },
    );
    environment = JSON.parse(stdout);
  } catch (error) {
    throw new Error(
      "BLOCKED_CI_SETUP: runtime/driver diagnostic failed: " + error.message,
      { cause: error },
    );
  }
  if (environment.status !== "PASS")
    throw new Error(
      "BLOCKED_CI_SETUP: runtime/driver version validation failed",
    );
  const root = mkdtempSync(join(tmpdir(), "qbit-hotkeys-e2e-"));
  validateRoot(root);
  validateBinary(binary);
  Object.assign(process.env, {
    QBIT_E2E_DATA_ROOT: root,
    QBIT_E2E_BINARY: binary,
    QBIT_E2E_DRIVER: driver,
    QBIT_E2E_NATIVE_DRIVER: environment.edgeDriver.path,
    EDGEDRIVER_VERSION: environment.edgeDriver.version,
    WEBVIEW2_BROWSER_EXECUTABLE_FOLDER: dirname(environment.runtime.path),
  });
  console.log("Isolated E2E data retained at " + root);
  result = await runOwnedDesktop({
    launch: () =>
      spawn(
        process.execPath,
        [
          resolve("node_modules/@wdio/cli/bin/wdio.js"),
          "run",
          "tests/e2e/wdio.conf.mjs",
        ],
        {
          stdio: ["ignore", "pipe", "pipe"],
          windowsHide: true,
          env: process.env,
        },
      ),
    sample,
    cleanup,
    finalize(error, value) {
      artifact("owned-processes.json", value.owned);
      artifact("runner-outcome.json", {
        ...value,
        error: error ? classifyFailure(error) : null,
      });
      completeReport(error);
    },
  });
  failure = result.failure;
} catch (error) {
  failure = error;
  artifact("preflight.json", classifyFailure(error));
} finally {
  // Setup errors, ENOENT, protocol bootstrap failure, interruption and cleanup
  // exceptions all complete the report; none can silently omit scenarios.
  const allPassed = completeReport(failure);
  const code = !failure && result?.code === 0 && allPassed ? 0 : 1;
  artifact("run.json", {
    status:
      code === 0
        ? "PASS"
        : failure
          ? classifyFailure(failure).status
          : "BLOCKED",
    exitCode: code,
    error: failure
      ? classifyFailure(failure)
      : allPassed
        ? null
        : { errorCategory: "MISSING_CASE" },
    profile: process.env.QBIT_E2E_DATA_ROOT,
    hardwareEvidence: false,
  });
  if (failure) console.error(failure.message);
  process.exitCode = code;
}
