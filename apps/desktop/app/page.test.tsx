import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { type CoreStatusDto } from "@qbit/ipc";
import { afterEach, describe, expect, it, vi } from "vitest";

import HomePage from "./page";
import { Providers } from "./providers";
import { setSystemColorScheme } from "../test/setup";

const { getCoreStatus } = vi.hoisted(() => ({ getCoreStatus: vi.fn() }));

vi.mock("@qbit/ipc", () => ({
  ipc: { getCoreStatus },
}));

const emptyCoreStatus = {
  appVersion: "0.1.0",
  platform: "windows",
  runtimeState: "running",
  schemaVersion: 1,
  startupError: null,
  features: [],
} satisfies CoreStatusDto;

function renderPage(catalogExtensions?: Readonly<Record<string, string>>) {
  return render(
    <Providers catalogExtensions={catalogExtensions}>
      <HomePage />
    </Providers>,
  );
}

describe("HomePage", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("shows loading before rendering an empty core status", async () => {
    let resolveStatus: (value: CoreStatusDto) => void;
    getCoreStatus.mockReturnValueOnce(
      new Promise<CoreStatusDto>((resolve) => {
        resolveStatus = resolve;
      }),
    );

    renderPage();

    expect(screen.getByRole("status")).toHaveTextContent(
      "Loading core status…",
    );

    resolveStatus!(emptyCoreStatus);

    expect(
      await screen.findByText("No features are registered yet."),
    ).toBeInTheDocument();
    expect(screen.getByText("App version: 0.1.0")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(getCoreStatus).toHaveBeenCalledTimes(1);
  });

  it("projects registered features into a semantic list", async () => {
    getCoreStatus.mockResolvedValueOnce({
      ...emptyCoreStatus,
      features: [
        {
          id: "notes",
          displayNameKey: "features.notes.name",
          descriptionKey: "features.notes.description",
          runtimeMode: "embeddedBackground",
          startupPolicy: "onApplicationStart",
          lifecycleState: "running",
          route: null,
        },
      ],
    } satisfies CoreStatusDto);

    renderPage({ "features.notes.name": "Notes" });

    const list = await screen.findByRole("list", {
      name: "Registered features",
    });
    const feature = within(list).getByRole("listitem");
    expect(feature).toBeInTheDocument();
    expect(feature).toHaveTextContent("Notes (notes) — Running");
    expect(list).toContainElement(feature);
  });

  it("shows a safe error and retries the status request", async () => {
    getCoreStatus.mockRejectedValueOnce(
      new Error("Native details must not be displayed"),
    );
    getCoreStatus.mockResolvedValueOnce(emptyCoreStatus);

    renderPage();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Unable to load core status. Please try again.",
    );
    expect(
      screen.queryByText("Native details must not be displayed"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByText("No features are registered yet."),
    ).toBeInTheDocument();
    expect(getCoreStatus).toHaveBeenCalledTimes(2);
  });

  it("shows safe recovery UI without exposing startup error details", async () => {
    getCoreStatus.mockResolvedValueOnce({
      ...emptyCoreStatus,
      runtimeState: "recoveryRequired",
      schemaVersion: null,
      startupError: {
        code: "migration-failed",
        category: "migration",
        messageKey: "errors.core.migration_failed",
        recoverable: false,
        context: { nativeDetail: "must remain private" },
      },
    } satisfies CoreStatusDto);

    renderPage();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The application requires recovery before it can continue safely.",
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "A data migration failed.",
    );
    expect(screen.getByText("Schema version: Unavailable")).toBeInTheDocument();
    expect(screen.queryByText("migration-failed")).not.toBeInTheDocument();
    expect(screen.queryByText("must remain private")).not.toBeInTheDocument();
  });

  it("keeps generic recovery guidance when the startup error key is unknown", async () => {
    getCoreStatus.mockResolvedValueOnce({
      ...emptyCoreStatus,
      runtimeState: "recoveryRequired",
      schemaVersion: null,
      startupError: {
        code: "native-storage-failure",
        category: "persistence",
        messageKey: "errors.native.unexpected_detail",
        recoverable: false,
        context: { nativeDetail: "must remain private" },
      },
    } satisfies CoreStatusDto);

    renderPage();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The application requires recovery before it can continue safely.",
    );
    expect(
      screen.queryByText("errors.native.unexpected_detail"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("native-storage-failure"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("must remain private")).not.toBeInTheDocument();
  });

  it("applies the selected MD3 theme preference and system scheme", async () => {
    getCoreStatus.mockReturnValueOnce(new Promise(() => {}));

    renderPage();

    const systemButton = screen.getByRole("button", { name: "System" });
    const lightButton = screen.getByRole("button", { name: "Light" });
    const darkButton = screen.getByRole("button", { name: "Dark" });
    const appShell = screen.getByRole("banner").parentElement;

    expect(systemButton).toHaveAttribute("aria-pressed", "true");
    expect(lightButton).toBeInTheDocument();
    expect(darkButton).toBeInTheDocument();
    expect(appShell).not.toBeNull();

    await waitFor(() => {
      expect(window.getComputedStyle(appShell!).backgroundColor).toBe(
        "rgb(253, 248, 253)",
      );
    });

    fireEvent.click(darkButton);
    expect(darkButton).toHaveAttribute("aria-pressed", "true");
    await waitFor(() => {
      expect(window.getComputedStyle(appShell!).backgroundColor).toBe(
        "rgb(20, 19, 22)",
      );
    });

    fireEvent.click(lightButton);
    expect(lightButton).toHaveAttribute("aria-pressed", "true");
    await waitFor(() => {
      expect(window.getComputedStyle(appShell!).backgroundColor).toBe(
        "rgb(253, 248, 253)",
      );
    });

    fireEvent.click(systemButton);
    expect(systemButton).toHaveAttribute("aria-pressed", "true");
    setSystemColorScheme("dark");
    await waitFor(() => {
      expect(window.getComputedStyle(appShell!).backgroundColor).toBe(
        "rgb(20, 19, 22)",
      );
    });

    setSystemColorScheme("light");
    await waitFor(() => {
      expect(window.getComputedStyle(appShell!).backgroundColor).toBe(
        "rgb(253, 248, 253)",
      );
    });
  });
});
