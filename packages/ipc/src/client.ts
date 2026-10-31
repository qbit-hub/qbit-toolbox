import { invoke } from "@tauri-apps/api/core";

import type {
  CoreStatusDto,
  HotkeyMapping,
  HotkeyMappingInput,
  HotkeysRuntimeStatus,
  HotkeysConflictReport,
  AutostartStatus,
} from "../generated";

/**
 * The only browser-facing boundary for commands implemented by the Tauri host.
 * Command strings intentionally stay here so UI packages never depend on Tauri.
 */
export interface IpcClient {
  getCoreStatus(): Promise<CoreStatusDto>;
  hotkeysListMappings(): Promise<HotkeyMapping[]>;
  hotkeysCreateMapping(input: HotkeyMappingInput): Promise<HotkeyMapping>;
  hotkeysUpdateMapping(
    id: string,
    input: HotkeyMappingInput,
  ): Promise<HotkeyMapping>;
  hotkeysDeleteMapping(id: string): Promise<void>;
  hotkeysSetMappingEnabled(
    id: string,
    enabled: boolean,
  ): Promise<HotkeyMapping>;
  hotkeysSetFeatureEnabled(enabled: boolean): Promise<HotkeysRuntimeStatus>;
  hotkeysGetRuntimeStatus(): Promise<HotkeysRuntimeStatus>;
  hotkeysAnalyzeCandidate(
    input: HotkeyMappingInput,
    id: string | null,
  ): Promise<HotkeysConflictReport>;
  getAutostart(): Promise<AutostartStatus>;
  setAutostart(enabled: boolean): Promise<AutostartStatus>;
}

export function createIpcClient(
  invokeCommand: <T>(
    command: string,
    args?: Record<string, unknown>,
  ) => Promise<T> = invoke,
): IpcClient {
  return {
    getCoreStatus: () => invokeCommand<CoreStatusDto>("get_core_status"),
    hotkeysListMappings: () =>
      invokeCommand<HotkeyMapping[]>("hotkeys_list_mappings"),
    hotkeysCreateMapping: (input: HotkeyMappingInput) =>
      invokeCommand<HotkeyMapping>("hotkeys_create_mapping", { input }),
    hotkeysUpdateMapping: (id: string, input: HotkeyMappingInput) =>
      invokeCommand<HotkeyMapping>("hotkeys_update_mapping", { id, input }),
    hotkeysDeleteMapping: (id: string) =>
      invokeCommand<void>("hotkeys_delete_mapping", { id }),
    hotkeysSetMappingEnabled: (id: string, enabled: boolean) =>
      invokeCommand<HotkeyMapping>("hotkeys_set_mapping_enabled", {
        id,
        enabled,
      }),
    hotkeysSetFeatureEnabled: (enabled: boolean) =>
      invokeCommand<HotkeysRuntimeStatus>("hotkeys_set_feature_enabled", {
        enabled,
      }),
    hotkeysGetRuntimeStatus: () =>
      invokeCommand<HotkeysRuntimeStatus>("hotkeys_get_runtime_status"),
    hotkeysAnalyzeCandidate: (input: HotkeyMappingInput, id: string | null) =>
      invokeCommand<HotkeysConflictReport>("hotkeys_analyze_candidate", {
        input,
        id,
      }),
    getAutostart: () => invokeCommand<AutostartStatus>("get_autostart"),
    setAutostart: (enabled: boolean) =>
      invokeCommand<AutostartStatus>("set_autostart", { enabled }),
  };
}

export const ipc = createIpcClient();
