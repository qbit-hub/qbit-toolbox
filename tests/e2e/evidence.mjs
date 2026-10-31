import {
  appendFileSync,
  mkdirSync,
  writeFileSync,
  existsSync,
  readFileSync,
} from "node:fs";
import { resolve, join } from "node:path";
export const scenarios = [
  "defaults disabled, creates a safe Disable mapping, reloads and persists",
  "gracefully quits and relaunches the same isolated root with OFF intent and an ON mapping",
  "reopens a destroyed control window without restarting the native host",
];
export function classifyFailure(error) {
  const message = String(error?.message ?? error ?? "");
  const blocked = message.match(/\b(BLOCKED_[A-Z0-9_]+)\b/)?.[1];
  if (blocked) return { status: "BLOCKED", errorCategory: blocked, message };
  if (
    /WEBDRIVER_BOOTSTRAP_FAILURE|session not created|DevToolsActivePort|SessionNotCreated/i.test(
      message,
    )
  )
    return {
      status: "BLOCKED",
      errorCategory: "WEBDRIVER_BOOTSTRAP_FAILURE",
      message,
    };
  if (error?.code === "ENOENT" || /PROCESS_LAUNCH_FAILURE/.test(message))
    return {
      status: "BLOCKED",
      errorCategory: "PROCESS_LAUNCH_FAILURE",
      message,
    };
  if (/PROCESS_INTERRUPTED/.test(message))
    return { status: "BLOCKED", errorCategory: "PROCESS_INTERRUPTED", message };
  return {
    status: "FAIL",
    errorCategory:
      error?.code === "ERR_ASSERTION" ||
      error?.name === "AssertionError" ||
      error?.matcherResult
        ? "ASSERTION_FAILURE"
        : "RUNNER_OR_SCENARIO_FAILURE",
    message,
  };
}
const escape = (s) =>
  String(s ?? "").replace(
    /[<>&"']/g,
    (c) =>
      ({
        "<": "&lt;",
        ">": "&gt;",
        "&": "&amp;",
        '"': "&quot;",
        "'": "&apos;",
      })[c],
  );
export function createEvidenceReporter(dir, profile) {
  dir = resolve(dir);
  mkdirSync(dir, { recursive: true });
  const path = join(dir, "results.json");
  let cases = [],
    events = [],
    runFailure = null;
  function load() {
    if (!existsSync(path)) return;
    const prior = JSON.parse(readFileSync(path, "utf8"));
    if (
      !Array.isArray(prior.cases) ||
      prior.cases.some(
        (c) =>
          !scenarios.includes(c.name) ||
          !["PASS", "FAIL", "BLOCKED"].includes(c.status),
      ) ||
      new Set(prior.cases.map((c) => c.name)).size !== prior.cases.length
    )
      throw new Error("REPORT_INVALID: malformed scenario evidence");
    cases = prior.cases;
    runFailure = prior.runFailure ?? null;
  }
  function save() {
    const runnerOnlyFailure =
      runFailure && cases.every((c) => c.status === "PASS");
    const status =
      runFailure?.status ??
      (cases.length === scenarios.length &&
      cases.every((c) => c.status === "PASS")
        ? "PASS"
        : cases.some((c) => c.status === "FAIL")
          ? "FAIL"
          : "BLOCKED");
    writeFileSync(
      path,
      JSON.stringify(
        {
          schemaVersion: 1,
          tier: "desktop-ui-ipc-sqlite-lifecycle",
          hardwareEvidence: false,
          status,
          runFailure,
          cases,
        },
        null,
        2,
      ),
    );
    writeFileSync(
      join(dir, "junit.xml"),
      `<?xml version="1.0" encoding="UTF-8"?><testsuite name="Hotkeys desktop lifecycle" tests="${cases.length + (runnerOnlyFailure ? 1 : 0)}" failures="${cases.filter((c) => c.status === "FAIL").length + (runnerOnlyFailure && runFailure.status === "FAIL" ? 1 : 0)}" skipped="${cases.filter((c) => c.status === "BLOCKED").length + (runnerOnlyFailure && runFailure.status === "BLOCKED" ? 1 : 0)}">${cases.map((c) => `<testcase name="${escape(c.name)}">${c.status === "BLOCKED" ? `<skipped message="${escape(c.errorCategory + ": " + c.error)}"/>` : c.status === "FAIL" ? `<failure message="${escape(c.error)}"/>` : ""}<system-out>${escape(JSON.stringify(c))}</system-out></testcase>`).join("")}${runnerOnlyFailure ? `<testcase name="Runner lifecycle"><${runFailure.status === "BLOCKED" ? "skipped" : "failure"} message="${escape(runFailure.message)}"/></testcase>` : ""}<system-err>${escape(JSON.stringify(runFailure))}</system-err></testsuite>`,
    );
  }
  return {
    record(kind, evidence) {
      const event = { time: new Date().toISOString(), kind, ...evidence };
      events.push(event);
      appendFileSync(join(dir, "events.jsonl"), JSON.stringify(event) + "\n");
    },
    reportCase(name, result, error) {
      if (
        !scenarios.includes(name) ||
        !["PASS", "FAIL", "BLOCKED"].includes(result)
      )
        throw new Error("REPORT_INVALID: unknown scenario or status");
      load();
      const failure = error
        ? classifyFailure(error)
        : result === "PASS"
          ? null
          : {
              status: result,
              errorCategory:
                result === "BLOCKED" ? "MISSING_CASE" : "ASSERTION_FAILURE",
              message: "Scenario did not pass",
            };
      const next = {
        name,
        status: failure?.status ?? result,
        errorCategory: failure?.errorCategory ?? null,
        error: failure?.message,
        profile,
        events: events.splice(0),
      };
      const index = cases.findIndex((c) => c.name === name);
      if (index < 0) cases.push(next);
      else if (cases[index].status === "PASS" || next.status !== "PASS")
        cases[index] = next;
      save();
    },
    completeReport(error) {
      try {
        load();
      } catch (reportError) {
        cases = [];
        runFailure = classifyFailure(reportError);
      }
      if (error) runFailure = classifyFailure(error);
      for (const name of scenarios)
        if (!cases.some((c) => c.name === name))
          cases.push({
            name,
            status: runFailure?.status ?? "BLOCKED",
            errorCategory: runFailure?.errorCategory ?? "MISSING_CASE",
            error: runFailure?.message ?? "Scenario did not execute",
            profile,
            events: [],
          });
      save();
      return (
        !runFailure &&
        cases.length === scenarios.length &&
        cases.every((c) => c.status === "PASS")
      );
    },
  };
}
let current;
function reporter() {
  const dir = resolve(
    process.env.QBIT_E2E_REPORT_DIR ?? ".ai-bridge/desktop-lifecycle-blocked",
  );
  if (!current || current.dir !== dir)
    current = {
      dir,
      value: createEvidenceReporter(dir, process.env.QBIT_E2E_DATA_ROOT),
    };
  return current.value;
}
export const record = (...args) => reporter().record(...args);
export const reportCase = (...args) => reporter().reportCase(...args);
export const completeReport = (...args) => reporter().completeReport(...args);
