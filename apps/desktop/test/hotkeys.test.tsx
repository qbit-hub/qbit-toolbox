import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import { HotkeysPage, captureChord } from "@qbit/hotkeys-ui";
import { Providers } from "../app/providers";
import {
  createIpcClient,
  type HotkeyMappingInput,
  type HotkeysRuntimeStatus,
} from "@qbit/ipc";
import axe from "axe-core";

const mock = vi.hoisted(() => ({
  hotkeysListMappings: vi.fn(),
  hotkeysGetRuntimeStatus: vi.fn(),
  getAutostart: vi.fn(),
  hotkeysSetFeatureEnabled: vi.fn(),
  hotkeysSetMappingEnabled: vi.fn(),
  hotkeysCreateMapping: vi.fn(),
  hotkeysUpdateMapping: vi.fn(),
  hotkeysDeleteMapping: vi.fn(),
  hotkeysAnalyzeCandidate: vi.fn(),
  setAutostart: vi.fn(),
}));
vi.mock("@qbit/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@qbit/ipc")>()),
  ipc: mock,
}));
const status: HotkeysRuntimeStatus = {
  desiredEnabled: false,
  lifecycleState: "disabled",
  lastEngineError: null,
  metrics: {
    hookActive: false,
    mappingCount: 0,
    eventsSeen: 0,
    eventsMatched: 0,
    actionsEmitted: 0,
    injectionFailures: 0,
    queueOverflows: 0,
    configReloadFailures: 0,
  },
};
function key(key: string, options: Partial<KeyboardEventInit> = {}) {
  return new KeyboardEvent("keydown", { key, ...options });
}
function setup() {
  mock.hotkeysListMappings.mockResolvedValue([]);
  mock.hotkeysGetRuntimeStatus.mockResolvedValue(status);
  mock.getAutostart.mockResolvedValue({
    desiredEnabled: false,
    registered: false,
  });
  mock.hotkeysAnalyzeCandidate.mockResolvedValue({ conflicts: [] });
  return render(
    <Providers>
      <HotkeysPage />
    </Providers>,
  );
}
afterEach(() => vi.resetAllMocks());
describe("focused shortcut capture", () => {
  it("is layout-aware, ordered, and distinguishes numpad Enter", () => {
    expect(
      captureChord(key("z", { code: "KeyY", ctrlKey: true, shiftKey: true })),
    ).toEqual({
      kind: "chord",
      chord: {
        modifiers: ["ctrl", "shift"],
        primary: { kind: "logical", token: "KeyZ" },
      },
    });
    expect(
      captureChord(key("Enter", { code: "NumpadEnter", location: 3 })),
    ).toMatchObject({ chord: { primary: { token: "NumpadEnter" } } });
    expect(captureChord(key("7"))).toMatchObject({
      chord: { primary: { token: "Digit7" } },
    });
    expect(captureChord(key(";", { code: "Semicolon" }))).toMatchObject({
      chord: { primary: { token: "Semicolon" } },
    });
  });
  it("waits for a primary, filters repeat, cancels Escape and rejects ambiguity", () => {
    expect(captureChord(key("Control"))).toEqual({ kind: "wait" });
    expect(captureChord(key("a", { repeat: true }))).toEqual({ kind: "wait" });
    expect(captureChord(key("Escape"))).toEqual({ kind: "cancel" });
    for (const event of [
      key("Dead"),
      key("ض", { code: "KeyQ" }),
      key("@", { code: "Digit2" }),
      key("Process", { isComposing: true }),
    ])
      expect(captureChord(event)).toEqual({ kind: "unsupported" });
  });
});
describe("Hotkeys page", () => {
  it("edits existing mappings, honors blocking conflicts, and invalidates prior reviews", async () => {
    setup();
    const input: HotkeyMappingInput = {
      enabled: true,
      trigger: {
        modifiers: ["ctrl"],
        primary: { kind: "logical", token: "KeyA" },
      },
      action: { kind: "disable" },
      label: "Existing",
    };
    mock.hotkeysListMappings.mockResolvedValue([{ id: "mapping-1", input }]);
    fireEvent.click(
      await screen.findByRole("button", { name: "Refresh status / Retry" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
    const dialog = screen.getByRole("dialog");
    mock.hotkeysAnalyzeCandidate.mockResolvedValueOnce({
      conflicts: [
        {
          code: "duplicateTrigger",
          severity: "error",
          messageKey: "hotkeys.conflict.duplicateTrigger",
        },
      ],
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review conflicts" }),
    );
    await within(dialog).findByText(
      "An enabled mapping already uses this trigger.",
    );
    expect(within(dialog).getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review conflicts" }),
    );
    await within(dialog).findByText("No blocking conflicts. Ready to save.");
    fireEvent.change(
      within(dialog).getByRole("textbox", { name: "Label (optional)" }),
      { target: { value: "Updated" } },
    );
    expect(within(dialog).getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review conflicts" }),
    );
    await within(dialog).findByText("No blocking conflicts. Ready to save.");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(mock.hotkeysUpdateMapping).toHaveBeenCalledWith("mapping-1", {
        ...input,
        label: "Updated",
      }),
    );
    expect(mock.hotkeysCreateMapping).not.toHaveBeenCalled();
  });
  it("uses a per-row toggle and requires confirmation before deletion", async () => {
    setup();
    const input: HotkeyMappingInput = {
      enabled: true,
      trigger: { modifiers: [], primary: { kind: "logical", token: "F12" } },
      action: { kind: "disable" },
      label: null,
    };
    mock.hotkeysListMappings.mockResolvedValue([{ id: "mapping-2", input }]);
    fireEvent.click(
      await screen.findByRole("button", { name: "Refresh status / Retry" }),
    );
    fireEvent.click(await screen.findByRole("switch", { name: "Enable F12" }));
    await waitFor(() =>
      expect(mock.hotkeysSetMappingEnabled).toHaveBeenCalledWith(
        "mapping-2",
        false,
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Delete" })).not.toBeDisabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(mock.hotkeysDeleteMapping).not.toHaveBeenCalled();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Cancel",
      }),
    );
    expect(mock.hotkeysDeleteMapping).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Delete",
      }),
    );
    await waitFor(() =>
      expect(mock.hotkeysDeleteMapping).toHaveBeenCalledWith("mapping-2"),
    );
    expect(mock.hotkeysSetFeatureEnabled).not.toHaveBeenCalled();
  });
  it("has no automated accessibility violations in loaded controls", async () => {
    const { container } = setup();
    await screen.findByText("No mappings yet.");
    const result = await axe.run(container, {
      rules: { "color-contrast": { enabled: false } },
    });
    expect(result.violations).toEqual([]);
  });
  it("separates the global and startup toggles and performs native feature toggle", async () => {
    setup();
    const toggle = await screen.findByRole("switch", {
      name: "Enable global Hotkeys",
    });
    expect(toggle).not.toBeChecked();
    expect(
      screen.getByRole("switch", { name: "Start Qbit Toolbox with Windows" }),
    ).not.toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(mock.hotkeysSetFeatureEnabled).toHaveBeenCalledWith(true),
    );
    expect(mock.setAutostart).not.toHaveBeenCalled();
  });
  it("requires focused captures and Rust conflict review before save", async () => {
    setup();
    await screen.findByText("No mappings yet.");
    fireEvent.click(screen.getByRole("button", { name: "Add mapping" }));
    const dialog = screen.getByRole("dialog");
    const trigger = within(dialog).getByRole("button", {
      name: "Capture trigger",
    });
    fireEvent.keyDown(trigger, { key: "a", ctrlKey: true });
    expect(trigger).toHaveTextContent("Choose shortcut");
    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, { key: "a", code: "KeyA", ctrlKey: true });
    const target = within(dialog).getByRole("button", {
      name: "Capture target",
    });
    fireEvent.click(target);
    fireEvent.keyDown(target, { key: "b", code: "KeyB", ctrlKey: true });
    expect(within(dialog).getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review conflicts" }),
    );
    await within(dialog).findByText("No blocking conflicts. Ready to save.");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(mock.hotkeysCreateMapping).toHaveBeenCalledWith(
        expect.objectContaining({
          trigger: {
            modifiers: ["ctrl"],
            primary: { kind: "logical", token: "KeyA" },
          },
        }),
      ),
    );
  });
  it("shows safe errors and retries without rendering native details", async () => {
    setup();
    mock.hotkeysListMappings.mockRejectedValueOnce({
      messageKey: "hotkeys.error.persistence",
      detail: "private.db",
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "Refresh status / Retry" }),
    );
    await screen.findByText(
      "Hotkeys storage is unavailable. Your last applied configuration is preserved.",
    );
    expect(screen.queryByText("private.db")).not.toBeInTheDocument();
  });
});
it("typed adapter preserves command arguments and rejected structured errors", async () => {
  const invoke = vi.fn().mockResolvedValue(undefined);
  const client = createIpcClient(invoke);
  const input: HotkeyMappingInput = {
    enabled: true,
    trigger: { modifiers: [], primary: { kind: "logical", token: "F24" } },
    action: { kind: "disable" },
    label: null,
  };
  await client.hotkeysUpdateMapping("id", input);
  expect(invoke).toHaveBeenCalledWith("hotkeys_update_mapping", {
    id: "id",
    input,
  });
  await client.hotkeysAnalyzeCandidate(input, null);
  expect(invoke).toHaveBeenCalledWith("hotkeys_analyze_candidate", {
    input,
    id: null,
  });
  const error = { code: "hotkeys_conflict", category: "conflict" };
  invoke.mockRejectedValueOnce(error);
  await expect(client.hotkeysCreateMapping(input)).rejects.toBe(error);
});
