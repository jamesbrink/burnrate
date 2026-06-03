import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  __resetFetchGuard,
  guardedFetch,
  isStale,
  markFetched,
  onRefreshRequested,
  readCachedDashboard,
  refreshSnapshots,
  resizeTrayToContent,
  summarizeMockSnapshots,
  writeCachedDashboard,
} from "./api";
import type { DashboardState, SnapshotStatus, UsageSnapshot } from "./types";

beforeEach(() => {
  __resetFetchGuard();
  sessionStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  sessionStorage.clear();
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

test("isStale compares against the freshness threshold", () => {
  const now = 1_000_000;
  expect(isStale(now - 59_000, now)).toBe(false);
  expect(isStale(now - 61_000, now)).toBe(true);
});

test("caches and reads back the dashboard, ignoring missing or corrupt entries", () => {
  expect(readCachedDashboard()).toBeNull();

  writeCachedDashboard(dashboardState(), 5_000);
  const cached = readCachedDashboard();
  expect(cached?.fetchedAt).toBe(5_000);
  expect(cached?.dashboard.snapshots).toHaveLength(1);

  sessionStorage.setItem("burnrate.dashboard.v1", "{not valid json");
  expect(readCachedDashboard()).toBeNull();
});

test("writeCachedDashboard swallows storage failures", () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota exceeded");
  });
  expect(() => writeCachedDashboard(dashboardState())).not.toThrow();
});

test("guardedFetch de-dupes concurrent fetches", async () => {
  const setItem = vi.spyOn(Storage.prototype, "setItem");

  const [first, second] = await Promise.all([guardedFetch(), guardedFetch()]);

  // One underlying fetch → one cache write → both callers share the payload.
  expect(setItem).toHaveBeenCalledTimes(1);
  expect(first).toBe(second);
});

test("guardedFetch throttles non-forced fetches but honors force", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(0);
  const setItem = vi.spyOn(Storage.prototype, "setItem");

  await guardedFetch();
  expect(setItem).toHaveBeenCalledTimes(1);

  // Within the throttle window a non-forced call returns cache (no new write).
  vi.setSystemTime(5_000);
  await guardedFetch();
  expect(setItem).toHaveBeenCalledTimes(1);

  // force bypasses the throttle.
  await guardedFetch({ force: true });
  expect(setItem).toHaveBeenCalledTimes(2);
});

test("guardedFetch honors a fresh persisted cache after a reload resets the guard", async () => {
  // Simulate an HMR/window reload: cache persists in sessionStorage but the
  // module-scoped lastFetchAt is back to 0.
  writeCachedDashboard(dashboardState());
  __resetFetchGuard();
  const setItem = vi.spyOn(Storage.prototype, "setItem");

  const result = await guardedFetch();

  // Served from the fresh cache — no underlying fetch, so no new cache write.
  expect(setItem).not.toHaveBeenCalled();
  expect(result.snapshots).toHaveLength(1);
});

test("markFetched records the dashboard and resets the throttle window", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(0);
  const setItem = vi.spyOn(Storage.prototype, "setItem");

  markFetched(dashboardState());
  expect(readCachedDashboard()?.fetchedAt).toBe(0);

  // A non-forced fetch right after markFetched stays throttled (no new write).
  vi.setSystemTime(3_000);
  await guardedFetch();
  expect(setItem).toHaveBeenCalledTimes(1);
});

test("resizeTrayToContent resolves to a no-op outside Tauri", async () => {
  await expect(resizeTrayToContent(480)).resolves.toBeUndefined();
});

function dashboardState(): DashboardState {
  return {
    accounts: [],
    snapshots: [snapshot("healthy")],
    traySummary: {
      label: "Burnrate: all quotas healthy",
      status: "healthy",
      criticalCount: 0,
      warningCount: 0,
      updatedAt: new Date().toISOString(),
    },
    settings: { hideFromDock: true },
  };
}

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
