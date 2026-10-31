import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

const root = process.cwd();
const errors: string[] = [];
const controlPermissions = [
  "allow-get-core-status",
  "allow-hotkeys-list-mappings",
  "allow-hotkeys-create-mapping",
  "allow-hotkeys-update-mapping",
  "allow-hotkeys-delete-mapping",
  "allow-hotkeys-set-mapping-enabled",
  "allow-hotkeys-set-feature-enabled",
  "allow-hotkeys-get-runtime-status",
  "allow-hotkeys-analyze-candidate",
  "allow-get-autostart",
  "allow-set-autostart",
];
const ignoredDirectories = new Set([
  ".ai-bridge",
  ".git",
  ".next",
  ".turbo",
  "build",
  "coverage",
  "dist",
  "generated",
  "node_modules",
  "out",
  "target",
]);
const sourceExtensions = new Set([
  ".ts",
  ".tsx",
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
  ".rs",
]);
const packageDependencySections = [
  "dependencies",
  "devDependencies",
  "peerDependencies",
  "optionalDependencies",
] as const;

function slashPath(path: string): string {
  return path.split(sep).join("/");
}

function canonicalPrefix(prefix: string): string {
  return prefix.replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
}

function isWithin(path: string, prefix: string): boolean {
  const normalizedPath = slashPath(path).replace(/^\/+/, "");
  const normalizedPrefix = canonicalPrefix(prefix);
  return (
    normalizedPath === normalizedPrefix ||
    normalizedPath.startsWith(`${normalizedPrefix}/`)
  );
}

function walk(directory: string): string[] {
  if (!existsSync(directory)) return [];
  const files: string[] = [];
  for (const entry of readdirSync(directory)) {
    if (ignoredDirectories.has(entry)) continue;
    const fullPath = join(directory, entry);
    if (statSync(fullPath).isDirectory()) files.push(...walk(fullPath));
    else files.push(fullPath);
  }
  return files;
}

function repoPath(file: string): string {
  return slashPath(relative(root, file));
}

function report(file: string, message: string): void {
  errors.push(`${repoPath(file)}: ${message}`);
}

function readText(file: string): string | undefined {
  try {
    return readFileSync(file, "utf8");
  } catch (error: unknown) {
    report(
      file,
      `could not read file (${error instanceof Error ? error.message : "unknown error"})`,
    );
    return undefined;
  }
}

function parseJson(file: string): unknown | undefined {
  const text = readText(file);
  if (text === undefined) return undefined;
  try {
    const value: unknown = JSON.parse(text);
    return value;
  } catch (error: unknown) {
    report(
      file,
      `invalid JSON (${error instanceof Error ? error.message : "unknown error"})`,
    );
    return undefined;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function stringArray(value: unknown): string[] | undefined {
  return Array.isArray(value) && value.every((item) => typeof item === "string")
    ? value
    : undefined;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function hasToken(text: string, token: string): boolean {
  return new RegExp(
    `(^|[^A-Za-z0-9_@/-])${escapeRegExp(token)}(?=$|[^A-Za-z0-9_@/-])`,
  ).test(text);
}

function md3NextTranspilePackages(text: string): RegExpMatchArray | null {
  return text.match(/transpilePackages\s*:\s*\[([\s\S]*?)\]/);
}

function allowsMd3NextInDesktopConfig(text: string): boolean {
  const transpilePackages = md3NextTranspilePackages(text);
  return (
    transpilePackages !== null &&
    /(["'])md3-next\1/.test(transpilePackages[1]) &&
    !hasToken(text.replace(transpilePackages[0], ""), "md3-next")
  );
}

function packageDependencies(file: string): void {
  const value = parseJson(file);
  if (!isRecord(value)) return;
  const manifest = value;
  for (const section of packageDependencySections) {
    const dependencies = manifest[section];
    if (!isRecord(dependencies)) continue;
    for (const dependency of Object.keys(dependencies)) {
      if (
        dependency === "md3-next" &&
        !isWithin(repoPath(file), "packages/ui")
      ) {
        report(file, "md3-next is allowed only in packages/ui/package.json");
      }
      if (
        dependency === "@tauri-apps/api" &&
        !isWithin(repoPath(file), "packages/ipc")
      ) {
        report(
          file,
          "@tauri-apps/api is allowed only in packages/ipc/package.json",
        );
      }
    }
  }
}

function checkCapability(file: string): void {
  const value = parseJson(file);
  if (!isRecord(value)) return;
  const capability = value;
  const identifier = capability.identifier;
  const windows = capability.windows;
  const webviews = capability.webviews;
  const permissions = capability.permissions;
  const checkOptional = (
    name: string,
    field: unknown,
    forbidden: string,
  ): string[] | undefined => {
    if (field === undefined) return undefined;
    const values = stringArray(field);
    if (values === undefined) {
      report(file, `${name} must be an array of strings when present`);
      return undefined;
    }
    if (values.includes(forbidden))
      report(file, `${name} must not contain ${forbidden}`);
    return values;
  };
  const windowValues = checkOptional("windows", windows, "*");
  checkOptional("webviews", webviews, "*");
  const permissionValues = checkOptional(
    "permissions",
    permissions,
    "core:default",
  );
  if (identifier === "control-window") {
    if (
      windowValues === undefined ||
      windowValues.length !== 1 ||
      windowValues[0] !== "control"
    ) {
      report(file, 'control-window must define windows exactly ["control"]');
    }
    if (
      permissionValues === undefined ||
      permissionValues.length !== controlPermissions.length ||
      controlPermissions.some(
        (permission) => !permissionValues.includes(permission),
      )
    ) {
      report(
        file,
        "control-window must define exactly the reviewed explicit control command permissions",
      );
    }
  }
}

function cargoUsesRusqlite(file: string): void {
  const path = repoPath(file);
  const text = readText(file);
  if (text === undefined || !hasToken(text, "rusqlite")) return;
  const isFeatureNativePersistence =
    /^features\/[^/]+\/native\/Cargo\.toml$/.test(path) ||
    /^features\/[^/]+\/native\/src\/persistence\//.test(path);
  if (isWithin(path, "crates/persistence") || isFeatureNativePersistence)
    return;
  if (path === "Cargo.toml") {
    const outsideWorkspaceDependencies = text
      .split(/(?=^\s*\[[^\]]+\]\s*$)/m)
      .some(
        (section) =>
          !/^\s*\[workspace\.dependencies\]\s*$/m.test(section) &&
          hasToken(section, "rusqlite"),
      );
    if (!outsideWorkspaceDependencies) return;
  }
  report(
    file,
    "rusqlite is allowed only in [workspace.dependencies], crates/persistence, or feature-native persistence modules",
  );
}

function checkSource(file: string): void {
  const path = repoPath(file);
  const nativeLayer =
    /^features\/[^/]+\/native\/src\/(domain|application|runtime)\//.test(path);
  if (!sourceExtensions.has(path.slice(path.lastIndexOf(".")).toLowerCase()))
    return;
  if (
    !isWithin(path, "apps") &&
    !isWithin(path, "packages") &&
    !isWithin(path, "features")
  )
    return;
  const text = readText(file);
  if (text === undefined) return;
  if (nativeLayer && /\b(?:windows|tauri)::/.test(text))
    report(
      file,
      "OS and Tauri APIs must stay outside feature domain/application/runtime",
    );
  if (
    /^features\/[^/]+\/native\/src\//.test(path) &&
    /\bunsafe\s*(?:\{|extern|fn)/.test(text) &&
    !/\/platform\/windows\/ffi\.rs$/.test(path)
  )
    report(
      file,
      "unsafe code is confined to the reviewed Windows FFI boundary",
    );
  if (
    hasToken(text, "md3-next") &&
    !isWithin(path, "packages/ui") &&
    !(
      path === "apps/desktop/next.config.ts" &&
      allowsMd3NextInDesktopConfig(text)
    )
  ) {
    report(file, "md3-next may be used only by packages/ui source");
  }
  if (hasToken(text, "@tauri-apps/api") && !isWithin(path, "packages/ipc")) {
    report(file, "@tauri-apps/api may be used only by packages/ipc source");
  }
  const requiresIpcAdapter =
    isWithin(path, "apps") ||
    /^features\/[^/]+\/ui\//.test(path) ||
    isWithin(path, "packages/ui") ||
    isWithin(path, "packages/i18n");
  if (requiresIpcAdapter && /\binvoke\s*(?:<[^>]*>)?\s*\(/.test(text)) {
    report(file, "invoke may be used only by packages/ipc source");
  }
  if (requiresIpcAdapter && hasToken(text, "@tauri-apps/api/core")) {
    report(
      file,
      "@tauri-apps/api/core may be imported only by packages/ipc source",
    );
  }
}

function checkDesktopNextConfig(): void {
  const file = join(root, "apps", "desktop", "next.config.ts");
  const text = readText(file);
  if (text === undefined) return;
  if (!allowsMd3NextInDesktopConfig(text)) {
    report(file, 'transpilePackages must include "md3-next"');
  }
}

function featureTokens(featureDirectory: string): string[] {
  const tokens = new Set<string>([
    repoPath(featureDirectory).split("/").pop() ?? "",
  ]);
  for (const file of walk(featureDirectory)) {
    const path = repoPath(file);
    if (path.endsWith("package.json")) {
      const manifest = parseJson(file);
      if (isRecord(manifest) && typeof manifest.name === "string")
        tokens.add(manifest.name);
    }
    if (path.endsWith("Cargo.toml")) {
      const text = readText(file);
      const match = text?.match(/^\s*name\s*=\s*"([^"]+)"\s*$/m);
      if (match?.[1] !== undefined) tokens.add(match[1]);
    }
  }
  tokens.delete("");
  return [...tokens];
}

function checkFeatureIsolation(): void {
  const features = join(root, "features");
  if (!existsSync(features)) return;
  const directories = readdirSync(features)
    .map((entry) => join(features, entry))
    .filter(
      (entry) =>
        statSync(entry).isDirectory() &&
        !ignoredDirectories.has(repoPath(entry).split("/").pop() ?? ""),
    );
  const featureData = directories.map((directory) => ({
    directory,
    tokens: featureTokens(directory),
  }));
  for (const current of featureData) {
    for (const file of walk(current.directory)) {
      const path = repoPath(file);
      const sourceOrManifest =
        sourceExtensions.has(path.slice(path.lastIndexOf(".")).toLowerCase()) ||
        path.endsWith("package.json") ||
        path.endsWith("Cargo.toml");
      if (!sourceOrManifest) continue;
      const text = readText(file);
      if (text === undefined) continue;
      for (const other of featureData) {
        if (other.directory === current.directory) continue;
        const otherName = repoPath(other.directory).split("/").pop() ?? "";
        const pathReference = `features/${otherName}`;
        if (
          text.includes(pathReference) ||
          other.tokens.some((token) => hasToken(text, token))
        ) {
          report(
            file,
            `feature ${repoPath(current.directory).split("/").pop() ?? ""} must not reference ${otherName}`,
          );
        }
      }
    }
  }
}

for (const file of walk(root)) {
  const path = repoPath(file);
  if (path.endsWith("package.json")) packageDependencies(file);
  if (path.endsWith("Cargo.toml") || path.endsWith(".rs"))
    cargoUsesRusqlite(file);
  if (/^apps\/[^/]+\/src-tauri\/capabilities\/[^/]+\.json$/.test(path))
    checkCapability(file);
  checkSource(file);
}
checkFeatureIsolation();
checkDesktopNextConfig();

if (errors.length > 0) {
  console.error(
    "Architecture boundary violations:\n" +
      errors.map((error) => `- ${error}`).join("\n"),
  );
  process.exitCode = 1;
} else {
  console.log("Architecture boundary checks passed.");
}
