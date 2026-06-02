import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { App } from "./App";
import type { AccountView, DashboardState, UsageSnapshot } from "./types";

const api = vi.hoisted(() => ({
  detectAccounts: vi.fn(),
  loadDashboard: vi.fn(),
  onDashboardUpdated: vi.fn(),
  onRefreshRequested: vi.fn(),
  onSettingsUpdated: vi.fn(),
  refreshDashboard: vi.fn(),
  removeAccount: vi.fn(),
  resizePreferencesToContent: vi.fn(),
  saveAccount: vi.fn(),
  saveSettings: vi.fn(),
}));

vi.mock("./api", () => api);

beforeEach(() => {
  vi.clearAllMocks();
  api.onDashboardUpdated.mockResolvedValue(() => {});
  api.onRefreshRequested.mockResolvedValue(() => {});
  api.onSettingsUpdated.mockResolvedValue(() => {});
  api.refreshDashboard.mockResolvedValue(dashboardState());
  api.resizePreferencesToContent.mockResolvedValue(undefined);
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
  api.loadDashboard.mockReturnValue(
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
  api.loadDashboard.mockRejectedValue(new Error("offline"));

  render(<App />);

  expect(await screen.findByRole("alert")).toHaveTextContent("Error: offline");
});

test("renders stale snapshot state", async () => {
  api.loadDashboard.mockResolvedValue(
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
  api.loadDashboard.mockResolvedValue(dashboardState());
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
});

test("renders compact tray view from the tray window route", async () => {
  window.history.replaceState({}, "", "/?view=tray");
  api.loadDashboard.mockResolvedValue(
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
    settings: overrides.settings ?? { hideFromDock: false },
  };
}
