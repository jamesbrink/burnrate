import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { App } from "./App";
import type { AccountView, DashboardState, UsageSnapshot } from "./types";

const api = vi.hoisted(() => ({
  detectAccounts: vi.fn(),
  loadDashboard: vi.fn(),
  onRefreshRequested: vi.fn(),
  refreshSnapshots: vi.fn(),
  removeAccount: vi.fn(),
  resizePreferencesToContent: vi.fn(),
  saveAccount: vi.fn(),
  saveSettings: vi.fn(),
}));

vi.mock("./api", () => api);

beforeEach(() => {
  vi.clearAllMocks();
  api.onRefreshRequested.mockResolvedValue(() => {});
  api.refreshSnapshots.mockResolvedValue([]);
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
          burnRate: { perHour: 3.75, projectedDepletionAt: null },
          message: "Last refresh is older than the quota window.",
          fetchedAt: new Date().toISOString(),
        },
      ],
    }),
  );

  render(<App />);

  expect(await screen.findByText("Stale")).toBeInTheDocument();
  expect(
    screen.getByText("Last refresh is older than the quota window."),
  ).toBeInTheDocument();
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
          burnRate: { perHour: 3.75, projectedDepletionAt: null },
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

  return {
    accounts,
    snapshots,
    traySummary: {
      label:
        snapshots.length > 0
          ? "Burnrate: all quotas healthy"
          : "Burnrate: no enabled accounts",
      status: snapshots.length > 0 ? "healthy" : "not-configured",
      criticalCount: 0,
      warningCount: 0,
      updatedAt: new Date().toISOString(),
    },
    settings: overrides.settings ?? { hideFromDock: false },
  };
}
