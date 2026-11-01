import { realpathSync, lstatSync } from "node:fs";
import { resolve, relative, basename, sep } from "node:path";
import { tmpdir } from "node:os";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { setTimeout as delay } from "node:timers/promises";

export function authorizeDesktop(env = process.env) {
  if (
    env.QBIT_E2E_ISOLATED_DESKTOP !== "1" ||
    !(
      env.GITHUB_ACTIONS === "true" &&
      env.RUNNER_ENVIRONMENT === "github-hosted"
    )
  ) {
    throw new Error(
      "BLOCKED_E2E_ENVIRONMENT: requires an explicitly selected GitHub-hosted Windows test desktop",
    );
  }
}

export function validateRoot(root) {
  if (!root) throw new Error("BLOCKED_ISOLATION: missing fixture root");
  const actual = realpathSync(root);
  const parent = realpathSync(tmpdir());
  if (
    resolve(root).toLowerCase() !== actual.toLowerCase() ||
    lstatSync(root).isSymbolicLink() ||
    relative(parent, actual).split(sep).length !== 1 ||
    !basename(actual).startsWith("qbit-hotkeys-e2e-")
  ) {
    throw new Error(
      "BLOCKED_ISOLATION: fixture must be a real direct child of OS temp",
    );
  }
  return actual;
}

export function validateBinary(binary) {
  const expected = realpathSync(resolve("target/e2e/release/qbit-desktop.exe"));
  if (
    !binary ||
    realpathSync(binary).toLowerCase() !== expected.toLowerCase() ||
    resolve(binary).toLowerCase() !== expected.toLowerCase()
  ) {
    throw new Error("BLOCKED_ISOLATION: unknown E2E executable");
  }
  return expected;
}

export function powerShellEnvironment(
  overrides = {},
  environment = process.env,
) {
  // pwsh -> Node -> powershell.exe retains PS7 module paths. Windows PowerShell
  // must construct its own paths to load its signature/security cmdlets.
  return Object.fromEntries(
    Object.entries({ ...environment, ...overrides }).filter(
      ([key]) => key.toUpperCase() !== "PSMODULEPATH",
    ),
  );
}

export async function powershell(script, env = {}) {
  const { stdout } = await promisify(execFile)(
    "powershell",
    ["-NoProfile", "-Command", "$ErrorActionPreference='Stop'; " + script],
    {
      windowsHide: true,
      timeout: 45000,
      env: powerShellEnvironment(env),
    },
  );
  return stdout.trim();
}

export async function assertNoQbit() {
  await powershell(
    `if (@(Get-CimInstance Win32_Process -Filter "name='qbit-desktop.exe' OR name='tauri-driver.exe' OR name='msedgedriver.exe'").Count -ne 0) { throw 'BLOCKED_ISOLATION: Qbit or desktop test driver process already exists' }`,
  );
}

export async function ownedProcess() {
  validateRoot(process.env.QBIT_E2E_DATA_ROOT);
  validateBinary(process.env.QBIT_E2E_BINARY);
  return JSON.parse(
    await powershell(`
    $all = @(Get-CimInstance Win32_Process -Filter "name='qbit-desktop.exe'")
    if ($all.Count -ne 1 -or $all[0].ExecutablePath -ne $env:QBIT_E2E_BINARY) { throw 'BLOCKED_ISOLATION: unknown or ambiguous Qbit process' }
    $p = Get-Process -Id $all[0].ProcessId
    $window = if ($p.MainWindowTitle -eq 'Qbit Toolbox') { $p.MainWindowHandle.ToInt64() } else { 0 }
    @{ id=$p.Id; path=$all[0].ExecutablePath; start=$p.StartTime.ToUniversalTime().ToString('o'); window=$window } | ConvertTo-Json -Compress
  `),
  );
}

export async function waitFor(check, description, timeout = 15000) {
  const deadline = Date.now() + timeout;
  let last;
  do {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) {
      last = error;
    }
    await delay(100);
  } while (Date.now() < deadline);
  throw new Error(`${description} timed out${last ? ": " + last.message : ""}`);
}

export async function waitForExit(identity) {
  await waitFor(
    async () =>
      (await powershell(
        `
    $p = Get-Process -Id ([int]$env:QBIT_E2E_PID) -ErrorAction SilentlyContinue
    if ($null -eq $p) { 'exited' } elseif ($p.StartTime.ToUniversalTime().ToString('o') -ne $env:QBIT_E2E_START) { throw 'PID identity changed' }
  `,
        { QBIT_E2E_PID: String(identity.id), QBIT_E2E_START: identity.start },
      )) === "exited",
    "owned host graceful exit",
  );
}

export async function exclusiveDatabases() {
  validateRoot(process.env.QBIT_E2E_DATA_ROOT);
  return powershell(`
    foreach ($relative in @('data/core.db','data/features/qbit.hotkeys/data.db')) {
      $path = Join-Path $env:QBIT_E2E_DATA_ROOT $relative
      if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw 'Missing owned fixture DB' }
      $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
      $stream.Dispose()
    }
    'exclusive-read-open-ok'
  `);
}
