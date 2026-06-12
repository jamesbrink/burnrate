import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  __resetFetchGuard,
  __resetMockLogins,
  cancelAccountLogin,
  checkForUpdates,
  detectAccounts,
  getAppVersion,
  guardedFetch,
  installUpdate,
  isStale,
  logoutAccount,
  markFetched,
  notifyUpdateAvailable,
  onCheckUpdateRequested,
  onLoginComplete,
  onLoginProgress,
  onRefreshRequested,
  onUpdateAvailable,
  onUpdateProgress,
  openPreferences,
  readCachedDashboard,
  refreshSnapshots,
  updaterAvailable,
  reorderAccounts,
  resizeTrayToContent,
  startAccountLogin,
  summarizeMockSnapshots,
  writeCachedDashboard,
} from "./api";
import type {
  DashboardState,
  LoginComplete,
  LoginProgress,
  SnapshotStatus,
  UpdateInfo,
  UsageSnapshot,
} from "./types";

beforeEach(() => {
  __resetFetchGuard();
  __resetMockLogins();
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
  await expect(refreshSnapshots()).resolves.toHaveLength(6);
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
  const [first, second] = await Promise.all([guardedFetch(), guardedFetch()]);

  // One underlying fetch → one cache entry → both callers share the payload.
  expect(readCachedDashboard()?.dashboard.snapshots).toHaveLength(6);
  expect(first).toBe(second);
});

test("guardedFetch throttles non-forced fetches but honors force", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(0);

  await guardedFetch();
  expect(readCachedDashboard()?.fetchedAt).toBe(0);

  // Within the throttle window a non-forced call returns cache (no new write).
  vi.setSystemTime(5_000);
  const throttled = await guardedFetch();
  expect(readCachedDashboard()?.fetchedAt).toBe(0);

  // force bypasses the throttle.
  const forced = await guardedFetch({ force: true });
  expect(readCachedDashboard()?.fetchedAt).toBe(5_000);
  expect(forced).not.toBe(throttled);
});

test("guardedFetch honors a fresh persisted cache after a reload resets the guard", async () => {
  // Simulate an HMR/window reload: cache persists in sessionStorage but the
  // module-scoped lastFetchAt is back to 0.
  writeCachedDashboard(dashboardState());
  const cachedAt = readCachedDashboard()?.fetchedAt;
  __resetFetchGuard();

  const result = await guardedFetch();

  // Served from the fresh cache — no underlying fetch, so no new cache write.
  expect(readCachedDashboard()?.fetchedAt).toBe(cachedAt);
  expect(result.snapshots).toHaveLength(1);
});

test("markFetched records the dashboard and resets the throttle window", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(0);

  markFetched(dashboardState());
  expect(readCachedDashboard()?.fetchedAt).toBe(0);

  // A non-forced fetch right after markFetched stays throttled (no new write).
  vi.setSystemTime(3_000);
  await guardedFetch();
  expect(readCachedDashboard()?.fetchedAt).toBe(0);
});

test("resizeTrayToContent resolves to a no-op outside Tauri", async () => {
  await expect(
    resizeTrayToContent({ width: 440, height: 480 }),
  ).resolves.toBeUndefined();
});

test("reorderAccounts reorders the mock array and drops unknown ids", async () => {
  const before = await detectAccounts();
  const reversed = before.map((account) => account.id).reverse();

  const reordered = await reorderAccounts([...reversed, "ghost-id"]);

  expect(reordered.map((account) => account.id)).toEqual(reversed);
});

test("startAccountLogin drives mock progress and completion events", async () => {
  vi.useFakeTimers();
  const progress: LoginProgress[] = [];
  const completed: LoginComplete[] = [];
  const offProgress = await onLoginProgress((event) => {
    progress.push(event);
  });
  const offComplete = await onLoginComplete((event) => {
    completed.push(event);
  });

  const pending = await startAccountLogin("claude-code", "Work");
  expect(pending.enabled).toBe(false);
  expect(pending.id).toMatch(/^claude-code-/);

  await vi.advanceTimersByTimeAsync(60);
  expect(last(progress)?.url).toContain("https://");

  await vi.advanceTimersByTimeAsync(120);
  expect(last(completed)?.account.id).toBe(pending.id);
  expect(last(completed)?.account.email).toContain("@");
  expect(last(completed)?.account.enabled).toBe(true);

  offProgress();
  offComplete();
});

test("cancelAccountLogin stops a pending mock login before completion", async () => {
  vi.useFakeTimers();
  const completed: LoginComplete[] = [];
  const offComplete = await onLoginComplete((event) => {
    completed.push(event);
  });

  const pending = await startAccountLogin("codex", "Second");
  await cancelAccountLogin(pending.id);
  await vi.advanceTimersByTimeAsync(300);

  expect(completed).toHaveLength(0);
  offComplete();
});

test("__resetMockLogins cancels any in-flight mock login timers", async () => {
  vi.useFakeTimers();
  const completed: LoginComplete[] = [];
  const offComplete = await onLoginComplete((event) => {
    completed.push(event);
  });

  await startAccountLogin("claude-code", "X");
  __resetMockLogins();
  await vi.advanceTimersByTimeAsync(300);

  expect(completed).toHaveLength(0);
  offComplete();
});

test("logoutAccount removes the account from the mock list", async () => {
  const before = await detectAccounts();
  const target = before[0].id;

  const after = await logoutAccount(target);

  expect(after.find((account) => account.id === target)).toBeUndefined();
});

test("updater mock is dormant unless VITE_MOCK_UPDATE is set", async () => {
  expect(await updaterAvailable()).toBe(false);
  expect(await checkForUpdates("stable")).toBeNull();
  expect(await getAppVersion()).toBe("dev");
  // No-op outside Tauri — just shouldn't throw.
  await openPreferences();
});

test("updater mock advertises an update when opted in", async () => {
  vi.stubEnv("VITE_MOCK_UPDATE", "1");
  try {
    expect(await updaterAvailable()).toBe(true);
    const info = await checkForUpdates("nightly");
    expect(info?.version).toBe("9.9.9");
  } finally {
    vi.unstubAllEnvs();
  }
});

test("installUpdate streams mock progress events", async () => {
  vi.useFakeTimers();
  const seen: number[] = [];
  const unlisten = await onUpdateProgress((pct) => {
    seen.push(pct);
  });

  await installUpdate("9.9.9");
  vi.advanceTimersByTime(500);

  expect(seen).toContain(0);
  expect(seen).toContain(100);
  unlisten();
});

test("onCheckUpdateRequested bridges window events", async () => {
  const handler = vi.fn();
  const unlisten = await onCheckUpdateRequested(handler);

  window.dispatchEvent(new CustomEvent("burnrate-check-update-requested"));
  expect(handler).toHaveBeenCalledOnce();

  unlisten();
  window.dispatchEvent(new CustomEvent("burnrate-check-update-requested"));
  expect(handler).toHaveBeenCalledOnce();
});

test("mock checkForUpdates broadcasts the result like the backend", async () => {
  const seen: (UpdateInfo | null)[] = [];
  const unlisten = await onUpdateAvailable((info) => {
    seen.push(info);
  });

  await checkForUpdates("stable");
  expect(seen).toEqual([null]);

  vi.stubEnv("VITE_MOCK_UPDATE", "1");
  try {
    await checkForUpdates("stable");
  } finally {
    vi.unstubAllEnvs();
  }
  expect(seen[1]?.version).toBe("9.9.9");

  unlisten();
  await checkForUpdates("stable");
  expect(seen).toHaveLength(2);
});

test("notifyUpdateAvailable is a no-op outside Tauri", async () => {
  await expect(notifyUpdateAvailable("9.9.9")).resolves.toBeUndefined();
});

function last<T>(items: T[]): T | undefined {
  return items[items.length - 1];
}

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
    settings: {
      hideFromDock: true,
      updateChannel: "stable",
      trayScale: 1,
      localInsights: true,
    },
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
