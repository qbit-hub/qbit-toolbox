"use client";

import {
  createContext,
  createElement,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
} from "react";

export type LocaleDirection = "ltr" | "rtl";
export type TranslationParameter = string | number;
export type TranslationCatalog = Readonly<Record<string, string>>;

export const englishLocale = { locale: "en", direction: "ltr" } as const;

export const englishCoreCatalog: TranslationCatalog = {
  "hotkeys.error.modifiersHeld":
    "Release Ctrl, Shift, Alt and Windows keys, then enable Hotkeys again.",
  "features.hotkeys.name": "Hotkeys",
  "features.hotkeys.description": "Remap or disable global keyboard shortcuts.",
  "hotkeys.home": "Home",
  "hotkeys.capture.press": "Press a shortcut…",
  "hotkeys.capture.empty": "Choose shortcut",
  "hotkeys.capture.hint":
    "Capture is active while this button is focused. Escape cancels.",
  "hotkeys.capture.unsupported":
    "Unsupported or ambiguous key. Use a letter, digit, supported punctuation, or special key. AltGraph and composed input are unavailable.",
  "hotkeys.limitations":
    "Global mappings apply in ordinary apps while Hotkeys is enabled, even with this window closed. Elevated apps may reject output due to Windows UIPI. Ctrl+Alt+Delete is unsupported. Third-party injected input is bypassed.",
  "hotkeys.loading": "Loading Hotkeys…",
  "hotkeys.refresh": "Refresh status / Retry",
  "hotkeys.featureEnabled": "Enable global Hotkeys",
  "hotkeys.liveStatus": "Desired: {desired} · Runtime: {state}",
  "hotkeys.on": "On",
  "hotkeys.off": "Off",
  "hotkeys.metrics":
    "Active mappings: {count} · Emitted: {emitted} · Injection failures: {failures} · Queue overflows: {overflows}",
  "hotkeys.autostart": "Start Qbit Toolbox with Windows",
  "hotkeys.autostartError":
    "Windows startup registration is unavailable. Retry or change the setting again.",
  "hotkeys.autostartMismatch":
    "Windows registration differs from the saved preference. Reapply the setting to reconcile.",
  "hotkeys.add": "Add mapping",
  "hotkeys.empty": "No mappings yet.",
  "hotkeys.mappings": "Global mappings",
  "hotkeys.disableAction": "Disable shortcut",
  "hotkeys.sendAction": "Send another shortcut",
  "hotkeys.global": "Global",
  "hotkeys.mappingEnabled": "Enable {shortcut}",
  "hotkeys.edit": "Edit",
  "hotkeys.delete": "Delete",
  "hotkeys.label": "Label (optional)",
  "hotkeys.trigger": "Capture trigger",
  "hotkeys.target": "Capture target",
  "hotkeys.action": "Action",
  "hotkeys.review": "Review conflicts",
  "hotkeys.noConflicts": "No blocking conflicts. Ready to save.",
  "hotkeys.cancel": "Cancel",
  "hotkeys.save": "Save",
  "hotkeys.deleteConfirm": "Delete this mapping?",
  "hotkeys.error.validation":
    "This mapping contains an invalid, reserved, or unsupported shortcut.",
  "hotkeys.error.conflict":
    "The mapping conflicts with the current configuration. Review conflicts again.",
  "hotkeys.error.notFound": "This mapping no longer exists. Refresh the list.",
  "hotkeys.error.persistence":
    "Hotkeys storage is unavailable. Your last applied configuration is preserved.",
  "hotkeys.error.migration":
    "Hotkeys data needs recovery. Existing data has been preserved.",
  "hotkeys.error.platform": "Hotkeys is available on Windows only.",
  "hotkeys.error.runtime":
    "The native operation failed. Refresh status and retry.",
  "hotkeys.conflict.duplicateTrigger":
    "An enabled mapping already uses this trigger.",
  "hotkeys.conflict.selfMap": "A shortcut cannot map to itself.",
  "hotkeys.conflict.cycle": "These mappings form a cycle.",
  "hotkeys.conflict.aliasCollision":
    "Generic and sided modifier triggers overlap.",
  "hotkeys.conflict.secureAttentionSequence":
    "Ctrl+Alt+Delete is reserved by Windows.",
  "hotkeys.conflict.unsupportedKey":
    "This shortcut cannot be translated safely on Windows.",
  "hotkeys.conflict.invalidMapping": "The mapping is invalid.",
  "app.title": "Qbit Toolbox",
  "theme.preference": "Theme preference",
  "theme.system": "System",
  "theme.light": "Light",
  "theme.dark": "Dark",
  "core.status.heading": "Core status",
  "core.status.loading": "Loading core status…",
  "core.status.load_error": "Unable to load core status. Please try again.",
  "core.status.retry": "Retry",
  "core.recovery.title": "Recovery required",
  "core.recovery.message":
    "The application requires recovery before it can continue safely.",
  "core.app_version": "App version: {version}",
  "core.platform": "Platform: {platform}",
  "core.schema_version": "Schema version: {version}",
  "core.schema_unavailable": "Unavailable",
  "core.features.empty": "No features are registered yet.",
  "core.features.list_label": "Registered features",
  "core.feature.format": "{name} ({id}) — {lifecycle}",
  "lifecycle.unavailable": "Unavailable",
  "lifecycle.disabled": "Disabled",
  "lifecycle.starting": "Starting",
  "lifecycle.running": "Running",
  "lifecycle.degraded": "Degraded",
  "lifecycle.stopping": "Stopping",
  "lifecycle.failed": "Failed",
  "error.internal": "An internal error occurred.",
  "errors.core.migration_failed": "A data migration failed.",
  "errors.core.schema_unsupported": "The data schema is unsupported.",
  "errors.core.schema_inconsistent": "The data schema is inconsistent.",
  "errors.core.persistence_unavailable": "Persistent storage is unavailable.",
  "errors.core.schema_invalid": "The data schema is invalid.",
};

type TranslationParameters = Readonly<Record<string, TranslationParameter>>;
export type CatalogExtensions =
  TranslationCatalog | readonly TranslationCatalog[];

interface I18nValue {
  locale: string;
  direction: LocaleDirection;
  t: (key: string, params?: TranslationParameters) => string;
}

const I18nContext = createContext<I18nValue | null>(null);

export interface I18nProviderProps {
  children: ReactNode;
  locale?: "en";
  catalogExtensions?: CatalogExtensions;
}

function interpolate(template: string, params?: TranslationParameters): string {
  return template.replace(/\{([A-Za-z0-9_]+)\}/g, (token, name: string) => {
    const value = params?.[name];
    return value === undefined ? token : String(value);
  });
}

export function I18nProvider({
  children,
  locale = "en",
  catalogExtensions,
}: I18nProviderProps) {
  const catalog = useMemo(() => {
    const extensions =
      catalogExtensions === undefined
        ? []
        : Array.isArray(catalogExtensions)
          ? catalogExtensions
          : [catalogExtensions];
    return Object.assign(
      {},
      englishCoreCatalog,
      ...extensions,
    ) as TranslationCatalog;
  }, [catalogExtensions]);

  const direction = englishLocale.direction;
  useEffect(() => {
    if (typeof document !== "undefined") {
      document.documentElement.lang = locale;
      document.documentElement.dir = direction;
    }
  }, [direction, locale]);

  const value = useMemo<I18nValue>(
    () => ({
      locale,
      direction,
      t: (key, params) => interpolate(catalog[key] ?? key, params),
    }),
    [catalog, direction, locale],
  );

  return createElement(I18nContext.Provider, { value }, children);
}

export function useI18n(): I18nValue {
  const value = useContext(I18nContext);
  if (value === null)
    throw new Error("useI18n must be used within an I18nProvider");
  return value;
}
