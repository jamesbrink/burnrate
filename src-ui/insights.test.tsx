import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { InsightsPanel, shortenProjectPath } from "./InsightsPanel";
import { LocalUsageSummary } from "./LocalUsageSummary";
import { BarSeries, Sparkline } from "./Sparkline";
import { TrayPanel } from "./TrayPanel";
import { formatAgo, formatTokenCount, formatUsd } from "./format";
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

test("shortenProjectPath keeps short names and trims deep paths to two segments", () => {
  expect(shortenProjectPath("burnrate")).toBe("burnrate");
  expect(shortenProjectPath("utensils/aethon")).toBe("utensils/aethon");
  // Short paths are normalized — no stray leading/trailing slashes.
  expect(shortenProjectPath("/foo/bar/")).toBe("foo/bar");
  expect(shortenProjectPath("/")).toBe("/");
  expect(shortenProjectPath("/Users/jamesbrink/Projects/utensils/aethon")).toBe(
    "…/utensils/aethon",
  );
});

test("InsightsPanel shows shortened project paths with the full path as tooltip", () => {
  render(
    <InsightsPanel
      report={report([
        providerUsage({
          topProjects: [
            {
              project: "/Users/jamesbrink/Projects/utensils/aethon",
              sessions: 4,
              costUsd: 9.5,
            },
          ],
        }),
      ])}
      enabled={true}
      onToggle={vi.fn()}
    />,
  );

  const entry = screen.getByText("…/utensils/aethon");
  expect(entry).toHaveAttribute(
    "title",
    "/Users/jamesbrink/Projects/utensils/aethon",
  );
});

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
  expect(
    screen.getByText(/MTD \$42\.10 → ~\$96\.20 proj\./),
  ).toBeInTheDocument();
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
  expect(
    screen.getByText(/Computed locally from CLI session logs/),
  ).toBeInTheDocument();
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

test("formatAgo walks just-now, minutes, hours, and date branches", () => {
  expect(formatAgo(null)).toBe("");
  expect(formatAgo("not a date")).toBe("");
  expect(formatAgo(new Date().toISOString())).toBe("just now");
  expect(formatAgo(new Date(Date.now() - 3 * 60 * 1000).toISOString())).toBe(
    "3m ago",
  );
  // Floors, never rounds: 90 seconds is still "1m ago".
  expect(formatAgo(new Date(Date.now() - 90 * 1000).toISOString())).toBe(
    "1m ago",
  );
  expect(
    formatAgo(new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString()),
  ).toBe("2h ago");
  expect(
    formatAgo(new Date(Date.now() - 80 * 60 * 60 * 1000).toISOString()),
  ).toMatch(/^[A-Z][a-z]{2} \d{1,2}$/);
});

test("formatTokenCount compacts with unit suffixes", () => {
  expect(formatTokenCount(931)).toBe("931");
  expect(formatTokenCount(1_400)).toBe("1.4k");
  expect(formatTokenCount(820_000)).toBe("820k");
  expect(formatTokenCount(1_000_000)).toBe("1M");
  expect(formatTokenCount(48_200_000)).toBe("48.2M");
  expect(formatTokenCount(2_500_000_000)).toBe("2.5B");
});

test("Sparkline exposes per-day hover titles when given", () => {
  const { container, rerender } = render(
    <LocalUsageSummary usage={providerUsage()} multiAccount={false} />,
  );
  const titles = Array.from(container.querySelectorAll("svg title")).map(
    (node) => node.textContent,
  );
  expect(titles).toContain("2026-06-01 · $0.50");
  expect(titles).toHaveLength(14);

  rerender(<Sparkline values={[1, 2, 3]} label="untitled" />);
  expect(container.querySelectorAll("svg title")).toHaveLength(0);

  // A titles array that doesn't match the series is ignored entirely rather
  // than rendering empty tooltips.
  rerender(
    <Sparkline values={[1, 2, 3]} titles={["only one"]} label="mismatched" />,
  );
  expect(container.querySelectorAll("svg title")).toHaveLength(0);
});

function detailSnapshot(
  accountId: string,
  overrides: Partial<UsageSnapshot> = {},
): UsageSnapshot {
  return {
    ...traySnapshot(accountId, accountId),
    subscription: {
      plan: "max",
      planLabel: "Max 20x",
      rateLimitTier: "default_claude_max_20x",
      extraUsageEnabled: null,
      source: "test",
    },
    usageBuckets: [
      {
        id: "5-hour",
        label: "5-hour",
        window: "5-hour",
        used: 10,
        limit: 100,
        remaining: 90,
        unit: "requests",
        resetAt: null,
        status: "healthy",
      },
      {
        // Value-less: filtered from the compact card, shown in the details.
        id: "extra-usage",
        label: "Extra usage",
        window: "monthly",
        used: 0,
        limit: null,
        remaining: null,
        unit: "USD",
        resetAt: null,
        status: "healthy",
      },
    ],
    ...overrides,
  };
}

function trayPanel(
  snapshots: UsageSnapshot[],
  options: {
    localUsage?: LocalUsageReport | null;
    onOpenPreferences?: () => void;
  } = {},
) {
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
  return (
    <TrayPanel
      state={state}
      snapshots={snapshots}
      busy={false}
      error={null}
      localUsage={options.localUsage ?? null}
      onRefresh={vi.fn()}
      onOpenPreferences={options.onOpenPreferences ?? vi.fn()}
      onReorderAccounts={vi.fn()}
    />
  );
}

test("expanding a tray card reveals snapshot details and collapses again", async () => {
  const user = userEvent.setup();
  render(trayPanel([detailSnapshot("claude-a")]));

  const toggle = screen.getByTitle("Show details");
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  // Hidden value-less buckets stay out of the compact card.
  expect(screen.queryByText("Extra usage")).not.toBeInTheDocument();

  await user.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("Updated")).toBeInTheDocument();
  expect(screen.getByText("just now")).toBeInTheDocument();
  expect(screen.getByText("Tier")).toBeInTheDocument();
  expect(screen.getByText("default claude max 20x")).toBeInTheDocument();
  expect(screen.getByText("Extra usage")).toBeInTheDocument();

  await user.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("Updated")).not.toBeInTheDocument();
});

test("shows AWS Cost Explorer snapshot age without expanding the tray card", () => {
  const snapshot = detailSnapshot("aws-main");
  snapshot.provider = "aws";
  snapshot.label = "AWS";
  snapshot.usageBuckets[0].id = "aws-mtd";

  render(trayPanel([snapshot]));

  expect(screen.getByText(/^AWS cost data · just now$/)).toBeInTheDocument();
});

test("does not label an AWS error attempt as cached cost data", () => {
  const snapshot = detailSnapshot("aws-main");
  snapshot.provider = "aws";
  snapshot.label = "AWS";
  snapshot.status = "error";
  snapshot.usageBuckets = [];
  snapshot.quota = null;
  snapshot.message = "AWS SSO token expired";

  render(trayPanel([snapshot]));

  expect(screen.queryByText(/^AWS cost data/)).not.toBeInTheDocument();
  expect(screen.getByText("AWS SSO token expired")).toBeInTheDocument();
});

test("only one tray card expands at a time", async () => {
  const user = userEvent.setup();
  render(trayPanel([detailSnapshot("claude-a"), detailSnapshot("claude-b")]));

  const [toggleA, toggleB] = screen.getAllByTitle("Show details");
  await user.click(toggleA);
  expect(toggleA).toHaveAttribute("aria-expanded", "true");

  await user.click(toggleB);
  expect(toggleB).toHaveAttribute("aria-expanded", "true");
  expect(toggleA).toHaveAttribute("aria-expanded", "false");
});

test("local-usage detail renders only on the provider's first card", async () => {
  const user = userEvent.setup();
  render(
    trayPanel([detailSnapshot("claude-a"), detailSnapshot("claude-b")], {
      localUsage: report([providerUsage()]),
    }),
  );

  const [toggleA, toggleB] = screen.getAllByTitle("Show details");
  await user.click(toggleA);
  expect(screen.getByText("This month · local")).toBeInTheDocument();
  expect(screen.getByText("Past week")).toBeInTheDocument();
  expect(screen.getByText("1M in · 50k out")).toBeInTheDocument();
  // Project paths reduce to their leaf directory in the tray.
  const details = document.querySelector(".tray-card-details");
  expect(
    within(details as HTMLElement).getByText("burnrate"),
  ).toBeInTheDocument();

  await user.click(toggleB);
  expect(screen.queryByText("This month · local")).not.toBeInTheDocument();
});

test("tray footer counts disabled accounts and opens preferences", async () => {
  const user = userEvent.setup();
  const onOpenPreferences = vi.fn();
  const snapshots = [detailSnapshot("claude-a")];
  const state: DashboardState = {
    accounts: [
      {
        id: "off-1",
        provider: "codex",
        label: "Codex",
        enabled: false,
        autoDetected: false,
        credentialPath: null,
        endpointOverride: null,
        secretStorage: "keyring",
        hasSecret: false,
        email: null,
        configDir: null,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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
      onRefresh={vi.fn()}
      onOpenPreferences={onOpenPreferences}
      onReorderAccounts={vi.fn()}
    />,
  );

  expect(screen.getByText("1 account off")).toBeInTheDocument();
  expect(screen.getByText(/^Updated just now$/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Manage" }));
  expect(onOpenPreferences).toHaveBeenCalledOnce();
});
