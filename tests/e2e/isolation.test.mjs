import { test } from "node:test";
import assert from "node:assert/strict";
import { authorizeDesktop, validateRoot } from "./isolation.mjs";
import { tmpdir } from "node:os";
import { mkdtempSync, mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { EventEmitter } from "node:events";
import {
  createEvidenceReporter,
  scenarios,
  classifyFailure,
} from "./evidence.mjs";
import { runOwnedDesktop } from "./runner-control.mjs";
test("Windows driver provisioning validators fail closed without native execution", () => {
  const results = JSON.parse(
    execFileSync(
      "powershell",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "tests/e2e/desktop-environment.tests.ps1",
      ],
      { encoding: "utf8", windowsHide: true, timeout: 20000 },
    ),
  );
  assert.equal(results.length, 29);
  assert(results.every((result) => result.status === "PASS"));
});
test("desktop automation fails closed on a normal workstation or self-hosted runner", () => {
  for (const env of [
    {},
    { CI: "true" },
    {
      GITHUB_ACTIONS: "true",
      QBIT_E2E_ISOLATED_DESKTOP: "1",
      RUNNER_ENVIRONMENT: "self-hosted",
    },
  ]) {
    assert.throws(() => authorizeDesktop(env), /BLOCKED_E2E_ENVIRONMENT/);
    try {
      authorizeDesktop(env);
    } catch (error) {
      assert.equal(classifyFailure(error).status, "BLOCKED");
      assert.equal(
        classifyFailure(error).errorCategory,
        "BLOCKED_E2E_ENVIRONMENT",
      );
    }
  }
  assert.doesNotThrow(() =>
    authorizeDesktop({
      GITHUB_ACTIONS: "true",
      RUNNER_ENVIRONMENT: "github-hosted",
      QBIT_E2E_ISOLATED_DESKTOP: "1",
    }),
  );
});
test("fixture root rejects temp itself and missing paths", () => {
  assert.throws(() => validateRoot(tmpdir()), /BLOCKED_ISOLATION/);
  assert.throws(() => validateRoot(undefined), /BLOCKED_ISOLATION/);
});
test("only a direct uniquely named fixture root is accepted", () => {
  const root = mkdtempSync(join(tmpdir(), "qbit-hotkeys-e2e-isolation-test-"));
  assert.equal(validateRoot(root).toLowerCase(), root.toLowerCase());
  const nested = join(root, "qbit-hotkeys-e2e-nested");
  mkdirSync(nested);
  assert.throws(() => validateRoot(nested), /BLOCKED_ISOLATION/);
});

function sqliteFixture(options = {}) {
  const root = mkdtempSync(join(tmpdir(), "qbit-hotkeys-e2e-sqlite-test-"));
  const env = {
    ...process.env,
    QBIT_E2E_DATA_ROOT: root,
    QBIT_SQLITE_FIXTURE: JSON.stringify(options),
  };
  execFileSync(
    "bun",
    [
      "-e",
      `
    const {Database}=require('bun:sqlite'); const {mkdirSync}=require('node:fs'); const {join}=require('node:path');
    const root=process.env.QBIT_E2E_DATA_ROOT, options=JSON.parse(process.env.QBIT_SQLITE_FIXTURE);
    mkdirSync(join(root,'data/features/qbit.hotkeys'),{recursive:true});
    // No enabled affinity/check constraint here: corrupt rows must reach the
    // inspector so its independent validation can be tested.
    const core=new Database(join(root,'data/core.db'));
    core.exec('CREATE TABLE feature_state(feature_id TEXT,enabled)');
    for(const value of (options.desiredRows ?? [0])) core.query('INSERT INTO feature_state VALUES(?,?)').run('qbit.hotkeys',value);
    core.close();
    if(options.featureDb !== false) {
      const db=new Database(join(root,'data/features/qbit.hotkeys/data.db'));
      db.exec('CREATE TABLE mappings(id TEXT,enabled,payload TEXT,sort_order INTEGER)');
      db.query('INSERT INTO mappings VALUES(?,?,?,?)').run('fixture',options.mappingEnabled ?? 1,JSON.stringify({id:options.payloadId ?? 'fixture',enabled:options.payloadEnabled ?? true}),0);
      db.close();
    }
  `,
    ],
    { env, windowsHide: true, timeout: 15000 },
  );
  return {
    root,
    inspect: () =>
      execFileSync("bun", ["tests/e2e/inspect-databases.mjs"], {
        env,
        encoding: "utf8",
        windowsHide: true,
        timeout: 15000,
        stdio: ["ignore", "pipe", "pipe"],
      }),
    hashes: () =>
      [
        "data/core.db",
        ...(options.featureDb === false
          ? []
          : ["data/features/qbit.hotkeys/data.db"]),
      ].map((path) =>
        createHash("sha256")
          .update(readFileSync(join(root, path)))
          .digest("hex"),
      ),
  };
}

test("read-only SQLite evidence preserves an explicit persisted OFF row and DB bytes", () => {
  const fixture = sqliteFixture();
  const before = fixture.hashes();
  const result = JSON.parse(fixture.inspect());
  assert.equal(result.persistedDesired, 0);
  assert.equal(result.effectiveDesired, 0);
  assert.equal(result.configured, true);
  assert.equal(result.mappings[0].enabled, 1);
  assert.equal(result.coreIntegrity, "ok");
  assert.equal(result.hotkeysIntegrity, "ok");
  assert.deepEqual(fixture.hashes(), before);
});

test("a missing feature row remains null and cannot masquerade as persisted OFF", () => {
  const missing = JSON.parse(
    sqliteFixture({ desiredRows: [], featureDb: false }).inspect(),
  );
  assert.equal(missing.persistedDesired, null);
  assert.equal(missing.configured, false);
  assert.equal(missing.effectiveDesired, 0);
  assert.equal(missing.hotkeysDatabasePresent, false);
  assert.deepEqual(missing.mappings, []);
  const on = JSON.parse(sqliteFixture({ desiredRows: [1] }).inspect());
  assert.equal(on.persistedDesired, 1);
  assert.equal(on.effectiveDesired, 1);
  assert.equal(on.configured, true);
});

test("corrupt, ambiguous and unsupported persisted intent is rejected", () => {
  for (const desiredRows of [
    [null],
    [2],
    [-1],
    ["off"],
    ["0"],
    [0.5],
    [0, 0],
    [0, 1],
  ]) {
    const fixture = sqliteFixture({ desiredRows });
    const before = fixture.hashes();
    assert.throws(
      () => fixture.inspect(),
      (error) => String(error.stderr).includes("INVALID_STORED_STATE"),
    );
    assert.deepEqual(fixture.hashes(), before);
  }
});

test("inconsistent mapping enabled state or identity is rejected", () => {
  for (const options of [
    { mappingEnabled: 2 },
    { payloadEnabled: false },
    { payloadId: "other-fixture" },
  ]) {
    const fixture = sqliteFixture(options);
    assert.throws(
      () => fixture.inspect(),
      (error) => String(error.stderr).includes("INVALID_STORED_STATE"),
    );
  }
});

function evidenceFixture() {
  const dir = mkdtempSync(join(tmpdir(), "qbit-hotkeys-e2e-report-test-"));
  return {
    reporter: createEvidenceReporter(dir, "owned-test-fixture"),
    read: () => JSON.parse(readFileSync(join(dir, "results.json"), "utf8")),
    xml: () => readFileSync(join(dir, "junit.xml"), "utf8"),
  };
}

test("incomplete reports retain every scenario as BLOCKED, never PASS", () => {
  const evidence = evidenceFixture();
  evidence.reporter.reportCase(scenarios[0], "PASS");
  assert.equal(evidence.reporter.completeReport(), false);
  assert.deepEqual(
    evidence.read().cases.map((c) => c.status),
    ["PASS", "BLOCKED", "BLOCKED"],
  );
  assert.equal(evidence.read().cases[1].errorCategory, "MISSING_CASE");
  assert.match(evidence.xml(), /skipped="2"/);
});

test("assertion and driver bootstrap failures have distinct report categories", () => {
  const assertion = Object.assign(new Error("fixed assertion failure"), {
    code: "ERR_ASSERTION",
  });
  assert.deepEqual(classifyFailure(assertion), {
    status: "FAIL",
    errorCategory: "ASSERTION_FAILURE",
    message: "fixed assertion failure",
  });
  const evidence = evidenceFixture();
  evidence.reporter.reportCase(
    scenarios[0],
    "FAIL",
    new Error("session not created: DevToolsActivePort file does not exist"),
  );
  evidence.reporter.completeReport(
    new Error("WEBDRIVER_BOOTSTRAP_FAILURE: session not created"),
  );
  assert(
    evidence
      .read()
      .cases.every(
        (c) =>
          c.status === "BLOCKED" &&
          c.errorCategory === "WEBDRIVER_BOOTSTRAP_FAILURE",
      ),
  );
  assert.match(evidence.xml(), /skipped="3"/);
});

test("late cleanup failure prevents an all-PASS run and is visible in JUnit", () => {
  const evidence = evidenceFixture();
  for (const name of scenarios) evidence.reporter.reportCase(name, "PASS");
  assert.equal(
    evidence.reporter.completeReport(
      new Error("BLOCKED_CLEANUP: owned identity uncertain"),
    ),
    false,
  );
  assert.equal(evidence.read().status, "BLOCKED");
  assert.equal(evidence.read().runFailure.errorCategory, "BLOCKED_CLEANUP");
  assert.match(evidence.xml(), /tests="4"/);
  assert.match(evidence.xml(), /skipped="1"/);
});

function fakeChild() {
  const child = new EventEmitter();
  Object.assign(child, {
    pid: 12345,
    exitCode: null,
    signalCode: null,
    stdout: new EventEmitter(),
    stderr: new EventEmitter(),
    kills: 0,
  });
  child.kill = () => {
    child.kills++;
    child.signalCode = "SIGTERM";
    child.emit("exit", null);
  };
  return child;
}
async function fakeRun({
  launch,
  cleanup = async () => {},
  signalSource = new EventEmitter(),
}) {
  const evidence = evidenceFixture();
  let finalized = 0;
  const result = await runOwnedDesktop({
    launch,
    sample: async () => [
      { id: 12346, path: "owned-fixture-driver.exe", start: "fixed-start" },
    ],
    cleanup,
    signalSource,
    finalize(error) {
      finalized++;
      evidence.reporter.completeReport(error);
    },
  });
  assert.equal(finalized, 1);
  assert.equal(result.code, 1);
  assert.equal(evidence.read().cases.length, 3);
  assert(evidence.read().cases.every((c) => c.status !== "PASS"));
  return { result, evidence };
}

test("synchronous child launch failure completes reports without cleanup authority", async () => {
  let cleanups = 0;
  const { evidence } = await fakeRun({
    launch() {
      throw Object.assign(new Error("spawn fixture ENOENT"), {
        code: "ENOENT",
      });
    },
    cleanup: async () => {
      cleanups++;
    },
  });
  assert.equal(cleanups, 0);
  assert(
    evidence
      .read()
      .cases.every((c) => c.errorCategory === "PROCESS_LAUNCH_FAILURE"),
  );
});

test("asynchronous spawn error without a PID also completes reports without cleanup", async () => {
  let cleanups = 0;
  const { evidence } = await fakeRun({
    launch() {
      const child = fakeChild();
      child.pid = undefined;
      queueMicrotask(() =>
        child.emit(
          "error",
          Object.assign(new Error("fixture ENOENT"), { code: "ENOENT" }),
        ),
      );
      return child;
    },
    cleanup: async () => {
      cleanups++;
    },
  });
  assert.equal(cleanups, 0);
  assert(
    evidence
      .read()
      .cases.every((c) => c.errorCategory === "PROCESS_LAUNCH_FAILURE"),
  );
});

test("driver bootstrap exit logs classify environment failure and complete all reports", async () => {
  const { evidence } = await fakeRun({
    launch() {
      const child = fakeChild();
      queueMicrotask(() => {
        child.stderr.emit(
          "data",
          Buffer.from("session not created: DevToolsActivePort\n"),
        );
        child.exitCode = 1;
        child.emit("exit", 1);
      });
      return child;
    },
  });
  assert(
    evidence
      .read()
      .cases.every((c) => c.errorCategory === "WEBDRIVER_BOOTSTRAP_FAILURE"),
  );
});

test("interrupting an owned child completes reports and cleans only recorded identities", async () => {
  const signalSource = new EventEmitter();
  let child, observed;
  const { evidence } = await fakeRun({
    signalSource,
    launch() {
      child = fakeChild();
      setImmediate(() => signalSource.emit("SIGINT"));
      return child;
    },
    cleanup: async (records) => {
      observed = records;
    },
  });
  assert.equal(child.kills, 1);
  assert.deepEqual(observed, [
    { id: 12346, path: "owned-fixture-driver.exe", start: "fixed-start" },
  ]);
  assert(
    evidence
      .read()
      .cases.every((c) => c.errorCategory === "PROCESS_INTERRUPTED"),
  );
});

test("cleanup identity failure cannot bypass report completion or become PASS", async () => {
  const { evidence } = await fakeRun({
    launch() {
      const child = fakeChild();
      setImmediate(() => {
        child.exitCode = 0;
        child.emit("exit", 0);
      });
      return child;
    },
    cleanup: async () => {
      throw new Error("BLOCKED_CLEANUP: fixture identity mismatch");
    },
  });
  assert(
    evidence.read().cases.every((c) => c.errorCategory === "BLOCKED_CLEANUP"),
  );
});
