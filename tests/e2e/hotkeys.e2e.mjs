import { browser, $, expect } from "@wdio/globals";
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { Key } from "webdriverio";
import {
  ownedProcess,
  powershell,
  waitForExit,
  exclusiveDatabases,
  waitFor,
} from "./isolation.mjs";
import { record, scenarios } from "./evidence.mjs";

async function databases() {
  const { stdout } = await promisify(execFile)(
    "bun",
    ["tests/e2e/inspect-databases.mjs"],
    { env: process.env, windowsHide: true, timeout: 20000 },
  );
  const value = JSON.parse(stdout);
  assert.equal(value.coreIntegrity, "ok");
  if (value.hotkeysDatabasePresent) assert.equal(value.hotkeysIntegrity, "ok");
  record("sqlite", value);
  return value;
}

async function persisted(desired, enabled, count = 1) {
  return waitFor(async () => {
    const db = await databases();
    return db.configured === true &&
      db.persistedDesired === desired &&
      db.effectiveDesired === desired &&
      db.hotkeysDatabasePresent === true &&
      db.mappings.length === count &&
      (count === 0 || db.mappings[0].enabled === enabled)
      ? db
      : false;
  }, `persisted desired=${desired}, mapping enabled=${enabled}, count=${count}`);
}

async function mappingCount(count) {
  await expect($("p*=Active mappings:")).toHaveText(
    expect.stringContaining(`Active mappings: ${count} ·`),
  );
}

async function captureTrigger(key) {
  await $("button[aria-label='Capture trigger']").click();
  await browser.keys([Key.Control, Key.Shift, key]);
  await browser.keys("\uE000");
}

async function addDisableMapping() {
  await $("button=Add mapping").click();
  await captureTrigger(Key.F12);
  await $("[role='combobox']").click();
  await $("li=Disable shortcut").click();
  await $("button=Review conflicts").click();
}

async function quitOwned() {
  const before = await ownedProcess();
  const exit = await powershell(
    `
    $p = Get-Process -Id ([int]$env:QBIT_E2E_PID)
    if ($p.StartTime.ToUniversalTime().ToString('o') -ne $env:QBIT_E2E_START -or $p.Path -ne $env:QBIT_E2E_BINARY) { throw 'BLOCKED_ISOLATION: quit identity mismatch' }
    $null = $p.Handle
    $helper = Start-Process -FilePath $env:QBIT_E2E_BINARY -ArgumentList '--e2e-quit' -WindowStyle Hidden -PassThru
    try {
      if (-not $helper.WaitForExit(10000)) { throw 'Owned quit helper timed out' }
      if ($helper.ExitCode -ne 0) { throw 'Owned quit helper failed' }
      if (-not $p.WaitForExit(15000)) { throw 'Owned host graceful quit timed out' }
      $p.ExitCode
    } finally { if (-not $helper.HasExited) { $helper.Kill() }; $helper.Dispose(); $p.Dispose() }
  `,
    { QBIT_E2E_PID: String(before.id), QBIT_E2E_START: before.start },
  );
  assert.equal(exit, "0", "actual native host must exit with code zero");
  await waitForExit(before);
  const handles = await exclusiveDatabases();
  record("graceful-quit", { before, exitCode: Number(exit), handles });
  return before;
}

// The external Edge driver terminates its session when the last WebView is
// destroyed. Verify the native close/reopen boundary against the SAME isolated
// app process instead of asking a terminated WebDriver session for new handles.
async function isolatedProcess() {
  return ownedProcess();
}

async function feature() {
  await browser.switchToWindow((await browser.getWindowHandles())[0]);
  await $("a[href='/features/hotkeys']").waitForDisplayed();
  await $("a[href='/features/hotkeys']").click();
  await $("h2=Hotkeys").waitForDisplayed();
}
async function runtime(text) {
  await expect($("p*=Runtime:")).toHaveText(expect.stringContaining(text));
}
describe("Windows Hotkeys through real UI / IPC / SQLite", () => {
  it(scenarios[0], async () => {
    await feature();
    // A real theme change must activate newly generated Emotion styles under
    // the bundled Tauri CSP, rather than only changing preference state.
    await $("button[value='light']").click();
    const background = () =>
      browser.execute(
        () =>
          getComputedStyle(document.querySelector("main").parentElement)
            .backgroundColor,
      );
    const light = await background();
    await $("button[value='dark']").click();
    await browser.waitUntil(async () => (await background()) !== light, {
      timeoutMsg: "Theme styles did not activate under Tauri CSP",
    });
    await $("button[value='light']").click();
    const enabled = $("input[role='switch']");
    await expect(enabled).not.toBeSelected();
    await runtime("Disabled");
    const fresh = await databases();
    assert.equal(fresh.configured, false);
    assert.equal(fresh.persistedDesired, null);
    assert.equal(fresh.effectiveDesired, 0);
    assert.deepEqual(fresh.mappings, []);
    await $("label*=Enable global Hotkeys").click();
    await runtime("Running");
    // Focused capture in this test WebView only; no OS SendInput or arbitrary
    // foreground-app remap test. The saved action only disables Ctrl+Shift+F12.
    await addDisableMapping();
    await expect($("[role='dialog'] [role='alert']")).toHaveText(
      "No blocking conflicts. Ready to save.",
    );
    await $("button=Save").click();
    await $("ul[aria-label='Global mappings'] li").waitForDisplayed();
    await mappingCount(1);
    await persisted(1, 1);
    const host = await ownedProcess();
    await browser.refresh();
    await $("ul[aria-label='Global mappings'] li").waitForDisplayed();
    await runtime("Running");
    await $("ul[aria-label='Global mappings'] input[role='switch']").click();
    await expect(
      $("ul[aria-label='Global mappings'] input[role='switch']"),
    ).not.toBeSelected();
    await mappingCount(0);
    await persisted(1, 0);
    await browser.refresh();
    await runtime("Running");
    await expect(
      $("ul[aria-label='Global mappings'] input[role='switch']"),
    ).not.toBeSelected();
    await $("ul[aria-label='Global mappings'] input[role='switch']").click();
    await mappingCount(1);
    await persisted(1, 1);
    assert.equal(
      (await ownedProcess()).id,
      host.id,
      "row changes publish without host restart",
    );
    // Disable before capturing the same trigger again; the saved Disable
    // mapping must never consume the driver's own capture keystrokes.
    await $("label*=Enable global Hotkeys").click();
    await runtime("Disabled");
    const beforeEdit = await persisted(0, 1);
    await addDisableMapping();
    await expect($("[role='dialog'] button=Save")).toBeDisabled();
    await expect($("[role='dialog'] [role='alert']")).toHaveText(
      "An enabled mapping already uses this trigger.",
    );
    await $("[role='dialog'] button=Cancel").click();
    assert.deepEqual(
      (await persisted(0, 1)).mappings,
      beforeEdit.mappings,
      "rejected duplicate must not be persisted",
    );
    await $("button=Edit").click();
    await captureTrigger(Key.F11);
    await $("[role='dialog'] input[maxlength='120']").setValue(
      "Lifecycle fixture edited",
    );
    await $("button=Review conflicts").click();
    await expect($("[role='dialog'] [role='alert']")).toHaveText(
      "No blocking conflicts. Ready to save.",
    );
    await $("[role='dialog'] button=Save").click();
    await browser.refresh();
    await runtime("Disabled");
    await expect($("ul[aria-label='Global mappings']")).toHaveText(
      expect.stringContaining("Lifecycle fixture edited"),
    );
    const edited = await persisted(0, 1);
    assert.equal(
      edited.mappings[0].payload.metadata.label,
      "Lifecycle fixture edited",
    );
    assert.equal(
      edited.mappings[0].id,
      beforeEdit.mappings[0].id,
      "edit preserves mapping identity",
    );
    assert.deepEqual(edited.mappings[0].payload.trigger, {
      kind: "chord",
      chord: {
        modifiers: ["ctrl", "shift"],
        primary: { kind: "logical", token: "F11" },
      },
    });
    assert.deepEqual(edited.mappings[0].payload.action, { kind: "disable" });
    await expect($("ul[aria-label='Global mappings']")).toHaveText(
      expect.stringContaining("Ctrl + Shift + F11"),
    );
    await expect($("ul[aria-label='Global mappings']")).not.toHaveText(
      expect.stringContaining("Ctrl + Shift + F12"),
    );
    await $("label*=Enable global Hotkeys").click();
    await runtime("Running");
    await mappingCount(1);
    const publishedEdit = await persisted(1, 1);
    assert.deepEqual(publishedEdit.mappings, edited.mappings);
    assert.equal(
      (await ownedProcess()).id,
      host.id,
      "trigger edit publishes in the same host",
    );
    // Native old-trigger/new-trigger dispatch is proved by the existing Rust
    // service acceptance test; no WebDriver or OS input is used to infer it.
    await $("label*=Enable global Hotkeys").click();
    await runtime("Disabled");
    await persisted(0, 1);
    await $("button=Delete").click();
    await $("[role='dialog']").$("button=Delete").click();
    await $("p=No mappings yet.").waitForDisplayed();
    await persisted(0, 0, 0);
  });
  it(scenarios[1], async () => {
    await addDisableMapping();
    await $("[role='dialog'] button=Save").click();
    const saved = await persisted(0, 1);
    // Two distinct graceful exits and launches; reloadSession is only the
    // transport for a new host after OS exit and DB handle release are proven.
    for (let cycle = 0; cycle < 2; cycle++) {
      const before = await quitOwned();
      const disk = await databases();
      assert.equal(disk.configured, true);
      assert.equal(
        disk.persistedDesired,
        0,
        "Quit must retain the explicit OFF row",
      );
      assert.deepEqual(disk.mappings, saved.mappings);
      try {
        await browser.reloadSession();
      } catch (error) {
        record("restart-session-failure", {
          cycle,
          before,
          errorCategory: "WEBDRIVER_BOOTSTRAP_FAILURE",
          message: error.message,
        });
        throw new Error(
          `WEBDRIVER_BOOTSTRAP_FAILURE: reloadSession after native exit failed: ${error.message}`,
          { cause: error },
        );
      }
      await feature();
      const after = await ownedProcess();
      assert.notEqual(after.id, before.id, "new OS host PID is required");
      assert.notEqual(after.start, before.start);
      await runtime("Disabled");
      await expect(
        $("ul[aria-label='Global mappings'] input[role='switch']"),
      ).toBeSelected();
      await mappingCount(0);
      const reloaded = await persisted(0, 1);
      assert.deepEqual(reloaded.mappings, saved.mappings);
      record("restart", {
        cycle,
        before,
        after,
        root: reloaded.root,
        persistedDesired: reloaded.persistedDesired,
        configured: reloaded.configured,
        mappingEnabled: 1,
      });
    }
  });
  it(scenarios[2], async () => {
    // Edge terminates its app after closing the last WebView. End that session,
    // then test real WM_CLOSE/second-instance activation outside Edge's job.
    const saved = await persisted(0, 1);
    await quitOwned();
    await browser.deleteSession().catch(() => {}); // the host already exited
    const app = spawn(process.env.QBIT_E2E_BINARY, ["--show-control"], {
      windowsHide: true,
      env: process.env,
      stdio: "ignore",
    });
    async function awaitWindow(wanted) {
      const deadline = Date.now() + 15000;
      do {
        try {
          const value = await isolatedProcess();
          if ((value.window !== 0) === wanted) return value;
        } catch {
          /* bounded startup */
        }
        await delay(100);
      } while (Date.now() < deadline);
      throw new Error("Native E2E process/window transition timed out");
    }
    try {
      const before = await awaitWindow(true);
      assert.equal(before.id, app.pid);
      await promisify(execFile)(
        "powershell",
        [
          "-NoProfile",
          "-Command",
          "$appProcess = Get-Process -Id ([int]$env:QBIT_E2E_PID); if (-not $appProcess.CloseMainWindow()) { throw 'Window close rejected' }",
        ],
        {
          windowsHide: true,
          env: { ...process.env, QBIT_E2E_PID: String(app.pid) },
        },
      );
      const closed = await awaitWindow(false);
      assert.equal(closed.id, before.id);
      const { stdout: alive } = await promisify(execFile)(
        "powershell",
        [
          "-NoProfile",
          "-Command",
          `
        Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class E2EWindowProbe { [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hWnd); }'
        [E2EWindowProbe]::IsWindow([IntPtr]([long]$env:QBIT_E2E_OLD_WINDOW))
      `,
        ],
        {
          windowsHide: true,
          env: { ...process.env, QBIT_E2E_OLD_WINDOW: String(before.window) },
        },
      );
      assert.equal(
        alive.trim(),
        "False",
        "closing settings must destroy its HWND, not just hide it",
      );
      const activation = spawn(
        process.env.QBIT_E2E_BINARY,
        ["--show-control"],
        { windowsHide: true, env: process.env, stdio: "ignore" },
      );
      await new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
          activation.kill();
          reject(new Error("Owned activation helper timed out"));
        }, 10000);
        activation.once("exit", (code) => {
          clearTimeout(timeout);
          code === 0
            ? resolve()
            : reject(new Error("Owned activation helper failed"));
        });
        activation.once("error", (error) => {
          clearTimeout(timeout);
          reject(error);
        });
      });
      const reopened = await awaitWindow(true);
      assert.equal(
        reopened.id,
        before.id,
        "second launch must activate the existing process",
      );
      assert.notEqual(reopened.window, 0, "settings must be recreated");
      assert.deepEqual((await databases()).mappings, saved.mappings);
      record("settings-window-recreated", {
        before,
        closed,
        reopened,
        persistedMapping: true,
      });
    } finally {
      await quitOwned();
      await waitFor(() => app.exitCode !== null, "owned child exit status");
      assert.equal(app.exitCode, 0, "owned host exit status");
      assert.deepEqual((await databases()).mappings, saved.mappings);
    }
  });
});
