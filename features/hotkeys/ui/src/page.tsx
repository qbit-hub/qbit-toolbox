"use client";
import { useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  List,
  ListItem,
  MenuItem,
  Select,
  Stack,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import { useI18n } from "@qbit/i18n";
import { AppShell } from "@qbit/ui";
import {
  ipc,
  type AutostartStatus,
  type HotkeyChord,
  type HotkeyMapping,
  type HotkeyMappingInput,
  type HotkeysConflictReport,
  type HotkeysRuntimeStatus,
} from "@qbit/ipc";
import { captureChord, formatChord } from "./capture";

function Capture({
  label,
  value,
  onChange,
}: {
  label: string;
  value: HotkeyChord | null;
  onChange: (chord: HotkeyChord) => void;
}) {
  const { t } = useI18n();
  const [active, setActive] = useState(false);
  const [invalid, setInvalid] = useState(false);
  return (
    <Stack spacing={1}>
      <Button
        aria-label={label}
        aria-pressed={active}
        variant="outlined"
        onClick={() => {
          setActive(true);
          setInvalid(false);
        }}
        onBlur={() => setActive(false)}
        onKeyDown={(event) => {
          if (!active) return;
          event.preventDefault();
          event.stopPropagation();
          const result = captureChord(event.nativeEvent);
          if (result.kind === "cancel") setActive(false);
          if (result.kind === "chord") {
            onChange(result.chord);
            setActive(false);
            setInvalid(false);
          }
          if (result.kind === "unsupported") setInvalid(true);
        }}
      >
        {label}:{" "}
        {active
          ? t("hotkeys.capture.press")
          : value
            ? formatChord(value)
            : t("hotkeys.capture.empty")}
      </Button>
      {active && (
        <Typography role="status">{t("hotkeys.capture.hint")}</Typography>
      )}
      {invalid && (
        <Alert severity="warning">{t("hotkeys.capture.unsupported")}</Alert>
      )}
    </Stack>
  );
}
function messageKey(error: unknown): string {
  if (
    error &&
    typeof error === "object" &&
    "messageKey" in error &&
    typeof error.messageKey === "string" &&
    error.messageKey.startsWith("hotkeys.")
  )
    return error.messageKey;
  return "hotkeys.error.runtime";
}
export function HotkeysPage() {
  const { t } = useI18n();
  const [mappings, setMappings] = useState<HotkeyMapping[]>([]);
  const [status, setStatus] = useState<HotkeysRuntimeStatus | null>(null);
  const [autostart, setAutostart] = useState<AutostartStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [startupError, setStartupError] = useState(false);
  const [editing, setEditing] = useState<HotkeyMapping | "new" | null>(null);
  const [deleting, setDeleting] = useState<HotkeyMapping | null>(null);
  const [trigger, setTrigger] = useState<HotkeyChord | null>(null);
  const [target, setTarget] = useState<HotkeyChord | null>(null);
  const [action, setAction] = useState<"emitShortcut" | "disable">(
    "emitShortcut",
  );
  const [label, setLabel] = useState("");
  const [report, setReport] = useState<{
    input: string;
    report: HotkeysConflictReport;
  } | null>(null);
  const generation = useRef(0);
  const loadGeneration = useRef(0);

  async function refresh() {
    const sequence = ++loadGeneration.current;
    const [list, runtime, startup] = await Promise.allSettled([
      ipc.hotkeysListMappings(),
      ipc.hotkeysGetRuntimeStatus(),
      ipc.getAutostart(),
    ]);
    if (sequence !== loadGeneration.current) return;
    if (list.status === "fulfilled") setMappings(list.value);
    else setError(messageKey(list.reason));
    if (runtime.status === "fulfilled") setStatus(runtime.value);
    else setError(messageKey(runtime.reason));
    setStartupError(startup.status === "rejected");
    if (startup.status === "fulfilled") setAutostart(startup.value);
    setLoading(false);
  }
  useEffect(() => {
    let alive = true;
    void Promise.allSettled([
      ipc.hotkeysListMappings(),
      ipc.hotkeysGetRuntimeStatus(),
      ipc.getAutostart(),
    ]).then(([list, runtime, startup]) => {
      if (!alive) return;
      if (list.status === "fulfilled") setMappings(list.value);
      else setError(messageKey(list.reason));
      if (runtime.status === "fulfilled") setStatus(runtime.value);
      else setError(messageKey(runtime.reason));
      if (startup.status === "fulfilled") setAutostart(startup.value);
      else setStartupError(true);
      setLoading(false);
    });
    return () => {
      alive = false;
    };
  }, []);
  async function perform(operation: () => Promise<unknown>) {
    if (busy) return;
    loadGeneration.current += 1;
    setBusy(true);
    setError(null);
    try {
      await operation();
      await refresh();
    } catch (failure) {
      setError(messageKey(failure));
      await ipc
        .hotkeysGetRuntimeStatus()
        .then(setStatus)
        .catch(() => {});
    } finally {
      setBusy(false);
    }
  }
  const input: HotkeyMappingInput | null =
    trigger && (action === "disable" || target)
      ? {
          enabled: editing && editing !== "new" ? editing.input.enabled : true,
          trigger,
          action:
            action === "disable"
              ? { kind: "disable" }
              : { kind: "emitShortcut", chord: target! },
          label: label || null,
        }
      : null;
  const signature = input ? JSON.stringify(input) : "";
  const reviewed = report?.input === signature;
  const blocking =
    !reviewed ||
    report.report.conflicts.some((conflict) => conflict.severity === "error");
  function open(mapping: HotkeyMapping | "new") {
    generation.current += 1;
    setReport(null);
    setEditing(mapping);
    setTrigger(mapping === "new" ? null : mapping.input.trigger);
    setTarget(
      mapping !== "new" && mapping.input.action.kind === "emitShortcut"
        ? mapping.input.action.chord
        : null,
    );
    setAction(mapping === "new" ? "emitShortcut" : mapping.input.action.kind);
    setLabel(mapping === "new" ? "" : (mapping.input.label ?? ""));
  }
  return (
    <AppShell>
      <Stack spacing={3}>
        <Button href="/">{t("hotkeys.home")}</Button>
        <Typography component="h2" variant="h4">
          {t("features.hotkeys.name")}
        </Typography>
        <Alert severity="info">{t("hotkeys.limitations")}</Alert>
        {loading && (
          <Typography role="status">{t("hotkeys.loading")}</Typography>
        )}
        {error && <Alert severity="error">{t(error)}</Alert>}
        <Button
          disabled={busy}
          onClick={() => {
            setError(null);
            void refresh();
          }}
        >
          {t("hotkeys.refresh")}
        </Button>
        {status && (
          <>
            <FormControlLabel
              label={t("hotkeys.featureEnabled")}
              control={
                <Switch
                  checked={status.desiredEnabled}
                  disabled={busy || status.lifecycleState === "unavailable"}
                  onChange={(_, enabled) =>
                    void perform(() => ipc.hotkeysSetFeatureEnabled(enabled))
                  }
                />
              }
            />
            <Typography role="status">
              {t("hotkeys.liveStatus", {
                desired: t(
                  status.desiredEnabled ? "hotkeys.on" : "hotkeys.off",
                ),
                state: t("lifecycle." + status.lifecycleState),
              })}
            </Typography>
            {status.lastEngineError && (
              <Alert severity="error">
                {t(status.lastEngineError.messageKey)}
              </Alert>
            )}
            <Typography>
              {t("hotkeys.metrics", {
                count: status.metrics.mappingCount,
                emitted: status.metrics.actionsEmitted,
                failures: status.metrics.injectionFailures,
                overflows: status.metrics.queueOverflows,
              })}
            </Typography>
          </>
        )}
        <FormControlLabel
          label={t("hotkeys.autostart")}
          control={
            <Switch
              checked={autostart?.desiredEnabled ?? false}
              disabled={busy || !autostart || startupError}
              onChange={(_, enabled) =>
                void perform(() => ipc.setAutostart(enabled))
              }
            />
          }
        />
        {startupError && (
          <Alert severity="error">{t("hotkeys.autostartError")}</Alert>
        )}
        {autostart && autostart.registered !== autostart.desiredEnabled && (
          <Alert severity="warning">{t("hotkeys.autostartMismatch")}</Alert>
        )}
        <Button
          variant="contained"
          disabled={
            loading ||
            busy ||
            !status ||
            status.lifecycleState === "unavailable"
          }
          onClick={() => open("new")}
        >
          {t("hotkeys.add")}
        </Button>
        {!loading && mappings.length === 0 && (
          <Typography>{t("hotkeys.empty")}</Typography>
        )}
        <List aria-label={t("hotkeys.mappings")}>
          {mappings.map((mapping) => (
            <ListItem key={mapping.id}>
              <Stack spacing={1}>
                <Typography>
                  {mapping.input.label || formatChord(mapping.input.trigger)}:{" "}
                  {formatChord(mapping.input.trigger)} →{" "}
                  {mapping.input.action.kind === "disable"
                    ? t("hotkeys.disableAction")
                    : formatChord(mapping.input.action.chord)}{" "}
                  · {t("hotkeys.global")}
                </Typography>
                <Stack direction="row" spacing={1}>
                  <FormControlLabel
                    label={t("hotkeys.mappingEnabled", {
                      shortcut: formatChord(mapping.input.trigger),
                    })}
                    control={
                      <Switch
                        checked={mapping.input.enabled}
                        disabled={busy}
                        onChange={(_, enabled) =>
                          void perform(() =>
                            ipc.hotkeysSetMappingEnabled(mapping.id, enabled),
                          )
                        }
                      />
                    }
                  />
                  <Button disabled={busy} onClick={() => open(mapping)}>
                    {t("hotkeys.edit")}
                  </Button>
                  <Button disabled={busy} onClick={() => setDeleting(mapping)}>
                    {t("hotkeys.delete")}
                  </Button>
                </Stack>
              </Stack>
            </ListItem>
          ))}
        </List>
        <Dialog
          open={editing !== null}
          onClose={() => {
            if (!busy) setEditing(null);
          }}
          fullWidth
          aria-labelledby="hotkeys-editor-title"
        >
          <DialogTitle id="hotkeys-editor-title">
            {t(editing === "new" ? "hotkeys.add" : "hotkeys.edit")}
          </DialogTitle>
          <DialogContent>
            <Stack spacing={3} sx={{ pt: 1 }}>
              {error && <Alert severity="error">{t(error)}</Alert>}
              <TextField
                autoFocus
                label={t("hotkeys.label")}
                value={label}
                inputProps={{ maxLength: 120 }}
                onChange={(e) => {
                  setLabel(e.target.value);
                  setReport(null);
                }}
              />
              <Capture
                label={t("hotkeys.trigger")}
                value={trigger}
                onChange={(value) => {
                  setTrigger(value);
                  setReport(null);
                }}
              />
              <Select
                inputProps={{ "aria-label": t("hotkeys.action") }}
                value={action}
                onChange={(e) => {
                  setAction(e.target.value as typeof action);
                  setReport(null);
                }}
              >
                <MenuItem value="emitShortcut">
                  {t("hotkeys.sendAction")}
                </MenuItem>
                <MenuItem value="disable">
                  {t("hotkeys.disableAction")}
                </MenuItem>
              </Select>
              {action === "emitShortcut" && (
                <Capture
                  label={t("hotkeys.target")}
                  value={target}
                  onChange={(value) => {
                    setTarget(value);
                    setReport(null);
                  }}
                />
              )}
              <Button
                disabled={!input || busy}
                onClick={() => {
                  if (!input) return;
                  const sequence = ++generation.current;
                  const snapshot = signature;
                  setBusy(true);
                  void ipc
                    .hotkeysAnalyzeCandidate(
                      input,
                      editing && editing !== "new" ? editing.id : null,
                    )
                    .then((value) => {
                      if (sequence === generation.current)
                        setReport({ input: snapshot, report: value });
                    })
                    .catch((failure) => setError(messageKey(failure)))
                    .finally(() => setBusy(false));
                }}
              >
                {t("hotkeys.review")}
              </Button>
              {reviewed && report.report.conflicts.length === 0 && (
                <Alert severity="success">{t("hotkeys.noConflicts")}</Alert>
              )}
              {reviewed &&
                report.report.conflicts.map((conflict, index) => (
                  <Alert key={index} severity={conflict.severity}>
                    {t(conflict.messageKey)}
                  </Alert>
                ))}
            </Stack>
          </DialogContent>
          <DialogActions>
            <Button disabled={busy} onClick={() => setEditing(null)}>
              {t("hotkeys.cancel")}
            </Button>
            <Button
              disabled={!input || busy || blocking}
              onClick={() => {
                if (!input) return;
                void perform(async () => {
                  if (editing === "new") await ipc.hotkeysCreateMapping(input);
                  else if (editing)
                    await ipc.hotkeysUpdateMapping(editing.id, input);
                  setEditing(null);
                });
              }}
            >
              {t("hotkeys.save")}
            </Button>
          </DialogActions>
        </Dialog>
        <Dialog
          open={deleting !== null}
          onClose={() => {
            if (!busy) setDeleting(null);
          }}
          aria-labelledby="hotkeys-delete-title"
        >
          <DialogTitle id="hotkeys-delete-title">
            {t("hotkeys.deleteConfirm")}
          </DialogTitle>
          <DialogActions>
            <Button disabled={busy} onClick={() => setDeleting(null)}>
              {t("hotkeys.cancel")}
            </Button>
            <Button
              disabled={busy}
              onClick={() => {
                if (deleting)
                  void perform(async () => {
                    await ipc.hotkeysDeleteMapping(deleting.id);
                    setDeleting(null);
                  });
              }}
            >
              {t("hotkeys.delete")}
            </Button>
          </DialogActions>
        </Dialog>
      </Stack>
    </AppShell>
  );
}
