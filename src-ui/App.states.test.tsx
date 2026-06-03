import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { App } from "./App";
import {
  bucketFromQuota,
  bucketMeterLabel,
  bucketPercent,
  displayBuckets,
  formatLimit,
  formatNumber,
  formatReset,
} from "./format";
import { TrayPanel } from "./TrayPanel";
import type { AccountView, DashboardState, UsageSnapshot } from "./types";

const api = vi.hoisted(() => ({
  cancelAccountLogin: vi.fn(),
  checkForUpdates: vi.fn(),
  closePreferences: vi.fn(),
  detectAccounts: vi.fn(),
  getAppVersion: vi.fn(),
  guardedFetch: vi.fn(),
  installUpdate: vi.fn(),
  isStale: vi.fn(),
  logoutAccount: vi.fn(),
  markFetched: vi.fn(),
  onCheckUpdateRequested: vi.fn(),
  onDashboardUpdated: vi.fn(),
  onLoginComplete: vi.fn(),
  onLoginFailed: vi.fn(),
  onLoginProgress: vi.fn(),
  onRefreshRequested: vi.fn(),
  onSettingsUpdated: vi.fn(),
  onUpdateProgress: vi.fn(),
  openPreferences: vi.fn(),
  readCachedDashboard: vi.fn(),
  removeAccount: vi.fn(),
  reorderAccounts: vi.fn(),
  resizePreferencesToContent: vi.fn(),
  resizeTrayToContent: vi.fn(),
  saveAccount: vi.fn(),
  saveSettings: vi.fn(),
  startAccountLogin: vi.fn(),
  updaterAvailable: vi.fn(),
}));

vi.mock("./api", () => api);

beforeEach(() => {
  vi.clearAllMocks();
  api.onDashboardUpdated.mockResolvedValue(() => {});
  api.onRefreshRequested.mockResolvedValue(() => {});
  api.onSettingsUpdated.mockResolvedValue(() => {});
  api.onLoginProgress.mockResolvedValue(() => {});
  api.onLoginComplete.mockResolvedValue(() => {});
  api.onLoginFailed.mockResolvedValue(() => {});
  api.onUpdateProgress.mockResolvedValue(() => {});
  api.onCheckUpdateRequested.mockResolvedValue(() => {});
  api.updaterAvailable.mockResolvedValue(false);
  api.checkForUpdates.mockResolvedValue(null);
  api.installUpdate.mockResolvedValue(undefined);
  api.getAppVersion.mockResolvedValue("1.2.3");
  api.openPreferences.mockResolvedValue(undefined);
  api.reorderAccounts.mockResolvedValue([]);
  api.logoutAccount.mockResolvedValue([]);
  api.startAccountLogin.mockResolvedValue({ id: "pending-1" });
  api.cancelAccountLogin.mockResolvedValue([]);
  api.guardedFetch.mockResolvedValue(dashboardState());
  // Default: cold start (no cached dashboard) so existing tests exercise the
  // fetch-on-mount path and the loading spinner.
  api.readCachedDashboard.mockReturnValue(null);
  api.isStale.mockReturnValue(true);
  api.markFetched.mockReturnValue(undefined);
  api.closePreferences.mockResolvedValue(undefined);
  api.resizePreferencesToContent.mockResolvedValue(undefined);
  api.resizeTrayToContent.mockResolvedValue(undefined);
  api.detectAccounts.mockResolvedValue([]);
  api.removeAccount.mockResolvedValue([]);
  api.saveAccount.mockResolvedValue([]);
  api.saveSettings.mockResolvedValue({ hideFromDock: false });
});

afterEach(() => {
  cleanup();
  window.history.replaceState({}, "", "/");
});

test("shows a loading refresh control while dashboard data is pending", async () => {
  let resolveDashboard: (state: DashboardState) => void = () => {};
  api.guardedFetch.mockReturnValue(
    new Promise<DashboardState>((resolve) => {
      resolveDashboard = resolve;
    }),
  );

  render(<App />);

  expect(screen.getByTitle("Refresh")).toBeDisabled();
  resolveDashboard(dashboardState());
  expect(
    await screen.findByText("Burnrate: no enabled accounts"),
  ).toBeInTheDocument();
});

test("renders dashboard load errors", async () => {
  api.guardedFetch.mockRejectedValue(new Error("offline"));

  render(<App />);

  expect(await screen.findByRole("alert")).toHaveTextContent("Error: offline");
});

test("hydrates instantly from a fresh cache without refetching", () => {
  api.readCachedDashboard.mockReturnValue({
    dashboard: dashboardState({ snapshots: [snapshot("healthy")] }),
    fetchedAt: 1_000,
  });
  api.isStale.mockReturnValue(false);

  render(<App />);

  // Cached data renders synchronously: no spinner, no fetch.
  expect(screen.getByText("Burnrate: all quotas healthy")).toBeInTheDocument();
  expect(screen.getByTitle("Refresh")).not.toBeDisabled();
  expect(api.guardedFetch).not.toHaveBeenCalled();
});

test("revalidates in the background when the cache is stale", async () => {
  api.readCachedDashboard.mockReturnValue({
    dashboard: dashboardState({ snapshots: [snapshot("healthy")] }),
    fetchedAt: 1_000,
  });
  api.isStale.mockReturnValue(true);
  api.guardedFetch.mockResolvedValue(
    dashboardState({ snapshots: [snapshot("warning")] }),
  );

  render(<App />);

  expect(await screen.findByText("Burnrate: 1 warning")).toBeInTheDocument();
  expect(api.guardedFetch).toHaveBeenCalledWith({});
});

test("renders stale snapshot state", async () => {
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      snapshots: [
        {
          accountId: "codex-local",
          provider: "codex",
          label: "Codex",
          status: "stale",
          subscription: {
            plan: "pro",
            planLabel: "Pro",
            rateLimitTier: null,
            extraUsageEnabled: null,
            source: "test",
          },
          usageBuckets: [
            {
              id: "5-hour",
              label: "5-hour",
              window: "5-hour",
              used: 90,
              limit: 100,
              remaining: 10,
              unit: "requests",
              resetAt: null,
              status: "stale",
            },
          ],
          quota: {
            used: 90,
            limit: 100,
            remaining: 10,
            unit: "requests",
            resetAt: null,
          },
          message: "Last refresh is older than the quota window.",
          fetchedAt: new Date().toISOString(),
        },
      ],
    }),
  );

  render(<App />);

  expect((await screen.findAllByText("Stale")).length).toBeGreaterThan(1);
  expect(screen.getByText("Burnrate: data is stale")).toBeInTheDocument();
  expect(
    screen.getByText("Last refresh is older than the quota window."),
  ).toBeInTheDocument();
});

test("applies dashboard updates emitted by the backend", async () => {
  let onDashboard: (dashboard: DashboardState) => void = () => {};
  api.guardedFetch.mockResolvedValue(dashboardState());
  api.onDashboardUpdated.mockImplementation((handler) => {
    onDashboard = handler;
    return Promise.resolve(() => {});
  });

  render(<App />);

  expect(
    await screen.findByText("Burnrate: no enabled accounts"),
  ).toBeInTheDocument();

  onDashboard(
    dashboardState({
      snapshots: [
        {
          accountId: "codex-local",
          provider: "codex",
          label: "Codex",
          status: "warning",
          subscription: null,
          usageBuckets: [],
          quota: null,
          message: null,
          fetchedAt: new Date().toISOString(),
        },
      ],
      traySummary: {
        label: "Burnrate: 1 warning",
        status: "warning",
        criticalCount: 0,
        warningCount: 1,
        updatedAt: new Date().toISOString(),
      },
    }),
  );

  expect(await screen.findByText("Burnrate: 1 warning")).toBeInTheDocument();
  expect(api.onDashboardUpdated).toHaveBeenCalledOnce();
});

test("renders compact tray view from the tray window route", async () => {
  window.history.replaceState({}, "", "/?view=tray");
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      accounts: [
        {
          id: "codex-local",
          provider: "codex",
          label: "Codex",
          enabled: true,
          autoDetected: true,
          credentialPath: "~/.codex/auth.json",
          endpointOverride: null,
          secretStorage: "keyring",
          hasSecret: false,
          email: "codex@example.com",
          configDir: null,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
        },
      ],
      snapshots: [
        {
          accountId: "codex-local",
          provider: "codex",
          label: "Codex",
          status: "warning",
          subscription: {
            plan: "pro",
            planLabel: "Pro",
            rateLimitTier: null,
            extraUsageEnabled: null,
            source: "test",
          },
          usageBuckets: [
            {
              id: "5-hour",
              label: "5-hour",
              window: "5-hour",
              used: 90,
              limit: 100,
              remaining: 10,
              unit: "requests",
              resetAt: null,
              status: "warning",
            },
          ],
          quota: {
            used: 90,
            limit: 100,
            remaining: 10,
            unit: "requests",
            resetAt: null,
          },
          message: null,
          fetchedAt: new Date().toISOString(),
        },
      ],
    }),
  );

  render(<App />);

  expect(
    await screen.findByRole("region", { name: "Usage" }),
  ).toBeInTheDocument();
  expect(screen.getAllByText("Codex").length).toBeGreaterThan(0);
  expect(screen.getByText("10 / 100 requests")).toBeInTheDocument();
});

test("falls back to frontend summaries for older dashboard payloads", async () => {
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      snapshots: [snapshot("healthy"), snapshot("error")],
      traySummary: undefined as unknown as DashboardState["traySummary"],
    }),
  );

  render(<App />);

  expect(await screen.findByText("Burnrate: 1 critical")).toBeInTheDocument();
});

test("closes preferences from macOS window shortcuts", async () => {
  api.guardedFetch.mockResolvedValue(dashboardState());

  render(<App />);

  await screen.findByRole("heading", { name: "Preferences" });
  fireEvent.keyDown(window, { key: "w", metaKey: true });
  fireEvent.keyDown(window, { key: "q", metaKey: true });

  expect(api.closePreferences).toHaveBeenCalledTimes(2);
});

test("refreshes usage after saving an OpenRouter account", async () => {
  const savedAccount: AccountView = {
    id: "openrouter-team",
    provider: "openrouter",
    label: "OpenRouter Team",
    enabled: true,
    autoDetected: false,
    credentialPath: null,
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: true,
    email: null,
    configDir: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
  api.guardedFetch.mockResolvedValueOnce(dashboardState()).mockResolvedValue(
    dashboardState({
      accounts: [savedAccount],
      snapshots: [snapshot("healthy", { accountId: "openrouter-team" })],
    }),
  );
  api.saveAccount.mockResolvedValue([savedAccount]);

  render(<App />);

  await screen.findByRole("heading", { name: "Preferences" });
  fireEvent.change(screen.getByLabelText("Label"), {
    target: { value: "OpenRouter Team" },
  });
  fireEvent.change(screen.getByLabelText("API Key"), {
    target: { value: "sk-openrouter" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));

  expect(
    await screen.findByText("Burnrate: all quotas healthy"),
  ).toBeInTheDocument();
  expect(api.saveAccount).toHaveBeenCalledWith(
    expect.objectContaining({
      endpointOverride: null,
      label: "OpenRouter Team",
      provider: "openrouter",
      secret: "sk-openrouter",
    }),
  );
  expect(api.guardedFetch).toHaveBeenCalledWith({ force: true });
});

test("persists the chosen update channel", async () => {
  api.guardedFetch.mockResolvedValue(dashboardState());
  api.saveSettings.mockResolvedValue({
    hideFromDock: true,
    updateChannel: "nightly",
  });

  render(<App />);
  await screen.findByRole("heading", { name: "Preferences" });

  fireEvent.change(screen.getByLabelText("Release channel"), {
    target: { value: "nightly" },
  });

  await waitFor(() =>
    expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ updateChannel: "nightly" }),
    ),
  );
});

test("selecting the already-active channel does not re-save", async () => {
  api.guardedFetch.mockResolvedValue(dashboardState());

  render(<App />);
  await screen.findByRole("heading", { name: "Preferences" });

  // dashboardState() defaults to the "stable" channel.
  fireEvent.change(screen.getByLabelText("Release channel"), {
    target: { value: "stable" },
  });
  expect(api.saveSettings).not.toHaveBeenCalled();
});

test("surfaces an error when saving the update channel fails", async () => {
  api.guardedFetch.mockResolvedValue(dashboardState());
  api.saveSettings.mockRejectedValue(new Error("disk full"));

  render(<App />);
  await screen.findByRole("heading", { name: "Preferences" });

  fireEvent.change(screen.getByLabelText("Release channel"), {
    target: { value: "nightly" },
  });

  expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
  // The optimistic change rolls back to the persisted channel on failure.
  expect(screen.getByLabelText("Release channel")).toHaveValue("stable");
});

test("falls back to frontend warning and stale summaries", async () => {
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      snapshots: [snapshot("warning")],
      traySummary: undefined as unknown as DashboardState["traySummary"],
    }),
  );

  render(<App />);

  expect(await screen.findByText("Burnrate: 1 warning")).toBeInTheDocument();

  cleanup();
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      snapshots: [snapshot("stale")],
      traySummary: undefined as unknown as DashboardState["traySummary"],
    }),
  );
  render(<App />);

  expect(
    await screen.findByText("Burnrate: data is stale"),
  ).toBeInTheDocument();
});

test("renders fallback healthy and empty summaries", async () => {
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      snapshots: [snapshot("healthy")],
      traySummary: undefined as unknown as DashboardState["traySummary"],
    }),
  );

  render(<App />);

  expect(
    await screen.findByText("Burnrate: all quotas healthy"),
  ).toBeInTheDocument();

  cleanup();
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      snapshots: [],
      traySummary: undefined as unknown as DashboardState["traySummary"],
    }),
  );
  render(<App />);

  expect(
    await screen.findByText("Burnrate: no enabled accounts"),
  ).toBeInTheDocument();
});

test("renders tray summary branches and account disabled state", () => {
  const { rerender } = render(
    <TrayPanel
      state={dashboardState({
        accounts: [
          {
            id: "disabled-openrouter",
            provider: "openrouter",
            label: "OpenRouter",
            enabled: false,
            autoDetected: false,
            credentialPath: null,
            endpointOverride: null,
            secretStorage: "plaintext",
            hasSecret: true,
            email: null,
            configDir: null,
            createdAt: new Date().toISOString(),
            updatedAt: new Date().toISOString(),
          },
        ],
      })}
      snapshots={[]}
      busy={false}
      error="network offline"
      onRefresh={() => {}}
      onOpenPreferences={() => {}}
      onReorderAccounts={() => {}}
    />,
  );

  expect(screen.getByText("No enabled accounts")).toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("network offline");
  expect(screen.getByText("Disabled")).toBeInTheDocument();

  rerender(
    <TrayPanel
      state={dashboardState()}
      snapshots={[snapshot("error")]}
      busy={false}
      error={null}
      onRefresh={() => {}}
      onOpenPreferences={() => {}}
      onReorderAccounts={() => {}}
    />,
  );
  expect(screen.getByText("Critical usage")).toBeInTheDocument();

  rerender(
    <TrayPanel
      state={dashboardState()}
      snapshots={[snapshot("stale")]}
      busy={false}
      error={null}
      onRefresh={() => {}}
      onOpenPreferences={() => {}}
      onReorderAccounts={() => {}}
    />,
  );
  expect(screen.getByText("Usage data is stale")).toBeInTheDocument();

  rerender(
    <TrayPanel
      state={dashboardState()}
      snapshots={[snapshot("healthy")]}
      busy
      error={null}
      onRefresh={() => {}}
      onOpenPreferences={() => {}}
      onReorderAccounts={() => {}}
    />,
  );
  expect(screen.getByText("All quotas healthy")).toBeInTheDocument();
  expect(screen.getByTitle("Refresh")).toBeDisabled();
});

test("opens preferences from the tray settings gear", async () => {
  window.history.replaceState({}, "", "/?view=tray");
  api.guardedFetch.mockResolvedValue(dashboardState());

  render(<App />);

  fireEvent.click(await screen.findByTitle("Settings"));
  expect(api.openPreferences).toHaveBeenCalledOnce();
});

test("dispatches tray refresh requests", async () => {
  window.history.replaceState({}, "", "/?view=tray");
  api.guardedFetch.mockResolvedValueOnce(dashboardState()).mockResolvedValue(
    dashboardState({
      snapshots: [snapshot("healthy")],
    }),
  );

  render(<App />);

  fireEvent.click(await screen.findByTitle("Refresh"));

  expect(await screen.findByText("All quotas healthy")).toBeInTheDocument();
  expect(api.guardedFetch).toHaveBeenCalledWith({ force: true });
});

test("auto-sizes the tray window to its measured content", async () => {
  window.history.replaceState({}, "", "/?view=tray");
  api.guardedFetch.mockResolvedValue(
    dashboardState({ snapshots: [snapshot("healthy")] }),
  );

  // jsdom reports zero layout; emulate a laid-out panel so the measure effect
  // produces a real size and drives the resize bridge. We only assert on the
  // mock (no DOM-visibility queries), so a stubbed computed style is safe.
  const styleSpy = vi.spyOn(window, "getComputedStyle").mockReturnValue({
    paddingTop: "12px",
    paddingBottom: "12px",
    paddingLeft: "12px",
    paddingRight: "12px",
    rowGap: "9px",
    gap: "9px",
    getPropertyValue: () => "",
  } as unknown as CSSStyleDeclaration);
  const offsetHeight = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "offsetHeight",
  );
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get: () => 36,
  });

  try {
    render(<App />);
    await waitFor(() =>
      expect(api.resizeTrayToContent).toHaveBeenCalledWith(expect.any(Number)),
    );
  } finally {
    styleSpy.mockRestore();
    if (offsetHeight) {
      Object.defineProperty(
        HTMLElement.prototype,
        "offsetHeight",
        offsetHeight,
      );
    }
  }
});

test("re-fetches and re-caches the dashboard after detecting accounts", async () => {
  api.guardedFetch.mockResolvedValue(dashboardState());
  api.detectAccounts.mockResolvedValue([]);

  render(<App />);

  await screen.findByRole("heading", { name: "Preferences" });
  api.guardedFetch.mockClear(); // ignore the mount fetch
  fireEvent.click(screen.getByTitle("Detect accounts"));

  await waitFor(() =>
    expect(api.guardedFetch).toHaveBeenCalledWith({ force: true }),
  );
});

test("renders a tray snapshot from the quota fallback with an inline message", () => {
  render(
    <TrayPanel
      state={dashboardState()}
      snapshots={[
        snapshot("warning", {
          usageBuckets: [],
          quota: {
            used: 9,
            limit: 10,
            remaining: 1,
            unit: "requests",
            resetAt: null,
          },
          message: "Approaching the limit.",
        }),
      ]}
      busy={false}
      error={null}
      onRefresh={() => {}}
      onOpenPreferences={() => {}}
      onReorderAccounts={() => {}}
    />,
  );

  // Bucket synthesized from the quota fallback (usageBuckets empty).
  expect(screen.getByText("Quota")).toBeInTheDocument();
  expect(screen.getByText("Approaching the limit.")).toBeInTheDocument();
});

test("formats quota fallback and reset edge cases", () => {
  expect(bucketFromQuota(snapshot("healthy", { quota: null }))).toBeNull();
  expect(
    formatLimit({
      id: "unknown",
      label: "Unknown",
      window: null,
      used: 0,
      limit: null,
      remaining: null,
      unit: "%",
      resetAt: null,
      status: "healthy",
    }),
  ).toBe("Unknown");
  expect(
    displayBuckets(
      snapshot("healthy", {
        usageBuckets: [
          {
            id: "unknown",
            label: "Unknown",
            window: null,
            used: 0,
            limit: null,
            remaining: null,
            unit: "USD",
            resetAt: null,
            status: "healthy",
          },
          {
            id: "current-burn",
            label: "Current burn",
            window: null,
            used: 0,
            limit: 80,
            remaining: null,
            unit: "USD/hr",
            resetAt: null,
            status: "healthy",
          },
        ],
      }),
    ).map((bucket) => bucket.id),
  ).toEqual(["current-burn"]);
  expect(
    formatLimit({
      id: "burn",
      label: "Burn",
      window: null,
      used: 1.25,
      limit: 80,
      remaining: null,
      unit: "USD/hr",
      resetAt: null,
      status: "healthy",
    }),
  ).toBe("$1.25 / $80");
  expect(
    formatLimit({
      id: "credits",
      label: "Credits",
      window: null,
      used: 66.4,
      limit: 125,
      remaining: 58.6,
      unit: "USD",
      resetAt: null,
      status: "healthy",
    }),
  ).toBe("$58.6 / $125");
  expect(
    bucketMeterLabel({
      id: "burn",
      label: "Current burn",
      window: null,
      used: 1.25,
      limit: 80,
      remaining: null,
      unit: "USD/hr",
      resetAt: null,
      status: "healthy",
    }),
  ).toBe("Current burn usage");
  expect(
    bucketPercent({
      id: "over",
      label: "Over",
      window: null,
      used: 0,
      limit: 100,
      remaining: 125,
      unit: "%",
      resetAt: null,
      status: "healthy",
    }),
  ).toBe(100);
  expect(formatNumber(101.25)).toBe("101");
  expect(formatReset(null)).toBe("");
  expect(formatReset("not a date")).toBe("");
  expect(
    formatReset(new Date(Date.now() + 49 * 60 * 60 * 1000).toISOString()),
  ).toMatch(/^resets /);
});

function dashboardState(
  overrides: Partial<DashboardState> = {},
): DashboardState {
  const accounts: AccountView[] = overrides.accounts ?? [];
  const snapshots: UsageSnapshot[] = overrides.snapshots ?? [];
  const criticalCount = snapshots.filter((snapshot) =>
    ["exhausted", "error"].includes(snapshot.status),
  ).length;
  const warningCount = snapshots.filter(
    (snapshot) => snapshot.status === "warning",
  ).length;
  const staleCount = snapshots.filter(
    (snapshot) => snapshot.status === "stale",
  ).length;
  const status =
    criticalCount > 0
      ? "exhausted"
      : warningCount > 0
        ? "warning"
        : staleCount > 0
          ? "stale"
          : snapshots.length > 0
            ? "healthy"
            : "not-configured";
  const label =
    status === "exhausted"
      ? `Burnrate: ${criticalCount} critical`
      : status === "warning"
        ? `Burnrate: ${warningCount} warning`
        : status === "stale"
          ? "Burnrate: data is stale"
          : status === "healthy"
            ? "Burnrate: all quotas healthy"
            : "Burnrate: no enabled accounts";

  return {
    accounts,
    snapshots,
    traySummary: overrides.traySummary ?? {
      label,
      status,
      criticalCount,
      warningCount,
      updatedAt: new Date().toISOString(),
    },
    settings: overrides.settings ?? {
      hideFromDock: false,
      updateChannel: "stable",
    },
  };
}

function snapshot(
  status: UsageSnapshot["status"],
  overrides: Partial<UsageSnapshot> = {},
): UsageSnapshot {
  return {
    accountId: `${status}-account`,
    provider: "codex",
    label: "Codex",
    status,
    subscription: {
      plan: "pro",
      planLabel: "Pro",
      rateLimitTier: null,
      extraUsageEnabled: true,
      source: "test",
    },
    usageBuckets: [
      {
        id: "5-hour",
        label: "5-hour",
        window: "5-hour",
        used: 25,
        limit: 100,
        remaining: 75,
        unit: "%",
        resetAt: new Date(Date.now() + 30 * 60 * 1000).toISOString(),
        status,
      },
    ],
    quota: {
      used: 25,
      limit: 100,
      remaining: 75,
      unit: "%",
      resetAt: null,
    },
    message: null,
    fetchedAt: new Date().toISOString(),
    ...overrides,
  };
}

function accountView(overrides: Partial<AccountView> = {}): AccountView {
  return {
    id: "claude-1",
    provider: "claude-code",
    label: "Claude Code",
    enabled: true,
    autoDetected: false,
    credentialPath: null,
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: false,
    email: null,
    configDir: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    ...overrides,
  };
}

test("shows the account email in the preferences list and usage", async () => {
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      accounts: [
        accountView({ id: "c1", label: "Work", email: "work@example.com" }),
      ],
      snapshots: [
        snapshot("healthy", {
          accountId: "c1",
          provider: "claude-code",
          label: "Work",
          email: "work@example.com",
        }),
      ],
    }),
  );

  render(<App />);

  expect(
    (await screen.findAllByText("work@example.com")).length,
  ).toBeGreaterThan(0);
});

test("completing a sign-in closes the modal and force-refreshes", async () => {
  const user = userEvent.setup();
  render(<App />);
  await screen.findByRole("heading", { name: "Accounts" });

  await user.click(screen.getByRole("button", { name: "Add account" }));
  await user.click(screen.getByRole("menuitem", { name: /Claude Code/ }));
  await user.click(
    screen.getByRole("menuitem", { name: /Sign in with browser/ }),
  );

  expect(api.startAccountLogin).toHaveBeenCalledWith(
    "claude-code",
    "Claude Code",
    undefined,
  );
  expect(
    await screen.findByRole("dialog", { name: /Sign in to Claude Code/ }),
  ).toBeInTheDocument();

  const completeHandler = api.onLoginComplete.mock.calls[0][0];
  await act(async () => {
    await completeHandler({
      id: "pending-1",
      account: accountView({ id: "pending-1", email: "new@example.com" }),
    });
  });

  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(api.guardedFetch).toHaveBeenCalledWith({ force: true });
});

test("a failed sign-in shows the error and retries", async () => {
  const user = userEvent.setup();
  render(<App />);
  await screen.findByRole("heading", { name: "Accounts" });

  await user.click(screen.getByRole("button", { name: "Add account" }));
  await user.click(screen.getByRole("menuitem", { name: /Codex/ }));
  await user.click(
    screen.getByRole("menuitem", { name: /Sign in with browser/ }),
  );

  const failHandler = api.onLoginFailed.mock.calls[0][0];
  await act(async () => {
    failHandler({ id: "pending-1", error: "Sign-in timed out." });
  });

  expect(await screen.findByText("Sign-in timed out.")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(api.startAccountLogin).toHaveBeenCalledTimes(2);
});

test("sign in again re-authenticates the existing account in place", async () => {
  const user = userEvent.setup();
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      accounts: [accountView({ id: "claude-local", label: "Default Claude" })],
      snapshots: [],
    }),
  );

  render(<App />);
  await user.click(await screen.findByText("Default Claude"));
  await user.click(screen.getByRole("button", { name: /Sign in again/ }));

  // The existing account id is threaded through so the backend re-auths in place.
  expect(api.startAccountLogin).toHaveBeenCalledWith(
    "claude-code",
    "Claude Code",
    "claude-local",
  );
});

test("signing out an account calls logoutAccount", async () => {
  const user = userEvent.setup();
  api.guardedFetch.mockResolvedValue(
    dashboardState({
      accounts: [accountView({ id: "c1", label: "Work Claude" })],
      snapshots: [],
    }),
  );
  api.logoutAccount.mockResolvedValue([]);

  render(<App />);
  await user.click(await screen.findByText("Work Claude"));
  await user.click(screen.getByRole("button", { name: /Sign out/ }));

  expect(api.logoutAccount).toHaveBeenCalledWith("c1");
});
