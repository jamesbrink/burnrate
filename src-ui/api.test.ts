import { afterEach, expect, test, vi } from "vitest";
import {
  onRefreshRequested,
  refreshSnapshots,
  summarizeMockSnapshots,
} from "./api";
import type { SnapshotStatus, UsageSnapshot } from "./types";

afterEach(() => {
  vi.restoreAllMocks();
});

test("summarizes mock snapshots across fallback states", () => {
  expect(summarizeMockSnapshots([snapshot("error")])).toMatchObject({
    label: "Burnrate: 1 critical",
    status: "exhausted",
    criticalCount: 1,
    warningCount: 0,
  });
  expect(summarizeMockSnapshots([snapshot("warning")])).toMatchObject({
    label: "Burnrate: 1 warning",
    status: "warning",
    criticalCount: 0,
    warningCount: 1,
  });
  expect(summarizeMockSnapshots([snapshot("stale")])).toMatchObject({
    label: "Burnrate: data is stale",
    status: "stale",
    criticalCount: 0,
    warningCount: 0,
  });
  expect(summarizeMockSnapshots([snapshot("healthy")])).toMatchObject({
    label: "Burnrate: all quotas healthy",
    status: "healthy",
  });
  expect(summarizeMockSnapshots([])).toMatchObject({
    label: "Burnrate: no enabled accounts",
    status: "not-configured",
  });
});

test("wires browser refresh events and refresh snapshot fallback", async () => {
  const handler = vi.fn();
  const unlisten = await onRefreshRequested(handler);

  window.dispatchEvent(new Event("burnrate-refresh-requested"));

  expect(handler).toHaveBeenCalledOnce();

  unlisten();
  window.dispatchEvent(new Event("burnrate-refresh-requested"));

  expect(handler).toHaveBeenCalledOnce();
  await expect(refreshSnapshots()).resolves.toHaveLength(3);
});

function snapshot(status: SnapshotStatus): UsageSnapshot {
  return {
    accountId: `${status}-account`,
    provider: "codex",
    label: "Codex",
    status,
    subscription: null,
    usageBuckets: [],
    quota: null,
    message: null,
    fetchedAt: new Date().toISOString(),
  };
}
