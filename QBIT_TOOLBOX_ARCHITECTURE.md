# Qbit Toolbox — Architecture & Engineering Source of Truth

> **Status:** Normative  
> **Document role:** Primary architecture and engineering source of truth  
> **Product:** Qbit Toolbox  
> **Architecture style:** Feature-first modular desktop platform  
> **Initial production platform:** Windows  
> **Target platforms:** Windows, macOS, Linux  
> **Desktop shell:** Tauri 2 + Rust  
> **UI:** Next.js static export + React + TypeScript + MUI + MD3 Next  
> **Persistence:** SQLite through Rust (`rusqlite`), with feature-owned stores  
> **Repository:** Monorepo, Cargo workspace + Bun workspace  
> **CI/CD:** GitHub Actions + GitHub Releases  
> **Last architecture review:** 2026-08-10

---

## 0. Authority, Scope, and Change Control

This document defines the default architecture, engineering constraints, delivery model, quality gates, release policy, and non-negotiable runtime rules for Qbit Toolbox.

It is intentionally designed for the **mature product**, not only for the first Hotkeys feature. Development is incremental, but architectural boundaries are long-lived.

### 0.1 Normative language

The terms **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are normative.

- **MUST / MUST NOT**: mandatory unless this document is changed through an approved Architecture Decision Record (ADR).
- **SHOULD / SHOULD NOT**: default rule; deviation requires an explicit reason in the implementing pull request.
- **MAY**: permitted but optional.

### 0.2 Precedence

When documentation conflicts, precedence is:

1. Security incident response instructions for an active incident.
2. This document.
3. Accepted ADRs that explicitly amend a section of this document.
4. Feature specifications.
5. Implementation comments and README files.

An ADR that changes a normative rule MUST update this document in the same pull request. The architecture must not permanently depend on readers discovering a hidden ADR.

### 0.3 What belongs here

This document owns decisions that affect more than one implementation detail, including:

- module and feature boundaries;
- process and runtime topology;
- persistence rules;
- IPC ownership and contracts;
- platform abstraction;
- security boundaries;
- background resource rules;
- UI composition and design-system boundaries;
- testing layers and release gates;
- versioning and release channels;
- CI/CD and supply-chain security;
- architecture evolution rules.

Feature-specific behavior belongs in the feature specification unless it creates a platform-wide contract.

---

# 1. Product Architecture Goals

Qbit Toolbox is a long-lived desktop utilities platform intended to grow toward the breadth of a PowerToys-class product while preserving lower idle cost, strong usability, and maintainable feature isolation.

The architecture MUST optimize for the following priorities, in order:

1. **Correctness and user trust.** A utility that changes keyboard, clipboard, files, windows, or system behavior must fail safely.
2. **Idle efficiency.** Qbit Toolbox is expected to remain running for long periods. Idle CPU, wakeups, memory, background I/O, and unnecessary threads are product-level concerns.
3. **Feature isolation.** Adding a new utility must not progressively turn the application into an interconnected monolith.
4. **Native runtime ownership.** Background/system behavior belongs in Rust/native platform adapters, not in the web UI.
5. **Progressive delivery without architectural rewrites.** A feature can be unimplemented while its eventual integration path remains defined.
6. **Cross-platform evolution.** Windows is first, but platform-neutral code must not become accidentally Windows-specific.
7. **Security by least privilege.** A WebView must never receive broad native capabilities merely because the application is local.
8. **Open-source maintainability.** Repository structure, testing, build reproducibility, contribution rules, and release provenance must remain understandable to external contributors.

---

# 2. Explicit Non-Goals

The following are **not** architectural goals unless a future ADR changes them:

- building a microservice architecture inside a desktop application;
- running every feature in a separate process;
- building a third-party plugin ecosystem before a concrete requirement exists;
- embedding a general scripting language as the primary UX;
- keeping the React/WebView runtime alive solely to power background features;
- using a database as an event bus;
- exposing generic filesystem, shell, SQL, or process APIs to the frontend;
- running the full application permanently with administrator/root privileges;
- forcing all platforms into the lowest common denominator when a platform-specific implementation is better;
- implementing speculative subsystems that have no current caller.

The product is **architecturally prepared** for future capabilities without paying their runtime or maintenance cost before they are needed.

---

# 3. Technology Baseline

## 3.1 Native host

- **Tauri 2** is the desktop application shell.
- **Rust** owns application lifecycle, feature lifecycle, persistence, system integration, IPC command handling, diagnostics, updater orchestration, and background execution.
- Rust uses the **2024 edition** and Cargo workspace resolver 3 unless an upstream compatibility issue requires an ADR-backed temporary exception.
- `rust-toolchain.toml` MUST pin the project toolchain. The project upgrades Rust intentionally; contributor machines must not silently select arbitrary compiler versions.

## 3.2 Frontend

The supported frontend architecture is:

- **Next.js** using **static export only**;
- **React**;
- **TypeScript** with strict mode;
- **MUI**;
- **MD3 Next (`md3-next`)** for Material Design 3 theming and MUI restyling;
- **Bun workspaces** for JavaScript/TypeScript packages.

### Why Next.js instead of Vite

MD3 Next currently documents React 18+, Next.js 13+, and MUI v6+ as prerequisites. Tauri supports Next.js when it is configured as a static export. Therefore Qbit Toolbox uses the upstream-supported MD3 Next environment rather than depending on undocumented Vite compatibility.

This is **not** a server-rendered web application. Qbit Toolbox MUST NOT require a Node.js/Next.js server at runtime.

Required Next.js constraints:

- `output: "export"` MUST be enabled.
- Tauri `frontendDist` MUST point to the generated static output directory.
- Server Actions, API routes, server-only runtime dependencies, and any feature requiring a persistent Next.js server are forbidden.
- Application state and business operations MUST come from Rust IPC or local UI state, not from a fake web backend.

## 3.3 Persistence

- **SQLite** is the default durable relational store.
- Rust accesses SQLite through **`rusqlite`**.
- The frontend MUST NOT issue SQL.
- `tauri-plugin-sql` is not part of the architecture unless a future ADR demonstrates a use case that cannot be served by feature APIs.

## 3.4 Source control and distribution

- GitHub is the canonical source repository.
- GitHub Actions is the canonical CI/CD system.
- GitHub Releases is the initial public binary distribution origin.
- Tauri Updater is the application update client.

## 3.5 Dependency version policy

Architecture documents MUST NOT hardcode ephemeral patch versions unless a specific version is itself an architectural requirement.

Actual build versions are controlled by:

- `Cargo.lock` for Rust;
- `bun.lock` for JavaScript/TypeScript;
- `rust-toolchain.toml` for Rust toolchain;
- the repository's Node version file/tool configuration for Node.js.

Rules:

- lockfiles MUST be committed;
- CI release builds MUST use locked/frozen dependency resolution;
- foundational dependencies SHOULD be stable releases, not alpha/beta/RC packages;
- an RC dependency in a leaf feature MAY be accepted only after risk review;
- an RC dependency MUST NOT become a core architecture dependency without an ADR;
- dependency upgrades require the same tests as source changes;
- security updates may bypass normal batching but not validation.

---

# 4. Monorepo and Feature-First Repository Structure

Qbit Toolbox uses one repository containing Rust and TypeScript workspaces. Feature code is grouped by **feature**, not by global technical layer.

```text
qbit-toolbox/
│
├── apps/
│   └── desktop/
│       ├── app/                         # Next.js App Router composition only
│       │   ├── layout.tsx
│       │   ├── page.tsx
│       │   ├── hotkeys/                 # Thin route adapters
│       │   ├── settings/
│       │   └── about/
│       │
│       ├── public/
│       ├── next.config.ts
│       ├── package.json
│       │
│       └── src-tauri/
│           ├── src/
│           │   ├── main.rs
│           │   ├── bootstrap.rs
│           │   └── app.rs
│           ├── capabilities/
│           ├── icons/
│           ├── tauri.conf.json
│           └── Cargo.toml
│
├── features/
│   ├── hotkeys/
│   │   ├── native/
│   │   │   ├── src/
│   │   │   │   ├── domain/
│   │   │   │   ├── application/
│   │   │   │   ├── runtime/
│   │   │   │   ├── persistence/
│   │   │   │   ├── platform/
│   │   │   │   │   ├── windows/
│   │   │   │   │   ├── macos/          # only when implemented
│   │   │   │   │   └── linux/          # only when implemented
│   │   │   │   ├── ipc/
│   │   │   │   ├── feature.rs
│   │   │   │   └── lib.rs
│   │   │   ├── migrations/
│   │   │   ├── benches/
│   │   │   ├── tests/
│   │   │   └── Cargo.toml
│   │   │
│   │   └── ui/
│   │       ├── src/
│   │       │   ├── components/
│   │       │   ├── pages/
│   │       │   ├── hooks/
│   │       │   ├── api/
│   │       │   ├── model/
│   │       │   ├── state/
│   │       │   └── index.ts
│   │       ├── tests/
│   │       └── package.json
│   │
│   ├── clipboard/                       # future
│   │   ├── native/
│   │   └── ui/
│   │
│   ├── launcher/                        # future
│   │   ├── native/
│   │   └── ui/
│   │
│   └── ...
│
├── crates/                              # shared native foundation only
│   ├── app-runtime/
│   ├── feature-api/
│   ├── persistence/
│   ├── platform/
│   ├── diagnostics/
│   ├── security/
│   ├── ipc-contracts/
│   └── updater/
│
├── packages/                            # shared frontend/build foundation only
│   ├── ui/
│   │   ├── src/theme/
│   │   ├── src/tokens/
│   │   ├── src/components/
│   │   ├── src/layouts/
│   │   └── src/icons/
│   ├── ipc/
│   │   └── generated/                   # generated Rust-owned DTO bindings
│   ├── i18n/
│   └── testing/
│
├── tests/
│   ├── e2e/
│   ├── packaging/
│   ├── updater/
│   ├── performance/
│   └── fixtures/
│
├── docs/
│   ├── architecture/
│   ├── adr/
│   ├── features/
│   ├── security/
│   └── release/
│
├── tools/
│   ├── ci/
│   ├── release/
│   └── dev/
│
├── .github/
│   ├── workflows/
│   ├── CODEOWNERS
│   └── dependabot.yml
│
├── Cargo.toml                           # Rust workspace
├── Cargo.lock
├── bun.lock
├── package.json                           # Bun workspace definition lives here
├── rust-toolchain.toml
├── SECURITY.md
├── CONTRIBUTING.md
└── QBIT_TOOLBOX_ARCHITECTURE.md
```

## 4.1 Repository ownership rules

- `features/<feature>/native` owns native logic unique to that feature.
- `features/<feature>/ui` owns UI logic unique to that feature.
- `crates/*` MUST contain only reusable platform/foundation code.
- `packages/*` MUST contain only reusable frontend/build code.
- A shared module MUST NOT be created merely because two pieces of code look similar. Shared abstractions must represent a stable shared concept.
- There MUST NOT be a generic `utils` dumping ground.
- App route files MUST remain composition adapters; feature behavior belongs in the feature package.

## 4.2 When to create a new shared crate/package

Create a shared crate/package when at least one of the following is true:

1. the code is used by two or more features and represents the same semantic capability;
2. a security boundary benefits from separate lint/dependency rules;
3. an unsafe/native boundary benefits from isolation;
4. it has an independently testable platform contract;
5. it prevents circular dependency pressure between features.

Do not create a crate only to mirror every folder or class.

---

# 5. Dependency Direction and Module Boundaries

The allowed dependency direction is:

```text
Desktop Host
     │
     ▼
Feature API / Core Capabilities
     ▲
     │
Features ───────────────► Shared Foundation

Feature UI ─────────────► packages/ui + packages/ipc
```

## 5.1 Feature independence

A feature MUST NOT import another feature's implementation.

Forbidden:

```text
hotkeys/native -> clipboard/native
clipboard/ui   -> launcher/ui/internal-state
```

Cross-feature behavior must use one of:

- a stable core capability interface;
- a typed event intended for fan-out notification;
- an explicit orchestration service owned by the application host;
- a separately promoted shared abstraction after it is proven to be shared.

## 5.2 Event bus constraints

The internal event bus is for **notifications**, not hidden request/response RPC and not distributed transactions.

Events MUST:

- be typed;
- have explicit ownership;
- use stable semantic names;
- avoid carrying secrets or unbounded payloads;
- be safe for zero listeners;
- not be required to reconstruct critical state unless an event log is explicitly designed later.

A command that requires success/failure MUST use an explicit API, not “emit an event and hope”.

---

# 6. Application Host Architecture

The desktop host is intentionally small. It coordinates features but does not absorb feature business logic.

```text
┌───────────────────────────────────────────────────────────┐
│                     Application Host                      │
│                                                           │
│  Bootstrap                                                │
│  Single Instance Guard                                    │
│  Core Persistence                                         │
│  Feature Registry                                         │
│  Runtime Supervisor                                       │
│  Window / Tray Manager                                    │
│  Security / Capability Policy                             │
│  Diagnostics                                              │
│  Update Manager                                           │
│  Shared Scheduler                                         │
│  Typed Event Bus                                          │
│                                                           │
└──────────────────────┬────────────────────────────────────┘
                       │
      ┌────────────────┼────────────────┐
      ▼                ▼                ▼
   Hotkeys          Clipboard       Future Feature
```

## 6.1 Startup order

The canonical startup sequence is:

1. initialize minimal crash/error reporting infrastructure;
2. acquire single-instance ownership;
3. resolve application directories;
4. open and migrate `core.db`;
5. load core settings and feature enablement metadata;
6. initialize security/capability configuration;
7. construct feature registry;
8. start only enabled startup features;
9. create system tray/menu integration;
10. remain headless unless the user explicitly requests the UI or startup policy requires onboarding/error recovery.

A second process MUST delegate the appropriate activation intent to the first process and exit.

## 6.2 Shutdown order

1. stop accepting new state-changing UI commands;
2. signal feature cancellation;
3. stop isolated workers;
4. stop background feature runtimes;
5. flush critical persistence operations;
6. checkpoint/close databases according to storage policy;
7. persist final core lifecycle state;
8. tear down tray/window resources;
9. exit.

Each graceful-shutdown attempt MUST have a bounded maximum duration. Incomplete or unknown cleanup MUST be diagnosed and return a recoverable result rather than freeze the UI or silently authorize exit. The application retains resource ownership and offers deliberate Quit retry; it MUST NOT automatically force exit while cleanup is unacknowledged.

## 6.3 Activation intents

Application activation is modeled as a typed intent rather than ad-hoc startup arguments. Future activation sources may include:

- ordinary application launch;
- tray/menu action;
- second-instance activation;
- deep link;
- file association;
- command-line invocation;
- updater/recovery restart;
- feature overlay request.

The Application Host translates external inputs into validated `ActivationIntent` values and routes them to the owning core/feature API. Feature internals MUST NOT parse raw process command lines independently when a host-level activation contract can represent the action.

## 6.4 Unclean shutdown recovery

The core may record minimal last-session/clean-shutdown state under the state storage class. On startup after an unclean shutdown, recovery behavior must be conservative:

- do not reset user data automatically;
- perform required SQLite recovery/integrity handling through the persistence layer;
- mark repeatedly failing features as degraded/failed rather than entering crash loops;
- expose actionable diagnostics when recovery cannot complete.

---

# 7. Feature Model

Each feature is a bounded vertical slice with native runtime, optional UI, persistence ownership, platform adapters, tests, and migrations.

## 7.1 Feature descriptor

Every feature MUST expose static metadata equivalent to:

```text
FeatureDescriptor
├── id                       # stable, never reused
├── display metadata key
├── version                  # feature implementation/schema metadata if needed
├── supported platforms
├── runtime mode
├── startup policy
├── capabilities required
├── storage classes required
├── background requirement
├── dependencies             # only core capabilities / explicit feature contracts
├── UI route metadata
└── diagnostics metadata
```

Feature IDs use a stable namespace such as:

```text
qbit.hotkeys
qbit.clipboard
qbit.launcher
```

A feature ID MUST NOT change merely because the product display name changes.

## 7.2 Feature lifecycle

Canonical states:

```text
Unavailable
Disabled
Starting
Running
Degraded
Stopping
Failed
```

Rules:

- `Unavailable`: platform or runtime prerequisites are not met.
- `Disabled`: no feature runtime should be active.
- `Starting`: migrations and runtime initialization are in progress.
- `Running`: expected functionality is healthy.
- `Degraded`: feature is partially operational and exposes a recoverable issue.
- `Failed`: feature cannot safely provide its primary function.

A feature failure SHOULD degrade that feature rather than crash the full application unless continued execution would violate a safety invariant.

## 7.3 Runtime modes

The platform supports three runtime modes:

### A. `EmbeddedBackground`

For low-cost, latency-sensitive background capabilities.

Examples:

- Hotkeys;
- clipboard listener metadata capture;
- native window event listeners.

### B. `EmbeddedOnDemand`

For work that runs only in response to an explicit action and is safe inside the host process.

Examples may include small transforms, parsing, or lightweight file operations.

### C. `IsolatedWorker`

For work where process isolation has concrete value:

- crash-prone native libraries;
- high memory peaks;
- CPU-heavy batch processing;
- third-party binaries;
- workloads requiring a different privilege boundary.

An isolated worker is **not** started merely because the mode exists. It is created only when a feature requires it.

## 7.4 Disabled-feature invariant

A disabled feature MUST have approximately zero ongoing runtime cost:

- no dedicated thread;
- no system hook/listener;
- no recurring timer;
- no open feature database connection;
- no worker process;
- no background network activity;
- no loaded WebView route code beyond unavoidable static application metadata.

Core metadata indicating that a feature is disabled is allowed.

---

# 8. Runtime Supervisor and Failure Isolation

The Runtime Supervisor owns feature activation and deactivation.

Responsibilities:

- lifecycle transitions;
- cancellation propagation;
- runtime health;
- worker process supervision;
- restart policy where safe;
- bounded shutdown;
- feature failure reporting;
- resource registration and cleanup.

## 8.1 Restart rules

Background features MUST NOT enter unbounded restart loops.

If automatic restart is appropriate:

- retries use bounded exponential backoff;
- retry count/window is capped;
- repeated failure moves the feature to `Failed` or `Degraded`;
- the failure is visible in diagnostics;
- user settings/data are not silently reset as a recovery mechanism.

## 8.2 Panic rules

Rust panics are programming defects, not normal error handling.

- recoverable failures MUST use typed errors;
- FFI and OS callback boundaries MUST NOT unwind across the foreign boundary;
- platform callback entry points MUST contain an appropriate panic boundary or otherwise guarantee non-unwinding behavior;
- production code MUST NOT depend on `unwrap()` for external input, I/O, database content, or platform calls;
- `expect()` MAY be used only for a proven internal invariant and MUST include a meaningful invariant message.

---

# 9. Threading, Async, and Scheduling

## 9.1 General rule

Threads are a resource. Each permanent thread must have a documented owner and reason.

Qbit Toolbox SHOULD prefer:

- the Tauri/Rust async runtime for ordinary asynchronous orchestration;
- dedicated OS threads only for APIs that require a message loop, affinity, blocking native contract, or deterministic isolation;
- isolated processes only where process isolation is justified.

## 9.2 Blocking work

Blocking filesystem, SQLite, compression, hashing, or native library calls MUST NOT run on an async executor thread when they can block other tasks.

SQLite access is synchronous through `rusqlite`; persistence operations therefore run through a bounded blocking persistence executor.

The persistence subsystem MUST NOT create a large connection pool or permanent worker thread for every feature by default.

## 9.3 Timers and polling

Polling is a last resort.

Features SHOULD prefer OS events, hooks, watchers, or subscriptions.

Rules:

- recurring sub-second polling requires an ADR or explicit feature performance justification;
- timers SHOULD be coalesced through a shared scheduler where semantics allow it;
- timers MUST be cancellable when a feature is disabled;
- timers MUST NOT exist solely to keep UI state synchronized when an event can be emitted instead.

## 9.4 Cancellation

Every long-lived task MUST have an owner and cancellation path.

Feature disable, application shutdown, and worker termination must not rely on process exit as the only cancellation strategy.

---

# 10. UI Runtime Model

React is a **control plane**, not the execution engine for desktop utilities.

## 10.1 Headless-by-default runtime

Normal background state:

```text
Qbit Toolbox Core
+ enabled native background features
+ native tray integration
- no visible window
- no permanently hidden WebView required for feature execution
```

When the user opens Qbit Toolbox:

1. create/show the Tauri window;
2. load the static Next.js application;
3. load the requested feature route/chunks;
4. query current state from Rust feature APIs.

When the last control UI window is closed, the application SHOULD destroy the WebView/window if doing so is reliable on the target platform and no explicit UI-owned operation remains active.

A hidden WebView MUST NOT be retained merely for convenience.

## 10.2 Next.js usage restrictions

Qbit Toolbox uses Next.js as a static application compiler/router, not as a web server platform.

Forbidden runtime dependencies include:

- API Routes;
- Server Actions;
- runtime SSR;
- dynamic server rendering;
- server sessions;
- server-only database clients;
- Node filesystem/process usage from application pages.

## 10.3 Route ownership

`apps/desktop/app/**/page.tsx` files are thin composition adapters.

Example:

```text
apps/desktop/app/hotkeys/page.tsx
        │
        ▼
features/hotkeys/ui
```

Feature pages/components do not move into the app shell simply because Next.js requires a route file.

## 10.4 Window taxonomy

A mature Toolbox will need more than one kind of window. The Window Manager therefore distinguishes at least:

1. **Control window** — settings/dashboard; on-demand and normally destroyable when closed.
2. **Feature overlay** — launcher, picker, recorder, quick action, or other short-lived always-on-top UI.
3. **Feature utility window** — longer-lived feature-specific UI when justified.
4. **Recovery/onboarding window** — core-owned exceptional UI.

Every window/webview has:

- a stable owner;
- a purpose-specific label;
- an explicit capability set;
- a creation/destruction lifecycle;
- an activation policy.

Wildcard capabilities across every future window are forbidden. Opening an overlay must not implicitly grant it the settings window's permissions.

## 10.5 Monorepo transpilation

The desktop Next.js app imports workspace UI packages. `next.config.ts` MUST explicitly configure any workspace packages that require Next.js transpilation. This is build configuration, not a reason to move feature source into `apps/desktop`.

---

# 11. Material Design 3 and UI System

## 11.1 UI stack ownership

`packages/ui` owns the global design system:

```text
packages/ui/
├── theme/
│   ├── md3-theme.ts
│   ├── provider.tsx
│   ├── mode.ts
│   └── system-preference.ts
├── tokens/
├── components/
├── layouts/
├── motion/
└── icons/
```

MD3 Next MUST be initialized centrally.

Feature packages MUST NOT create independent MD3 themes.

## 11.2 Dependency boundary

Feature UI code:

- MAY import stable MUI primitives directly;
- SHOULD use `packages/ui` for Qbit-specific composite patterns;
- MUST NOT initialize or configure `md3-next` directly;
- SHOULD NOT import `md3-next` except inside the design-system package unless a documented API cannot otherwise be exposed cleanly.

This keeps the product visually consistent and limits dependency blast radius if the MD3 implementation changes in the future.

## 11.3 Avoid pointless wrapper components

Do not create `QbitButton`, `QbitCheckbox`, `QbitTextField`, etc. merely to mirror every MUI primitive.

Shared components should represent a Qbit interaction or composition, for example:

- `FeatureCard`;
- `SettingsSection`;
- `SettingsRow`;
- `ShortcutBadge`;
- `PermissionNotice`;
- `FeatureStatusBanner`;
- `EmptyState`;
- standard destructive-action confirmation patterns.

## 11.4 Theme requirements

The UI MUST support:

- Light;
- Dark;
- System preference.

Qbit brand color(s) generate the MD3 color scheme through the central theme definition.

Raw feature-specific color constants SHOULD NOT be used for ordinary UI state. Semantic theme tokens are required.

## 11.5 Accessibility

The UI target is **WCAG 2.2 AA** for applicable desktop-web interactions.

At minimum:

- all actions are keyboard reachable;
- focus state is visible;
- dialogs manage focus correctly;
- color is not the only state indicator;
- text and control contrast meets target requirements;
- screen-reader labels are provided for icon-only controls;
- reduced-motion preference is respected;
- shortcut capture UI has an accessible textual representation.

Accessibility regressions are release defects.

## 11.6 Internationalization and RTL

The architecture MUST support localization from the beginning even if the first release has a small language set.

Rules:

- user-facing strings SHOULD be localization keys rather than scattered literals;
- UI layout must tolerate text expansion;
- RTL direction MUST be supported by the design system and tested before declaring an RTL locale supported;
- locale formatting must be used for dates/numbers shown to users;
- internal IDs, logs, and persisted enum values MUST NOT depend on translated strings.

## 11.7 Large data UI

Features such as clipboard history, launcher indexes, logs, or file tools may expose large collections.

Such UIs MUST use pagination/windowing/virtualization as appropriate and MUST NOT render unbounded datasets into the DOM.

---

# 12. IPC and Contract Architecture

IPC is a security and compatibility boundary.

## 12.1 Direction

```text
Feature UI
   ↓ typed feature client
Tauri invoke/event boundary
   ↓
Rust feature IPC adapter
   ↓
Application/domain services
```

The UI MUST NOT call persistence or platform adapters directly.

## 12.2 Rust-owned DTOs

Rust is the source of truth for serialized command/event DTOs.

TypeScript type declarations are generated from Rust-owned DTOs using a stable binding generator such as `ts-rs`.

Generated bindings live under:

```text
packages/ipc/generated/
```

Generated files MUST NOT be hand-edited.

CI MUST fail when generated bindings are stale relative to Rust source.

## 12.3 Command organization

Commands are explicit and feature-scoped.

Good:

```text
hotkeys_list_mappings
hotkeys_create_mapping
hotkeys_update_mapping
hotkeys_delete_mapping
hotkeys_set_enabled
hotkeys_get_runtime_status
```

Forbidden generic dispatcher:

```text
execute(feature, action, arbitrary_json)
```

A generic dispatcher destroys discoverability, capability granularity, contract testing, and type safety.

## 12.4 API wrappers

Each feature UI owns a small client adapter in:

```text
features/<feature>/ui/src/api/
```

UI components SHOULD NOT contain raw `invoke()` string calls.

## 12.5 Error contract

User-visible command failures use a stable structured error envelope containing at least:

```text
code
category
message_key or safe display message
recoverable
context (non-sensitive, bounded)
```

Internal source errors may be chained in Rust/logs, but raw platform/database errors MUST NOT be dumped directly into the user UI.

Error codes are stable identifiers and MUST NOT be translated.

## 12.6 Payload size

IPC MUST NOT be used to repeatedly transfer very large binary payloads if a file handle/path token, chunked transport, or dedicated mechanism is more appropriate.

Features introducing large payloads must define transfer and memory limits in their feature specification.

## 12.7 IPC compatibility policy

The main Rust host and bundled frontend are released together, so Qbit Toolbox does not promise backward compatibility for private in-process UI IPC across unrelated application versions. It **does** require deterministic generated contracts within a build.

Longer-lived compatibility contracts are different and MUST be explicitly versioned, including:

- persisted data schemas;
- import/export formats;
- updater metadata;
- sidecar/worker protocols;
- future external plugin APIs;
- future deep-link/CLI public contracts where users may automate against them.

Do not introduce compatibility machinery for private UI IPC that is not actually needed, and do not confuse that with permission to break durable/public formats.

---

# 13. Persistence Architecture

Persistence is designed for a mature multi-feature product.

## 13.1 Storage classes

The platform recognizes these logical storage classes:

1. **Core relational state** — application-level settings and feature registry metadata.
2. **Feature relational state** — feature-owned SQLite database.
3. **Feature blob data** — feature-owned files for large/binary content.
4. **Cache/rebuildable data** — disposable data that can be regenerated.
5. **Secrets** — credential/key storage behind a dedicated secret-store abstraction.

A feature declares only the storage classes it needs.

## 13.2 Database layout

Canonical logical layout:

```text
<AppData>/Qbit/Toolbox/
├── data/
│   ├── core.db
│   └── features/
│       ├── qbit.hotkeys/
│       │   ├── data.db
│       │   └── blobs/
│       ├── qbit.clipboard/
│       │   ├── data.db
│       │   └── blobs/
│       └── ...
├── cache/
│   └── features/
├── backups/
├── logs/
└── state/
```

Actual platform paths are resolved through the platform path abstraction; feature code must not hardcode Windows AppData paths.

## 13.3 `core.db`

`core.db` contains only application-wide state, such as:

- feature enabled/disabled state;
- application preferences;
- onboarding state;
- update channel preference;
- global UI preferences where they are not purely ephemeral;
- feature registry metadata;
- core schema metadata.

`core.db` MUST NOT become a dumping ground for feature tables.

## 13.4 Feature-owned databases

A feature that needs durable relational persistence owns its own database and schema.

Benefits intended by this boundary:

- independent migrations;
- failure isolation;
- independent retention policies;
- independent backup/reset;
- separation of high-volume data from small settings;
- ability to remove a feature's data without manipulating unrelated tables.

Feature databases MUST NOT query another feature database directly.

## 13.5 No cross-database transaction assumption

Qbit Toolbox does **not** assume atomic transactions across feature databases.

Cross-feature workflows MUST NOT require a multi-database atomic commit. If such a requirement emerges, it requires an explicit architecture design rather than silently attaching databases and relying on SQLite details that vary with journal mode.

Cross-feature operations should use idempotent orchestration and explicit recovery states.

## 13.6 Persistence executor

All database operations flow through the Rust persistence layer.

```text
Feature Repository
      ↓
Persistence Service
      ↓
bounded blocking executor
      ↓
rusqlite / SQLite
```

Forbidden in latency-sensitive callbacks:

- SQL queries;
- opening/closing files;
- migrations;
- checkpoints;
- logging to disk.

## 13.7 Connection lifecycle

- feature databases are opened lazily when the feature/runtime or explicit UI operation requires them;
- disabling a feature SHOULD release feature-owned long-lived DB resources;
- the architecture does not use a large per-feature connection pool by default;
- pools may be added only where measured concurrency requires them.

## 13.8 SQLite durability profiles

Persistence exposes named policies instead of scattering PRAGMA decisions across features.

### `Critical`

For configuration/state where a committed change is expected to survive power loss.

Baseline policy SHOULD use WAL with `synchronous=FULL` or another profile demonstrated to provide the required durability on supported filesystems.

### `Standard`

For ordinary durable application state where corruption resistance is required but the latest few committed changes may be acceptable to lose after sudden power loss.

Baseline policy MAY use WAL with `synchronous=NORMAL`.

### `Rebuildable`

For caches/indexes that can be regenerated. These should normally live under the cache storage class rather than weakening durability of user data databases.

Features MUST select a named policy; they MUST NOT invent ad-hoc PRAGMAs without persistence-layer review.

## 13.9 Schema migrations

Every persistent database has an explicit schema version and migration history.

Migration rules:

- migrations are **forward-only** in shipped application code;
- migration declarations/plans MUST use positive, strictly increasing unique versions and non-empty stable names, and the shared persistence layer MUST validate the plan before creating/updating migration metadata or applying migration SQL;
- migrations MUST be deterministic;
- migrations MUST be tested against fixtures from previously released schema versions;
- transactional migrations SHOULD run in one transaction where SQLite permits it;
- destructive migrations require a pre-migration backup/snapshot strategy;
- a failed feature migration fails that feature startup, not unrelated features;
- a failed `core.db` migration places the application into a controlled recovery mode;
- the application MUST NOT silently delete/recreate a user database because migration failed;
- schema downgrade is not automatic.

## 13.10 Update compatibility window

Where feasible, schema changes SHOULD remain readable by the immediately previous stable application version for one release window, especially for critical settings.

If a migration makes rollback impossible, the release notes and migration metadata MUST make that explicit, and the release process MUST ensure a recoverable pre-migration backup exists where practical.

## 13.11 Backup and restore

A future global backup/import feature is anticipated by the storage boundaries.

Backup coordination MUST quiesce or snapshot feature stores consistently enough for the backup contract being offered. Simply copying live database files without respecting SQLite journal/WAL state is forbidden.

A feature MAY define its own export format when user portability is useful, but that export format is separate from an internal database backup.

## 13.12 Large blobs

Large binary objects SHOULD live in feature-owned blob storage, referenced by stable metadata in SQLite.

Examples:

- large clipboard images;
- OCR source artifacts;
- thumbnails;
- cached binary indexes.

Database BLOB storage MAY be used for small bounded values, but a feature must define limits.

## 13.13 Secrets

Secrets MUST NOT be stored as plaintext configuration values in SQLite.

The architecture reserves a `SecretStore` interface backed by the appropriate secure platform mechanism or a vetted cross-platform secure store when the first secret-bearing feature is introduced.

Secret-store implementation is not required before there is a real secret consumer.

---

# 14. Data Modeling Conventions

## 14.1 Stable identifiers

Persisted entities that may need stable references, export/import, or future synchronization SHOULD use opaque stable IDs rather than array positions or display names.

IDs MUST NOT encode translated names or mutable paths unless the path itself is the identity.

## 14.2 Time

- persisted absolute timestamps use UTC;
- UI converts to locale/time zone at presentation time;
- durations and runtime latency use monotonic clocks where available;
- business logic MUST NOT use formatted display strings as timestamps.

## 14.3 Enumerations

Persisted enum/string discriminants are stable machine identifiers.

Renaming a UI label MUST NOT require rewriting persisted enum values.

## 14.4 Optional future synchronization

Cloud sync is not implemented by default, but the persistence model avoids assumptions that would make it impossible later:

- entities use stable IDs;
- feature schemas have explicit versions;
- feature ownership is clear;
- mutation timestamps/version fields can be added without replacing identity;
- secrets remain outside ordinary syncable state.

Any actual sync protocol requires a separate ADR covering conflict resolution, encryption, privacy, and offline semantics.

---

# 15. Platform Abstraction and Cross-Platform Strategy

Windows is the first production platform. The architecture is not Windows-only.

## 15.1 Separation rule

Platform-neutral domain/application code MUST NOT import Win32/macOS/Linux API types.

Feature layout:

```text
features/<feature>/native/src/
├── domain/
├── application/
├── runtime/
└── platform/
    ├── windows/
    ├── macos/
    └── linux/
```

Only implemented platform folders are required to contain code.

Do not create dummy macOS/Linux implementations solely to make the tree look complete.

## 15.2 Capability over parity

Feature support is represented as a platform capability matrix.

The UI must distinguish:

- supported;
- supported with limitations;
- unavailable.

The product SHOULD use native platform strengths rather than implementing the lowest common denominator just to claim identical behavior.

## 15.3 Shared platform crate

`crates/platform` owns truly shared OS abstractions such as:

- application directories;
- process/window identity primitives where generic;
- OS version/capability detection;
- safe common wrappers used by multiple features.

Feature-specific OS behavior remains inside the feature.

Example: keyboard hook implementation belongs to Hotkeys, not a giant generic `platform/windows` module.

## 15.4 OS session and power lifecycle

The mature product must tolerate desktop lifecycle changes. The platform layer exposes typed lifecycle notifications/capabilities for events that matter to enabled features, such as:

- sleep/suspend and resume;
- user session lock/unlock;
- session/logon changes where supported;
- display/topology changes;
- device/input changes when required by a feature;
- shell/tray recreation conditions where applicable.

Features subscribe only to events they need. The host MUST NOT create polling loops to synthesize lifecycle events that the OS can provide natively.

A feature affected by suspend/resume or session changes must include those paths in its integration/manual validation plan.

---

# 16. Security Architecture

Qbit Toolbox manipulates OS-level behavior, so local-only does not mean trusted-by-default.

## 16.1 Trust boundaries

Treat these as separate trust zones:

1. frontend WebView;
2. Rust application host;
3. feature native code;
4. OS APIs;
5. worker/sidecar processes;
6. update/download origin;
7. user-selected files/URLs/data.

## 16.2 Frontend least privilege

The WebView MUST NOT receive broad generic permissions for:

- unrestricted filesystem access;
- arbitrary process spawning;
- arbitrary shell commands;
- raw SQL;
- unrestricted HTTP/networking;
- secret storage internals.

Tauri capabilities MUST be minimal and scoped to the windows/webviews that require them.

Custom Tauri commands MUST also be treated as privileged APIs and kept feature-specific.

## 16.3 Content policy

Production UI SHOULD load local bundled application assets only.

Remote script execution, CDN-hosted application JavaScript, `eval`-style code execution, and arbitrary navigation inside the trusted application WebView are forbidden unless a reviewed requirement explicitly changes the threat model.

External links SHOULD open through a validated external opener rather than navigating the application shell.

## 16.4 Runtime style CSP nonce propagation

Tauri asset CSP modification injects cryptographic nonces/hashes into bundled assets; production MUST keep this protection enabled unless a security ADR explicitly proves another design.

Runtime CSS-in-JS engines (currently Emotion via MUI/MD3 Next) MUST create dynamically inserted `<style>` elements with a nonce already authorized by the current Tauri document CSP.

The frontend integration layer MUST read the nonce from the DOM `nonce` property of a Tauri-nonced bundled style/script and pass it into the Emotion cache at cache creation time. Never use `getAttribute("nonce")` as the source because browsers may hide nonce values there.

The nonce is per-document security metadata: it MUST NOT be hard-coded, persisted, logged, sent over IPC, or surfaced to UI/telemetry.

Do not weaken CSP (`dangerousDisableAssetCspModification`, wildcard sources, or broadening inline policy) to make dynamic styling work. Fix nonce propagation at the styling integration boundary.

For the current Next App Router stack, `AppRouterCacheProvider`/Emotion cache owns this integration; MD3 feature/app components MUST NOT manage CSP or custom Emotion caches themselves.

Production validation MUST cover both static placement (Emotion SSR styles in `<head>`) and runtime insertion under real Tauri CSP; a theme switch must prove that newly generated stylesheets are active, not merely that UI preference state changed.

## 16.5 Privilege policy

The main application MUST run as a normal user by default.

It MUST NOT request permanent Administrator/root access merely to make some feature interactions easier.

If a future operation genuinely requires elevation:

- elevation is scoped to the operation or a narrowly defined broker;
- the privileged component has a separate threat model;
- IPC to the privileged component is authenticated/validated as appropriate;
- the full UI process does not inherit elevation unnecessarily.

## 16.6 Unsafe Rust

Unsafe code is allowed only when required for OS/FFI/native performance boundaries.

Policy:

- safe crates SHOULD deny unsafe code;
- crates/modules containing required unsafe platform code MUST isolate it tightly;
- every non-trivial unsafe block MUST contain a `SAFETY:` justification describing the invariant;
- unsafe boundaries require focused tests and reviewer attention;
- business/domain/application layers MUST remain safe Rust.

## 16.7 Input validation

All IPC, file, URL, imported configuration, and platform-derived untrusted values must be validated before they reach sensitive operations.

Validation belongs at the boundary plus domain invariants, not only in UI controls.

## 16.8 Supply-chain security

- GitHub Actions from external repositories MUST be pinned to full immutable commit SHAs.
- Workflow permissions MUST use least privilege.
- Secrets MUST be stored in GitHub encrypted secret/environment facilities, never committed.
- Dependency vulnerability alerts and automated updates MUST be enabled.
- CodeQL SHOULD scan Rust, JavaScript/TypeScript, and GitHub Actions workflows.
- Release artifacts SHOULD include build provenance attestations.
- Release artifacts SHOULD include an SBOM as the release process matures; stable public releases SHOULD ultimately make this mandatory.
- Windows production artifacts MUST be code signed before stable public distribution.
- Tauri updater packages MUST use updater signing.

---

# 17. Privacy and Telemetry

Qbit Toolbox is a utility platform with potentially sensitive access.

## 17.1 Default privacy posture

- no keystroke content is logged;
- clipboard content is not logged by diagnostics;
- file contents are not logged;
- credentials/tokens are never logged;
- paths are treated as potentially sensitive and should be redacted where full path detail is unnecessary;
- telemetry/network reporting is not assumed as a core requirement.

If telemetry is added later, it requires an ADR and privacy specification covering:

- exact fields;
- purpose;
- retention;
- opt-in/opt-out behavior;
- network endpoint;
- failure behavior;
- user-visible disclosure.

Telemetry MUST NOT become a requirement for core functionality.

---

# 18. Diagnostics and Observability

Observability must help diagnose failures without turning background utilities into high-I/O logging systems.

## 18.1 Structured logging

Logs SHOULD be structured with fields such as:

```text
timestamp
level
component
feature_id
event
error_code
operation_id (where applicable)
```

Per-event hot-path logging is forbidden for high-frequency inputs.

## 18.2 Log policy

Log:

- feature lifecycle transitions;
- migration failures;
- hook/listener registration failures;
- updater failures;
- worker crashes;
- unexpected persistence errors;
- security-relevant denied operations when useful and safe.

Do not log:

- every key press;
- every clipboard event payload;
- every successful SQL statement;
- heartbeat messages with no diagnostic value.

## 18.3 In-memory metrics

High-frequency runtimes MAY maintain cheap counters/gauges, preferably atomics, such as:

```text
events_seen
events_matched
actions_emitted
errors
queue_depth
dropped_events
```

Metrics are sampled when diagnostics are requested; they are not continuously persisted.

## 18.4 Diagnostics bundle

The architecture anticipates a future user-exportable diagnostics bundle containing safe metadata such as:

- app version/build SHA;
- OS version;
- feature states;
- feature versions/schema versions;
- redacted logs;
- migration status;
- updater status;
- resource snapshot.

Sensitive user content must be excluded by default.

---

# 19. Performance and Resource Engineering

Performance is a release quality dimension, not a cleanup task.

## 19.1 Hard invariants

The following are mandatory regardless of benchmark numbers:

- no WebView is required for background feature execution;
- disabled features have near-zero ongoing cost;
- no DB/filesystem/IPC/logging in latency-critical keyboard hook callbacks;
- no unbounded queues;
- no unbounded caches;
- no accidental permanent worker process for an on-demand feature;
- no tight polling loop where an event-driven API exists;
- no network activity at idle unless a documented update/online feature is enabled and scheduled;
- every background timer/listener has an owner and shutdown path.

## 19.2 Quantitative budgets

Absolute resource numbers depend on reference hardware, OS, WebView version, enabled features, and measurement tooling. Therefore fabricated numbers are not normative before baseline measurement.

Before the first public stable release, the project MUST establish a versioned reference performance baseline containing at least:

- idle CPU over a defined measurement window;
- process working set/private memory with UI closed;
- process memory with settings UI open;
- idle wakeups/timer activity where measurable;
- cold and warm UI-open latency;
- feature enable/disable latency;
- hotkey callback processing latency;
- shortcut end-to-end latency;
- database migration time on representative fixture sizes;
- application startup-to-background-ready time.

The baseline belongs under:

```text
tests/performance/baselines/
```

A release MUST NOT accept a material regression without explanation and approval. Once baselines exist, each metric receives explicit thresholds rather than informal “fast enough” judgment.

## 19.3 Feature budgets

Each always-on feature specification MUST document:

- permanent threads;
- listeners/hooks;
- recurring timers;
- expected idle CPU behavior;
- expected memory footprint class;
- queue/cache limits;
- disk retention policy;
- network behavior.

---

# 20. Hotkeys — First Vertical Slice

Hotkeys is the first feature implemented through the platform architecture, not a special-case subsystem embedded in the host.

## 20.1 Initial native architecture

Windows implementation:

```text
WH_KEYBOARD_LL
      ↓
minimal keyboard state update
      ↓
compiled immutable keymap lookup
      ↓
match / consume decision
      ↓
bounded action handoff
      ↓
keystroke planner
      ↓
SendInput
```

## 20.2 Hot-path invariant

The low-level keyboard callback MUST NOT perform:

- SQLite access;
- filesystem access;
- Tauri IPC;
- React/UI work;
- synchronous logging;
- network work;
- blocking lock acquisition;
- configuration parsing;
- expensive allocation;
- process/window discovery unless a later scoped implementation can prove bounded safe lookup behavior.

## 20.3 Configuration flow

```text
Feature DB
   ↓ load/update
Domain mappings
   ↓ validate
Conflict analysis
   ↓
Compile
   ↓
Immutable CompiledKeymap
   ↓ atomic publication
Hook thread reads snapshot
```

Runtime lookup never queries the database.

## 20.4 Domain extensibility

The Hotkeys domain must not model the world as only `sourceShortcut -> targetShortcut`.

Conceptual model:

```text
Mapping
├── id
├── enabled
├── trigger
├── action
├── scope
├── behavior
└── metadata
```

Initial actions may be:

```text
EmitShortcut
Disable
```

The model must be able to evolve toward actions such as:

- emit key;
- launch application;
- open URL;
- media/system command;
- text action;
- macro/sequence;
- feature command.

Future behavior is not implemented until specified.

## 20.5 Scope extensibility

Initial scope can be global, while the domain remains capable of future scoped matching such as application/process/window context.

## 20.6 Physical/logical key identity

The domain must be able to distinguish logical key identity from physical/scan-code identity, and must preserve left/right modifier semantics where required.

Default UX should remain simple and hide technical distinctions unless the user opts into advanced behavior.

## 20.7 Injected input recursion

Qbit-generated input must be identifiable and must not recursively trigger Qbit remapping.

Third-party injected input policy must remain separately configurable/designable; do not conflate “injected” with “generated by Qbit”.

## 20.8 Keystroke planning

Shortcut-to-shortcut remapping requires explicit modifier transition planning. The engine must account for physically held modifiers when generating the target chord.

This planner is domain/runtime logic and receives extensive deterministic unit/property coverage.

## 20.9 Implemented core invariants

- Semantic mappings compile into a Windows-native immutable keymap and publication is an atomic `ArcSwap` replacement. A failed native compile leaves the previously published map active.
- Generic modifier requirements expand during configuration compilation to exact left, right, and both-side aliases. Overlapping generic and sided aliases in one identity space are blocking conflicts.
- Physical trigger lookup wins when physical and logical mappings both match an event.
- Action handoff is bounded and preallocated and fails open: if enqueueing fails, the original physical input is not suppressed.
- Qbit-generated input and third-party injected input are distinct origins. The v1 policy bypasses both for remapping and modifier state; Qbit's extra-info tag identifies recursion only and is not a security boundary.
- Output modifier planning uses physical modifier state at worker execution time, not a trigger-time snapshot.

## 20.10 Windows lifecycle and configuration ownership

- When enabled, Hotkeys owns exactly one hook/message-loop thread and one parked output worker. The preallocated action queue holds at most 64 requests. A disabled feature owns neither thread, hook, timer nor persistent feature database connection.
- The callback uses thread-local, nonblocking reentrancy guards, validates raw codes, and passes injected events without modifying physical modifier state. Failed enqueue passes the source down/repeat/up until the next physical press. A consumed key-up remains consumed across map publication/removal.
- Only the Windows `ffi` module may allow unsafe code. Hook installation/unhooking, OS message processing, callback-pointer copying, and SendInput carry explicit lifetime/ABI safety arguments. The callback contains a no-unwind boundary and fails open.
- The output worker reads current atomic physical modifiers at execution, creates a complete fixed-capacity batch, and tags every generated event. A short send is an injection failure and is never blindly retried. UIPI failure cannot be inferred from GetLastError.
- `hook_active` records acknowledged installation and known shutdown, not proof against Windows silently removing a timed-out low-level hook. There is no polling-based health claim; re-enable is an explicit recovery path.
- Application configuration uses one shared bounded blocking persistence lane (32 queued jobs). Feature mutations serialize complete-state validation/native compilation, transactional persistence, then infallible atomic publication. Failed validation or persistence leaves the last published map intact; restart reloads committed database truth after a crash between commit and publication.
- Core feature enablement defaults to disabled. The core owns desired state; Hotkeys owns actual runtime and feature data. Failed core enablement writes compensate by stopping a newly started runtime. Failed disablement writes retain desired state and expose failure after stopping safely. Core recovery rejects all feature operations.
- Quit closes command admission permanently for that session, drains accepted configuration operations in FIFO order, requires explicit feature cleanup acknowledgement, releases connections, and only then permits application exit. A failed cleanup retains the lane/service/native owners for a deliberate tray `Retry Quit`; a timed-out wait retains the same in-flight attempt. Retries cannot overlap cleanup or reopen mutations. A failed stop signal is reported; a native stop timeout retains its pending acknowledgement and join handle. Failed unhook retains the hook/TLS/message thread, with callback cancellation already signaled, until an explicitly requested retry succeeds. No polling recovery or automatic forced exit is permitted.
- The app-global Windows autostart preference defaults off, lives in core settings (forward migration 2), and uses the supported Tauri 2 autostart plugin. Persisted intent wins during startup reconciliation. OS changes are verified and compensated on persistence failure. Autostart is independent of Hotkeys enablement.
- UI capture uses real focused KeyboardEvents and generic modifiers. Letters follow `event.key`; numpad/special keys use reliable location/code distinctions. Dead, composing, AltGraph, non-Latin and ambiguous punctuation input is rejected instead of guessing. Escape cancels capture; repeats and modifier-only events do not save a chord. Native analysis remains the save authority.
- Windows desktop E2E uses a separately identified `desktop-e2e` build, a fail-closed temporary data root and the official external tauri-driver path. No test override or driver is present in production builds. Normal tests never install a real hook or emit OS shortcuts; the lifecycle probe is opt-in.

---

# 21. Testing Strategy

Testing is layered. Coverage percentage alone is not a quality strategy.

Every feature must implement every **meaningful** layer below. Omitting a layer requires a written justification in the feature specification or pull request.

## 21.1 Rust unit tests

Scope:

- domain invariants;
- state machines;
- validators;
- parsers;
- mapping compilers;
- conflict analyzers;
- orchestration decisions that do not require real OS integration;
- serialization compatibility;
- error mapping.

Properties:

- fast;
- deterministic;
- parallel-safe unless explicitly testing synchronization;
- no dependence on user machine state.

## 21.2 Frontend unit tests

Use a modern unit runner such as Vitest.

Scope:

- pure hooks/state logic;
- formatting;
- reducers/stores;
- feature client mapping;
- form validation;
- error presentation decisions.

Tauri frontend APIs should be mocked through the supported Tauri mock facilities where relevant.

## 21.3 Component tests

Scope:

- user interaction with feature components;
- keyboard accessibility;
- disabled/loading/error states;
- form behavior;
- dialogs;
- focus management;
- large-list behavior where applicable.

Prefer tests that interact through user-observable behavior rather than internal React implementation details.

## 21.4 Contract tests

Contract tests are mandatory for the Rust ↔ TypeScript boundary.

They verify:

- generated TypeScript bindings match Rust DTOs;
- generated files have no drift;
- serialized enum tags/field names remain compatible;
- representative request/response payloads round-trip;
- stable error codes remain stable where required.

## 21.5 Persistence integration tests

Use real SQLite databases in isolated temporary directories.

Test:

- repository behavior;
- transactions;
- constraints;
- indexing-sensitive behavior when relevant;
- connection lifecycle;
- corruption/error propagation paths where practical;
- migration behavior.

Do not mock SQLite for tests whose purpose is persistence correctness.

## 21.6 Migration tests

For every released schema version still supported as an upgrade source, maintain representative fixtures.

Tests MUST verify:

- old schema → current schema migration;
- data preservation;
- invariant preservation;
- failure behavior;
- migration on realistic dataset sizes where size matters.

A migration is not considered tested merely because it succeeds on an empty database.

## 21.7 Platform integration tests

Exercise real OS APIs behind the platform adapter.

Examples for Windows Hotkeys:

- hook registration/unregistration;
- keyboard event classification;
- input injection;
- enable/disable lifecycle;
- injected-event filtering;
- behavior across ordinary foreground applications where automation permits.

Platform tests must clearly separate assumptions that cannot run in headless CI.

## 21.8 Desktop end-to-end tests

Use WebdriverIO with the supported Tauri service/embedded WebDriver path for real desktop E2E.

E2E verifies complete user journeys across UI + IPC + Rust + persistence.

Examples:

- open Toolbox;
- enable feature;
- create mapping;
- close/reopen settings;
- verify persistence;
- disable feature;
- verify runtime status;
- restart application;
- verify startup restoration.

E2E tests are behavior tests, not screenshot-only tests.

## 21.9 Packaging tests

Release packaging must be tested separately from application E2E.

Verify:

- installer launches;
- install completes;
- expected files/registrations exist;
- first launch works;
- uninstall works;
- user data retention/removal behavior matches documented product policy;
- upgrade install from at least the previous stable version works.

## 21.10 Updater tests

Updater tests verify:

- stable/beta/nightly channel selection;
- update manifest parsing;
- update signature validation;
- upgrade from previous stable;
- no update when already current;
- corrupt/invalid update is rejected;
- network failure is recoverable;
- DB migrations after update behave correctly.

## 21.11 Performance tests

Performance tests include:

- microbenchmarks for hot-path algorithms;
- startup benchmark;
- idle resource benchmark;
- UI-open benchmark;
- representative database benchmark;
- high-volume list benchmark for data-heavy features;
- worker startup/shutdown benchmark where workers exist.

Performance tests must use repeatable fixtures and record environment metadata.

## 21.12 Soak/endurance tests

Nightly or scheduled validation SHOULD include long-running tests for always-on features:

- extended idle runtime;
- repeated enable/disable cycles;
- repeated UI open/close;
- sustained event input;
- memory growth detection;
- handle/thread leak detection;
- database growth/retention behavior.

## 21.13 Concurrency tests

Features with concurrent state MUST test:

- cancellation during operation;
- shutdown races;
- simultaneous read/write paths;
- bounded queue overflow behavior;
- duplicate event/idempotency behavior where applicable.

## 21.14 Property-based tests

Property tests SHOULD be used for state machines and combinatorial logic where examples alone are weak.

Hotkeys candidates:

- chord normalization;
- modifier transition planner;
- mapping graph/cycle analysis;
- press/release state sequences.

## 21.15 Fuzz tests

Fuzzing SHOULD be introduced for parsers and untrusted input surfaces with meaningful attack/robustness value, including future import formats, protocol parsers, or binary formats.

Fuzzing is not mandatory for simple UI DTOs with no parsing complexity.

## 21.16 Security tests

Security validation includes:

- dependency advisory scanning;
- CodeQL/static analysis;
- capability review;
- unsafe-code review;
- path/URL validation tests;
- command authorization/scope tests where relevant;
- malicious/oversized imported data tests for import features.

## 21.17 Accessibility tests

Automated checks SHOULD use an accessibility engine such as axe for applicable UI.

Manual keyboard and screen-reader-focused validation remains required for major interactive workflows because automated checks are incomplete.

## 21.18 Snapshot/visual tests

Visual regression tests MAY be used for the shared design system and stable complex layouts.

They MUST NOT become the primary assertion mechanism for business behavior.

---

# 22. CI Strategy

CI is split by purpose so fast feedback does not require every expensive test on every commit while release quality remains comprehensive.

## 22.1 Pull request gate

Every PR MUST run applicable fast/medium checks:

### Repository hygiene

- formatting;
- linting;
- generated binding drift;
- migration metadata checks;
- forbidden dependency/boundary checks where tooling exists.

### Rust

- `cargo fmt --check`;
- `cargo clippy` with project lint policy;
- unit tests;
- integration tests that do not require interactive desktop state;
- compile checks for relevant targets/configurations.

### Frontend

- formatting;
- lint;
- TypeScript strict typecheck;
- unit tests;
- component tests;
- static Next.js export build.

### Security

- dependency advisory/policy checks;
- secret scanning as provided by GitHub;
- CodeQL workflow according to repository configuration.

### Windows application

While Windows is the shipping platform, every PR that changes the desktop host or native feature runtime MUST at minimum produce a Windows build/check artifact in CI.

## 22.2 Cross-platform portability lane

Even before macOS/Linux are public releases, platform-neutral crates SHOULD be compiled/tested on Windows, macOS, and Linux in CI where practical.

Purpose: prevent “Windows first” from silently becoming “Windows embedded in every domain type”.

Unsupported platform-specific feature code remains excluded through explicit `cfg` boundaries.

## 22.3 Nightly/scheduled CI

Scheduled CI runs expensive checks such as:

- full desktop E2E;
- long-running/soak tests;
- performance regression suite;
- broader cross-platform builds;
- packaging smoke tests;
- dependency freshness/security review jobs;
- selected fuzz/property campaigns.

## 22.4 Release candidate CI

A release candidate pipeline MUST run the full required matrix for the target release platform, including:

- clean locked build;
- all unit/integration/component/contract tests;
- migrations from supported previous versions;
- real desktop E2E;
- packaging tests;
- updater tests;
- security scanning;
- performance acceptance;
- code signing;
- updater signing;
- artifact hashes;
- build provenance attestation;
- SBOM when required by the release maturity policy.

---

# 23. GitHub Actions Security

## 23.1 Action pinning

Third-party and GitHub-authored actions used in security-sensitive/release workflows MUST be pinned to full commit SHAs rather than mutable tags.

A comment MAY record the human-readable release tag next to the SHA for maintainability.

## 23.2 Workflow permissions

Every workflow MUST set the minimum `GITHUB_TOKEN` permissions needed for its jobs.

`write-all` is forbidden unless there is an exceptional, documented reason.

## 23.3 CODEOWNERS

Sensitive areas SHOULD have explicit ownership/review requirements, including:

- `.github/workflows/**`;
- update/signing configuration;
- Tauri capabilities;
- security crate;
- unsafe platform adapters;
- migration framework;
- release tooling.

## 23.4 Untrusted PRs

Pull requests from forks MUST NOT receive release secrets or signing credentials.

No workflow triggered by untrusted code may run with privileged secrets unless the checked-out code is controlled and the threat is explicitly mitigated.

---

# 24. Branching and Development Flow

Qbit Toolbox uses a **trunk-based** model.

- `main` is protected and always expected to be releasable at engineering quality gates.
- work occurs on short-lived feature/fix branches.
- no permanent `develop` branch is required.
- “Stage” is a validation/distribution environment/channel, not a second source-of-truth branch.

## 24.1 Pull request rules

A feature PR should be reviewable as one coherent product increment, even if implementation used multiple internal subtasks.

Do not merge a partially wired architectural slice merely to claim progress if the incomplete state would create unused infrastructure or broken contracts.

## 24.2 Feature completion workflow

For each complete feature increment:

1. architecture/requirements are resolved;
2. implementation is completed;
3. repository diff/status is reviewed against the specification;
4. automated test suites pass;
5. local human validation passes;
6. the exact candidate commit is promoted/built for Stage;
7. Stage human validation passes;
8. only then is the production version tag created on the validated commit;
9. validated immutable artifacts are promoted/published to the intended release channel.

A version tag MUST NOT be used as a substitute for Stage validation.

---

# 25. Versioning

Application releases follow Semantic Versioning syntax:

```text
MAJOR.MINOR.PATCH
```

Pre-release syntax:

```text
MAJOR.MINOR.PATCH-beta.N
MAJOR.MINOR.PATCH-rc.N
```

## 25.1 Before 1.0

During `0.x` development:

- MINOR may introduce substantial product capabilities;
- PATCH is reserved for compatible fixes/refinements to the current minor line;
- user data migration and update safety rules still apply; “pre-1.0” is not permission to destroy user state.

## 25.2 Version source

There MUST be one canonical application version source used by build/release tooling and synchronized with Tauri packaging requirements.

Version values MUST NOT be independently edited across multiple manifests without automated consistency checks.

## 25.3 Feature schema versions

Database schema versions are independent from application SemVer.

A single app release may migrate multiple feature databases.

---

# 26. Release Channels and Release Types

Qbit Toolbox supports distinct channels with separate updater metadata.

## 26.1 Stable

Default for ordinary users.

Requirements:

- all stable release gates pass;
- signed artifacts;
- signed updater bundle;
- Stage validation completed;
- release notes include user-visible changes and known limitations;
- migration/upgrade from supported previous stable release validated.

## 26.2 Beta

Opt-in preview channel.

Purpose:

- validate feature behavior with broader users before stable;
- receive compatible update path to later beta/stable as defined by version rules.

Beta MUST still be signed and must not bypass data safety/security gates.

## 26.3 Nightly / Edge

Developer/advanced-user channel built from `main` or scheduled snapshots.

Properties:

- expected to change frequently;
- not the default channel;
- may run a reduced manual-validation gate;
- must remain clearly identified in UI/diagnostics;
- should still be traceable to an exact commit SHA;
- if publicly distributed for execution, signing remains strongly preferred and required once signing infrastructure is established.

## 26.4 Release Candidate

RC is an immutable candidate used for final Stage validation.

It is not automatically a public stable release.

The key principle is **build once, validate, promote the same bits** whenever technically possible.

If validation causes any source or packaging change, the candidate is invalidated and the release pipeline restarts.

## 26.5 Hotfix

A hotfix is a narrowly scoped PATCH release for a production defect or security issue.

Hotfix rules:

- branch/fix from the affected stable baseline or current main depending on divergence;
- minimize unrelated change;
- run the relevant full regression suite;
- validate upgrade from the affected stable version;
- follow signing and Stage gates unless emergency security handling explicitly shortens a manual step;
- merge the fix back to main if developed separately.

## 26.6 Security release

Security releases may use private coordination and embargoed fixes.

They MUST prioritize:

- minimal disclosure before availability;
- signed artifacts;
- rapid but verified build;
- clear supported-version statement;
- post-release advisory when appropriate.

Security urgency does not justify shipping an unverified binary from an unknown source pipeline.

---

# 27. Release Artifact Promotion

The release pipeline must avoid rebuilding different binaries after Stage validation.

Preferred flow:

```text
Validated source commit
        ↓
clean signed candidate build
        ↓
hashes + provenance + metadata
        ↓
Stage distribution
        ↓
manual/automated Stage validation
        ↓
annotated version tag on SAME commit
        ↓
promote SAME artifacts
        ↓
GitHub Release + updater channel metadata
```

If the final public release necessarily requires artifact mutation, the mutation must be deterministic, documented, and revalidated. Recompilation after Stage validation is considered a new artifact and therefore a new candidate.

---

# 28. Release Packaging and Signing

## 28.1 Windows

Initial stable distribution targets Windows.

Artifacts should include the installer format(s) chosen by product distribution policy and MUST be code signed once the project begins public stable distribution.

Signing keys MUST NOT exist in the repository.

## 28.2 macOS future

When macOS becomes supported:

- code signing and notarization are required for normal direct distribution;
- macOS-specific entitlements and permissions require platform security review;
- packaging/release tests run on macOS runners.

## 28.3 Linux future

Linux may use one or more formats such as AppImage/deb/rpm depending on distribution strategy. Artifact signing and repository-specific signing should be used where appropriate.

The architecture does not require every Linux package format from the first Linux release.

## 28.4 Updater signing

Updater signing is independent from OS code signing and MUST be configured for updater-delivered releases.

An update failing signature verification MUST be rejected.

---

# 29. Update Architecture

Tauri Updater is the initial update client.

## 29.1 Channel model

Each release channel has separate update metadata or a dynamic update endpoint capable of honoring channel selection.

Default channel: `stable`.

Users may opt into `beta`; nightly/edge should be clearly marked as advanced/developer.

## 29.2 Update check policy

Update checks MUST avoid high-frequency network wakeups.

A reasonable periodic strategy is configured centrally by Update Manager, and an explicit manual “Check for updates” path is provided.

Features MUST NOT implement their own application-version update loops.

## 29.3 Failed update

A failed download/verification/install must leave the current installation usable whenever the platform installer/updater permits it.

Update failure must not delete user data.

## 29.4 Downgrade

Automatic downgrade is not a supported recovery mechanism because forward database migrations may make older binaries incompatible.

Rollback planning therefore distinguishes:

- binary rollback feasibility;
- database compatibility;
- pre-migration backups;
- feature data recovery.

---

# 30. Build Reproducibility and Provenance

Release builds MUST:

- originate from an exact commit SHA;
- use committed lockfiles;
- run on declared toolchain versions;
- record build metadata;
- produce SHA-256 hashes for public artifacts;
- produce GitHub artifact provenance attestations for public stable artifacts once release automation is established;
- produce an SBOM as part of mature stable release policy.

The About/Diagnostics UI SHOULD expose at least:

- product version;
- release channel;
- commit/build identifier where appropriate;
- architecture/platform.

---

# 31. Static Analysis and Code Quality

## 31.1 Rust

Required baseline:

- rustfmt;
- Clippy with warnings elevated according to workspace policy;
- dead-code review;
- dependency advisory/license policy tooling;
- CodeQL for Rust in GitHub code scanning.

Production code SHOULD avoid broad lint suppressions. Suppressions require local justification.

## 31.2 TypeScript/React

Required baseline:

- `strict: true`;
- linting;
- formatting;
- no implicit `any`;
- explicit parsing/validation at native IPC boundaries when runtime validation is needed;
- CodeQL JavaScript/TypeScript analysis.

`any` SHOULD be treated as an exception requiring a narrow scope and comment or a better typed boundary.

## 31.3 Dependency boundaries

The repository SHOULD add automated architecture checks as the codebase grows, for example:

- no feature-to-feature implementation imports;
- no `md3-next` import outside approved UI foundation paths;
- no `rusqlite` usage outside persistence/repository layers;
- no Tauri raw IPC strings inside ordinary components;
- no platform API usage from domain crates.

---

# 32. Error Handling and Recovery UX

Errors are classified by ownership and recovery.

Suggested categories:

```text
Validation
Conflict
Permission
PlatformUnsupported
ExternalDependency
Persistence
Migration
Runtime
Update
Security
Internal
```

## 32.1 User-facing behavior

The UI should answer:

1. what failed;
2. what functionality is affected;
3. whether user data is safe;
4. what the user can do next.

Do not show raw stack traces/platform error dumps as primary UX.

## 32.2 Feature failure

A feature can fail independently.

Example:

```text
Hotkeys: Failed — keyboard hook could not be installed
Clipboard: Running
Core: Running
```

The application remains usable for unaffected features.

## 32.3 Core persistence failure

If `core.db` cannot be safely opened/migrated, the application enters recovery mode rather than silently creating an empty configuration and losing the reference to existing data.

---

# 33. Feature Permissions and Capability Declarations

Every feature declares the native capabilities it requires.

Examples may include:

- keyboard monitoring/injection;
- clipboard read/write;
- filesystem scopes;
- process/window enumeration;
- screen capture;
- network access;
- notifications;
- elevated operation.

A feature SHOULD NOT gain a capability because another feature already has it.

The user-facing product may later expose privacy/permission explanations using the same metadata.

---

# 34. Future Plugin/Extension Architecture

A third-party extension ecosystem is intentionally deferred.

The current compile-time Feature Registry is **not** presented as a public plugin ABI.

Before third-party plugins are allowed, a dedicated ADR must resolve at least:

- trust model;
- sandbox/process isolation;
- capability permission model;
- API versioning;
- plugin signing/distribution;
- update compatibility;
- crash isolation;
- resource quotas;
- UI extension points;
- migration/data ownership;
- malicious plugin response.

The internal feature model should make this future work possible, but no unstable internal Rust trait is promised as a future plugin ABI.

---

# 35. Future Cloud/Account Features

Cloud sync, accounts, remote settings, and cross-device state are not assumed today.

Architecture preparation consists only of:

- stable IDs;
- explicit schema versions;
- feature data ownership;
- secret separation;
- typed mutation APIs;
- absence of hidden filesystem coupling.

If cloud features are introduced, they require explicit decisions for:

- authentication;
- encryption;
- offline-first behavior;
- conflict resolution;
- deletion semantics;
- privacy/export requirements;
- server compatibility/versioning.

No current local feature should depend on future cloud availability.

---

# 36. Feature UI Discovery and Navigation

The Application Host provides a feature registry projection to the UI containing safe display metadata and status.

The app shell renders navigation from registered feature metadata rather than hardcoding behavior for each feature.

The routing table remains compile-time/static because the current product has compile-time features.

Feature UI chunks SHOULD be lazy-loaded by route so a large mature Toolbox does not load all feature code on every settings launch.

---

# 37. State Management in the Frontend

Global frontend state must be kept small.

App-global UI state may include:

- theme mode;
- locale;
- navigation state;
- feature registry/status summary;
- update notification state.

Feature-specific state belongs inside the feature UI package.

A single giant application store containing every feature's mutable state is forbidden.

Server/native truth MUST be refreshed through feature APIs rather than assuming WebView memory is authoritative across window destruction/recreation.

---

# 38. Caching

Caches are explicitly bounded and disposable.

Every cache must define:

- owner;
- maximum size/count;
- invalidation strategy;
- storage location;
- cleanup trigger;
- whether it survives restart.

Unbounded in-memory maps are forbidden for event/history features.

Cache data must not be required to recover user-created durable state.

---

# 39. Data Retention

Data-heavy features MUST define retention policy.

Examples:

- clipboard history item count/age/size;
- thumbnails;
- OCR caches;
- log retention;
- launcher usage history.

Retention defaults should prioritize bounded disk growth and user control.

A feature adding durable history MUST include deletion/clear behavior in the specification and tests.

---

# 40. Networking

The core application has no requirement for continuous networking beyond explicitly enabled online capabilities such as updater checks.

Rules:

- network clients are feature/core-service scoped;
- timeouts are mandatory;
- retries are bounded with backoff;
- offline operation remains graceful where the feature is primarily local;
- TLS verification must never be disabled in production;
- URLs/endpoints are not silently overridden by untrusted local content;
- background network activity must be observable in feature documentation.

---

# 41. Worker/Sidecar Rules

Sidecars are a capability, not the default architecture.

A feature may introduce a sidecar only if one or more are true:

- required native dependency cannot safely/integrally run in-process;
- crash isolation materially protects the host;
- memory can be reclaimed more reliably by process exit;
- a different privilege/security context is required;
- an existing external binary is the correct maintained dependency.

Every sidecar must define:

- protocol;
- version compatibility;
- startup/shutdown;
- crash policy;
- resource limits;
- capability permissions;
- packaging/signing implications;
- log handling;
- security validation of inputs/outputs.

By default, workers do **not** open feature databases directly. The host remains persistence owner and passes bounded work/results through the worker protocol. A worker may own direct database access only when the feature architecture explicitly assigns that database to the worker and defines locking, crash recovery, migration, and host/worker compatibility.

---

# 42. Manual Validation Policy

Automation is necessary but not sufficient for system utilities.

Every user-visible feature release requires a manual test checklist covering behavior that is difficult to guarantee in automated environments.

For Hotkeys, examples include:

- ordinary desktop apps;
- modifier interactions;
- repeated press/release behavior;
- tray lifecycle;
- startup behavior;
- UI close while runtime continues;
- system sleep/resume if hooks/listeners are affected;
- elevated-target limitation messaging where applicable.

Manual validation results should be recorded in the release/feature checklist rather than existing only in memory.

---

# 43. Release Definition of Done

A feature/release is not complete when “the code is written”.

Required sequence:

1. requirements and architecture are resolved;
2. implementation is complete for the intended feature scope;
3. changed files/diff are reviewed against the specification;
4. automated unit/integration/component/contract/E2E tests pass as applicable;
5. migration tests pass;
6. security/static checks pass;
7. performance checks pass or approved baseline changes are documented;
8. local manual validation passes;
9. candidate is promoted to Stage;
10. Stage manual validation passes;
11. version tag is created on the exact validated commit;
12. signed immutable artifacts are published/promoted;
13. updater metadata is published to the intended channel;
14. post-release smoke check confirms distribution/update availability.

Skipping a genuinely inapplicable test layer is allowed only with explicit justification.

---

# 44. Architecture Decision Records

ADRs live under:

```text
docs/adr/
```

Use an ADR for decisions that:

- change a normative rule in this document;
- introduce a foundational dependency;
- alter process topology;
- alter persistence boundaries;
- add a privilege/security boundary;
- create a public/plugin compatibility promise;
- accept a known performance/security tradeoff;
- establish a new cross-feature abstraction.

Suggested ADR states:

```text
Proposed
Accepted
Superseded
Rejected
```

An accepted ADR changing this document MUST amend the affected section here.

---

# 45. Documentation Requirements

Each feature SHOULD have:

```text
docs/features/<feature>.md
```

containing:

- user intent;
- scope/non-scope;
- domain model;
- runtime mode;
- platform matrix;
- permissions;
- persistence schema ownership;
- IPC surface;
- resource behavior;
- failure modes;
- test plan;
- migration concerns;
- manual validation checklist.

Public-facing docs may be separate from internal engineering specifications.

---

# 46. Prohibited Architectural Patterns

The following patterns are explicitly rejected unless an ADR reverses the rule:

- one giant `services/`, `repositories/`, `components/`, or `hooks/` directory spanning all features;
- direct feature-to-feature implementation imports;
- global mutable singleton state shared across unrelated features;
- SQL from React;
- generic `execute(feature, action, json)` IPC;
- local HTTP/WebSocket server used as an internal replacement for Tauri IPC without an ADR-backed requirement;
- business logic in Tauri command functions;
- platform API types leaking into feature domain models;
- hidden forever-running WebView needed for native features;
- permanent Admin/root application mode;
- polling where an event API exists without justification;
- unbounded background queues;
- unbounded history/caches;
- synchronous logging in high-frequency hooks;
- filesystem/database access from keyboard hook callback;
- logging keystrokes or clipboard payloads;
- silently recreating a failed user DB;
- cross-feature SQL joins/transactions;
- mandatory cloud dependency for local utilities;
- premature third-party plugin ABI;
- sidecar-per-feature by default;
- shared `utils` dumping grounds;
- mutable GitHub Action tags in release/security-sensitive workflows;
- hand-edited generated IPC types;
- foundational reliance on unstable RC dependencies without ADR approval.

---

# 47. Initial Implementation Order

Architecture is designed for the final platform, but implementation remains incremental.

Recommended bootstrap order:

1. monorepo/workspace foundation;
2. Tauri host bootstrap + single instance;
3. core persistence + migrations;
4. feature API + registry + lifecycle;
5. diagnostics foundation;
6. Window/Tray lifecycle;
7. Next.js static-export application shell;
8. `packages/ui` + MUI + MD3 Next theme;
9. Rust → TypeScript DTO generation and IPC conventions;
10. Hotkeys feature persistence/domain/application layers;
11. Windows Hotkeys runtime/platform adapter;
12. Hotkeys UI;
13. full automated test layers;
14. updater/release pipeline;
15. Stage and stable release process.

Do not implement future Clipboard/Launcher/OCR runtime code until its feature work begins. Their integration path is already defined by the platform contracts above.

---

# 48. Architecture Acceptance Checklist for New Features

Before implementation of any new feature, its design must answer:

### Ownership

- What is the stable feature ID?
- What code/data does the feature own?
- Does it depend on another feature? If yes, can that dependency be replaced with a core capability or explicit contract?

### Runtime

- `EmbeddedBackground`, `EmbeddedOnDemand`, or `IsolatedWorker`?
- What permanent threads/listeners/timers exist?
- How is runtime cancelled?
- What happens when disabled?

### Platform

- Which platforms are supported?
- What platform-specific APIs are required?
- Is platform-neutral domain code clean?

### Persistence

- Does it need relational data, blobs, cache, or secrets?
- What is its durability profile?
- What are retention limits?
- What are migrations/backups?

### Security

- What privileges/capabilities are required?
- What untrusted inputs exist?
- Does it access sensitive user data?

### IPC

- What explicit commands/events are needed?
- What are payload limits?
- What error codes are required?

### UI

- What route exists?
- Does it use MD3 design-system patterns?
- Is it keyboard accessible?
- Does it support localization/RTL constraints?
- Does it require virtualization?

### Quality

- What unit tests?
- What persistence/platform integration tests?
- What E2E tests?
- What performance/resource tests?
- What manual validation?

A feature is not ready for implementation while material questions above remain unresolved.

---

# 49. Architecture Decisions Summary

| Area | Decision |
|---|---|
| Product shape | Long-lived PowerToys-class utilities platform |
| Architecture | Feature-first modular monolith with optional isolated workers |
| Initial platform | Windows |
| Target platforms | Windows, macOS, Linux |
| Desktop host | Tauri 2 |
| Native language | Rust |
| Rust workspace | Cargo workspace, edition 2024/resolver 3 |
| Frontend | Next.js static export + React + TypeScript |
| UI library | MUI |
| Design system | Material Design 3 via MD3 Next |
| JS workspace | Bun workspace |
| UI runtime | On-demand control plane; not required for background features |
| Persistence | SQLite via rusqlite in Rust |
| DB topology | `core.db` + feature-owned databases where needed |
| SQL from frontend | Forbidden |
| Secrets | Separate secure-store abstraction when needed |
| IPC ownership | Explicit feature commands/events; Rust DTO source of truth |
| TS DTO generation | Stable Rust → TypeScript binding generation; generated files checked in/drift-tested |
| Feature communication | Explicit core capability/orchestration/typed notifications, no implementation imports |
| Feature registry | Compile-time/internal; not a public plugin ABI |
| Runtime modes | EmbeddedBackground / EmbeddedOnDemand / IsolatedWorker |
| Disabled feature | Near-zero runtime cost |
| Background UI | No permanent WebView requirement |
| Hotkeys Windows backend | Low-level keyboard hook + compiled map + SendInput |
| Cross-platform | Platform adapters inside features, domain remains neutral |
| Security | Least privilege Tauri capabilities, normal-user host |
| CI/CD | GitHub Actions |
| Distribution | GitHub Releases initially |
| Updates | Tauri Updater, channel-aware |
| Branching | Trunk-based protected `main` |
| Release channels | Stable / Beta / Nightly-Edge / RC |
| Stable tag timing | After Stage validation, on exact validated commit |
| Release principle | Build once, validate, promote same artifacts |
| Versioning | SemVer |
| Tests | Unit + component + contract + integration + migration + E2E + packaging + updater + performance + manual, as applicable |
| Supply chain | SHA-pinned Actions, least privilege, CodeQL, signed releases, provenance |
| Architecture changes | ADR + same-PR update to this document |

---

# 50. Intentionally Deferred Decisions

A source of truth must distinguish architectural decisions from choices that cannot yet be made responsibly. The following are intentionally deferred until their trigger exists; they MUST NOT be silently guessed during implementation.

| Decision | Trigger for resolution | Required output |
|---|---|---|
| Exact Windows installer format set (MSI/NSIS/store packaging combination) | First public distribution design | Release/distribution ADR + packaging tests |
| Windows certificate/signing provider and key custody | Before public signed beta/stable | Security/release ADR |
| Exact numeric CPU/RAM/latency release budgets | After reference hardware/tooling baseline is measured | Versioned performance baseline + thresholds |
| Secret-store implementation | First feature that persists a real credential/secret | Security ADR + threat model |
| Telemetry implementation | First concrete product requirement for telemetry | Privacy/telemetry ADR and user disclosure |
| Cloud sync/account architecture | First approved cross-device/cloud feature | Dedicated sync/security ADRs |
| Third-party plugin ABI | First approved extension ecosystem requirement | Plugin threat model + compatibility ADR |
| Elevated broker/service | First requirement that cannot safely operate at normal integrity level | Privilege separation ADR + threat model |
| Exact updater check cadence | Updater UX/product specification | Update policy and resource measurement |
| Uninstaller user-data deletion default | Before first public installer is frozen | Product/privacy decision + packaging tests |
| First supported locale set | Before public UI localization commitment | Localization product spec |
| Linux package formats | Before Linux public beta | Linux distribution ADR |
| macOS distribution path (direct/App Store/both) | Before macOS public beta | macOS distribution/signing ADR |

These are **not architecture holes**. The surrounding boundaries are defined now so the later decision does not require restructuring unrelated features.

---

# 51. Upstream Technical References

These links document upstream behavior that materially informs this architecture. They are references, not substitutes for the normative decisions above.

## Tauri

- Architecture: https://v2.tauri.app/concept/architecture/
- Next.js frontend/static export: https://v2.tauri.app/start/frontend/nextjs/
- Capabilities: https://v2.tauri.app/security/capabilities/
- Testing overview: https://v2.tauri.app/develop/tests/
- Tauri API mocking: https://v2.tauri.app/develop/tests/mocking/
- WebDriver testing: https://v2.tauri.app/develop/tests/webdriver/
- Updater: https://v2.tauri.app/plugin/updater/
- Autostart: https://v2.tauri.app/plugin/autostart/
- Sidecars: https://v2.tauri.app/develop/sidecar/
- Distribution: https://v2.tauri.app/distribute/
- Windows code signing: https://v2.tauri.app/distribute/sign/windows/
- GitHub pipeline guidance: https://v2.tauri.app/distribute/pipelines/github/

## MD3 Next

- Product/docs: https://www.md3next.dev/
- Getting started: https://www.md3next.dev/docs/getting-started

## Next.js

- Static exports: https://nextjs.org/docs/app/guides/static-exports

## SQLite

- Transactions: https://sqlite.org/lang_transaction.html
- WAL/temp files: https://sqlite.org/tempfiles.html
- PRAGMA/synchronous behavior: https://sqlite.org/pragma.html
- Locking/concurrency: https://sqlite.org/lockingv3.html

## Rust / TypeScript contracts

- Cargo dependency resolver: https://doc.rust-lang.org/cargo/reference/resolver.html
- Rust 2024 resolver behavior: https://doc.rust-lang.org/edition-guide/rust-2024/cargo-resolver.html
- ts-rs: https://github.com/Aleph-Alpha/ts-rs

## GitHub security and release provenance

- Secure use of GitHub Actions: https://docs.github.com/en/actions/reference/security/secure-use
- CodeQL code scanning: https://docs.github.com/code-security/code-scanning/introduction-to-code-scanning/about-code-scanning-with-codeql
- Artifact attestations/provenance: https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations

---

# 52. Final Principle

The architecture should make a new Qbit utility feel like adding a **vertical feature module to a stable desktop platform**, not like adding another special case to a growing application.

The permanent rule is:

> **Predict the integration needs of the mature product, keep the architectural boundaries ready for them, but instantiate runtime complexity only when a real feature needs it.**

This is the engineering basis on which all Qbit Toolbox features, beginning with Hotkeys, are to be designed, implemented, tested, validated, and released.
