// Executed with Bun by the existing Node/WebdriverIO runner. Queries only the
// two fixed databases beneath the runner-owned temp fixture; no SQL writes.
import { Database } from "bun:sqlite";
import { join } from "node:path";
import { realpathSync, existsSync } from "node:fs";
import { validateRoot } from "./isolation.mjs";
const root = validateRoot(process.env.QBIT_E2E_DATA_ROOT);
const paths = ["data/core.db", "data/features/qbit.hotkeys/data.db"].map((p) =>
  join(root, p),
);
const hotkeysDatabasePresent = existsSync(paths[1]);
for (const path of paths.filter(
  (path, index) => index === 0 || hotkeysDatabasePresent,
)) {
  if (realpathSync(path).toLowerCase() !== path.toLowerCase())
    throw new Error("BLOCKED_ISOLATION: DB path substitution");
}
const core = new Database(paths[0], { readonly: true, strict: true });
let hotkeys;
try {
  if (hotkeysDatabasePresent)
    hotkeys = new Database(paths[1], { readonly: true, strict: true });
  const integrity = (db) => {
    const values = db
      .query("PRAGMA quick_check")
      .all()
      .map((row) => Object.values(row)[0]);
    if (values.length !== 1 || values[0] !== "ok")
      throw new Error("INVALID_STORED_STATE: SQLite integrity failure");
    return "ok";
  };
  const desiredRows = core
    .query("SELECT enabled FROM feature_state WHERE feature_id='qbit.hotkeys'")
    .all();
  if (
    desiredRows.length > 1 ||
    (desiredRows.length === 1 && ![0, 1].includes(desiredRows[0].enabled))
  ) {
    throw new Error(
      "INVALID_STORED_STATE: unsupported or duplicate feature intent",
    );
  }
  const persistedDesired =
    desiredRows.length === 1 ? desiredRows[0].enabled : null;
  const mappings = hotkeys
    ? hotkeys
        .query(
          "SELECT id, enabled, payload FROM mappings ORDER BY sort_order, id",
        )
        .all()
        .map((row) => {
          const payload = JSON.parse(row.payload);
          if (
            ![0, 1].includes(row.enabled) ||
            payload?.id !== row.id ||
            payload.enabled !== (row.enabled === 1)
          ) {
            throw new Error(
              "INVALID_STORED_STATE: mapping row/payload mismatch",
            );
          }
          return { ...row, payload };
        })
    : [];
  if (new Set(mappings.map((row) => row.id)).size !== mappings.length)
    throw new Error("INVALID_STORED_STATE: duplicate mapping identity");
  console.log(
    JSON.stringify({
      root,
      coreIntegrity: integrity(core),
      hotkeysIntegrity: hotkeys ? integrity(hotkeys) : null,
      hotkeysDatabasePresent,
      configured: persistedDesired !== null,
      persistedDesired,
      effectiveDesired: persistedDesired ?? 0,
      mappings,
    }),
  );
} finally {
  hotkeys?.close();
  core.close();
}
