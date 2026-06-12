import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { InsightsPanel } from "./InsightsPanel";
import { LocalUsageSummary } from "./LocalUsageSummary";
import { BarSeries, Sparkline } from "./Sparkline";
import { TrayPanel } from "./TrayPanel";
import { formatUsd } from "./format";
import type {
  DashboardState,
  LocalUsageReport,
  ProviderLocalUsage,
  UsageSnapshot,
} from "./types";

afterEach(() => cleanup());

function providerUsage(
  overrides: Partial<ProviderLocalUsage> = {},
): ProviderLocalUsage {
  return {
    provider: "claude-code",
    todayCostUsd: 1.84,
    todaySessions: 6,
    weekCostUsd: 12.4,
    monthCostUsd: 42.1,
    projectedMonthCostUsd: 96.2,
    monthInputTokens: 1_000_000,
    monthOutputTokens: 50_000,
    topModel: "claude-fable-5",
    modelDistribution: [
      { model: "claude-fable-5", sessions: 41, costUsd: 38.0 },
    ],
    topProjects: [{ project: "burnrate", sessions: 22, costUsd: 31.0 }],
    daily: Array.from({ length: 14 }, (_, i) => ({
      date: `2026-06-${String(i + 1).padStart(2, "0")}`,
      costUsd: i + 0.5,
      sessions: i,
    })),
    ...overrides,
  };
}

function report(providers: ProviderLocalUsage[]): LocalUsageReport {
  return {
    available: true,
    message: null,
    providers,
    generatedAt: new Date().toISOString(),
  };
}

test("formatUsd keeps cents for small amounts and drops them past $100", () => {
  expect(formatUsd(1.842)).toBe("$1.84");
  expect(formatUsd(42.1)).toBe("$42.10");
  expect(formatUsd(196.4)).toBe("$196");
});

test("Sparkline renders one point per value and nothing for short series", () => {
  const { container, rerender } = render(
    <Sparkline values={[1, 4, 2, 8]} label="test series" />,
  );
  const polyline = container.querySelector("polyline");
  expect(polyline?.getAttribute("points")?.split(" ")).toHaveLength(4);
  expect(screen.getByRole("img", { name: "test series" })).toBeInTheDocument();

  rerender(<Sparkline values={[1]} label="too short" />);
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
});

test("BarSeries renders a rect per bucket with zero days as baseline ticks", () => {
  const { container } = render(
    <BarSeries
      items={[
        { label: "2026-06-01", value: 4 },
        { label: "2026-06-02", value: 0 },
        { label: "2026-06-03", value: 8 },
      ]}
      label="daily cost"
    />,
  );
  const rects = container.querySelectorAll("rect");
  expect(rects).toHaveLength(3);
  expect(Number(rects[1].getAttribute("height"))).toBe(1);
  expect(Number(rects[2].getAttribute("height"))).toBeGreaterThan(
    Number(rects[0].getAttribute("height")),
  );
});

test("LocalUsageSummary shows today, projection, and attribution captions", () => {
  const { rerender } = render(
    <LocalUsageSummary usage={providerUsage()} multiAccount={false} />,
  );
  expect(screen.getByText(/Today \$1\.84 · 6 sessions/)).toBeInTheDocument();
  expect(screen.getByText(/MTD \$42\.10 → ~\$96\.20 proj\./)).toBeInTheDocument();
  expect(screen.getByText("local estimate")).toBeInTheDocument();

  rerender(<LocalUsageSummary usage={providerUsage()} multiAccount={true} />);
  expect(
    screen.getByText("local · all Claude Code accounts on this Mac"),
  ).toBeInTheDocument();

  rerender(
    <LocalUsageSummary
      usage={providerUsage({ projectedMonthCostUsd: null, todaySessions: 1 })}
      multiAccount={false}
    />,
  );
  expect(screen.getByText(/Today \$1\.84 · 1 session$/)).toBeInTheDocument();
  expect(screen.queryByText(/proj\./)).not.toBeInTheDocument();
});

test("InsightsPanel walks disabled, collecting, unavailable, and data states", async () => {
  const user = userEvent.setup();
  const onToggle = vi.fn();
  const { rerender } = render(
    <InsightsPanel report={null} enabled={false} onToggle={onToggle} />,
  );
  expect(screen.getByText(/Local insights are off/)).toBeInTheDocument();

  await user.click(screen.getByLabelText("Enabled"));
  expect(onToggle).toHaveBeenCalledWith(true);

  rerender(<InsightsPanel report={null} enabled={true} onToggle={onToggle} />);
  expect(screen.getByText(/Collecting local usage/)).toBeInTheDocument();

  rerender(
    <InsightsPanel
      report={{
        available: false,
        message: "claudex index unavailable",
        providers: [],
        generatedAt: new Date().toISOString(),
      }}
      enabled={true}
      onToggle={onToggle}
    />,
  );
  expect(screen.getByText("claudex index unavailable")).toBeInTheDocument();

  rerender(
    <InsightsPanel
      report={report([providerUsage()])}
      enabled={true}
      onToggle={onToggle}
    />,
  );
  const card = screen.getByRole("article");
  expect(within(card).getByText("Claude Code")).toBeInTheDocument();
  expect(within(card).getByText("Today")).toBeInTheDocument();
  expect(within(card).getByText("~$96.20")).toBeInTheDocument();
  // Appears as the card's top-model badge and in the model distribution list.
  expect(within(card).getAllByText("claude-fable-5")).toHaveLength(2);
  expect(within(card).getByText("burnrate")).toBeInTheDocument();
  expect(screen.getByText(/Computed locally from CLI session logs/))
    .toBeInTheDocument();
});

function traySnapshot(accountId: string, label: string): UsageSnapshot {
  return {
    accountId,
    provider: "claude-code",
    label,
    status: "healthy",
    email: null,
    subscription: null,
    usageBuckets: [],
    quota: null,
    message: null,
    fetchedAt: new Date().toISOString(),
  };
}

test("TrayPanel renders local usage once per provider with a shared-history caption", () => {
  const snapshots = [
    traySnapshot("claude-a", "Claude A"),
    traySnapshot("claude-b", "Claude B"),
  ];
  const state: DashboardState = {
    accounts: [],
    snapshots,
    traySummary: {
      label: "Burnrate: all quotas healthy",
      status: "healthy",
      criticalCount: 0,
      warningCount: 0,
      updatedAt: new Date().toISOString(),
    },
    settings: {
      hideFromDock: true,
      updateChannel: "stable",
      trayScale: 1,
      localInsights: true,
    },
  };

  render(
    <TrayPanel
      state={state}
      snapshots={snapshots}
      busy={false}
      error={null}
      localUsage={report([providerUsage()])}
      onRefresh={vi.fn()}
      onOpenPreferences={vi.fn()}
      onReorderAccounts={vi.fn()}
    />,
  );

  // Provider-level data renders exactly once (under the first card), and the
  // caption is explicit that both accounts share it.
  expect(screen.getAllByText(/Today \$1\.84/)).toHaveLength(1);
  expect(
    screen.getByText("local · all Claude Code accounts on this Mac"),
  ).toBeInTheDocument();
  const cardA = screen.getByText("Claude A").closest(".tray-card");
  expect(
    within(cardA as HTMLElement).getByText(/Today \$1\.84/),
  ).toBeInTheDocument();
});
