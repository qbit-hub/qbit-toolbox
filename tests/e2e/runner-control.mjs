// Orchestration only. Injectable boundaries let ordinary unit tests exercise
// launch/interrupt/cleanup failures without spawning a GUI, hook or driver.
export async function runOwnedDesktop({
  launch,
  sample,
  cleanup,
  finalize,
  signalSource = process,
  timeoutMs = 300000,
  sampleMs = 500,
}) {
  let child,
    timer,
    watchdog,
    pending,
    failure,
    code = 1,
    tail = "",
    cleanupEvidence,
    cleanupFailure;
  const owned = new Map();
  function interrupt() {
    failure ??= new Error(
      "PROCESS_INTERRUPTED: owned desktop runner interrupted or timed out",
    );
    if (child?.pid && child.exitCode === null && child.signalCode === null) {
      try {
        child.kill();
      } catch (error) {
        failure ??= error;
      }
    }
  }
  async function snapshot() {
    if (
      pending ||
      !child?.pid ||
      child.exitCode !== null ||
      child.signalCode !== null
    )
      return;
    pending = (async () => {
      try {
        for (const identity of await sample(child))
          owned.set(`${identity.id}:${identity.start}`, identity);
      } catch (error) {
        failure ??= error;
        interrupt();
      }
    })();
    try {
      await pending;
    } finally {
      pending = null;
    }
  }
  try {
    child = launch();
    const exited = new Promise((resolve, reject) => {
      child.once("exit", resolve);
      child.once("error", reject);
    });
    for (const stream of [child.stdout, child.stderr])
      stream?.on("data", (chunk) => {
        const text = chunk.toString();
        tail = (tail + text).slice(-65536);
        process.stdout.write(text);
      });
    timer = setInterval(() => {
      void snapshot();
    }, sampleMs);
    watchdog = setTimeout(interrupt, timeoutMs);
    signalSource.once("SIGINT", interrupt);
    signalSource.once("SIGTERM", interrupt);
    void snapshot();
    code = (await exited) ?? 1;
    if (code !== 0 && !failure)
      failure = new Error(
        tail ||
          "PROCESS_LAUNCH_FAILURE: desktop worker exited before reporting scenarios",
      );
  } catch (error) {
    failure ??= error;
  } finally {
    clearInterval(timer);
    clearTimeout(watchdog);
    signalSource.removeListener("SIGINT", interrupt);
    signalSource.removeListener("SIGTERM", interrupt);
    if (pending) await pending;
    try {
      // A failed spawn has no owned PID and grants no cleanup authority.
      if (Number.isInteger(child?.pid) && child.pid > 0)
        cleanupEvidence = await cleanup([...owned.values()]);
    } catch (error) {
      failure ??= error;
      cleanupFailure = error.message;
    } finally {
      if (failure) code = 1;
      await finalize(failure, {
        code,
        cleanupEvidence,
        cleanupFailure,
        outputTail: tail,
        owned: [...owned.values()],
      });
    }
  }
  return { code, failure };
}
