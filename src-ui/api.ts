import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AccountInput,
  AccountView,
  AppSettings,
  DashboardState,
  UsageSnapshot,
} from "./types";

const isTauri = "__TAURI_INTERNALS__" in window;

// Per-window dashboard cache + fetch guard. During `tauri dev`, HMR reloads
// remount the app and would otherwise re-fetch on every save, and opening the
// tray popover emits a refresh — bursts that hammer the rate-limited provider
// APIs (429s). We hydrate instantly from sessionStorage and collapse bursts:
// in-flight de-dupe + a min-interval throttle for non-forced fetches.
const DASHBOARD_CACHE_KEY = "burnrate.dashboard.v1";
const STALE_THRESHOLD_MS = 60_000;
const MIN_FETCH_INTERVAL_MS = 10_000;

let mockAccounts: AccountView[] = [
  {
    id: "claude-code-local",
    provider: "claude-code",
    label: "Claude Code",
    enabled: true,
    autoDetected: true,
    credentialPath: "~/.claude/.credentials.json",
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  },
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
  {
    id: "openrouter-main",
    provider: "openrouter",
    label: "OpenRouter",
    enabled: true,
    autoDetected: false,
    credentialPath: null,
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  },
  {
    id: "runpod-main",
    provider: "runpod",
    label: "Runpod",
    enabled: true,
    autoDetected: false,
    credentialPath: null,
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  },
];

let mockSettings: AppSettings = {
  hideFromDock: true,
};

const mockSnapshots: UsageSnapshot[] = [
  {
    accountId: "claude-code-local",
    provider: "claude-code",
    label: "Claude Code",
    status: "healthy",
    subscription: {
      plan: "max",
      planLabel: "Max 20x",
      rateLimitTier: "default_claude_max_20x",
      extraUsageEnabled: true,
      source: "claude-local-metadata",
    },
    usageBuckets: [
      {
        id: "5-hour",
        label: "5-hour",
        window: "5-hour",
        used: 528000,
        limit: 1000000,
        remaining: 472000,
        unit: "tokens",
        resetAt: new Date(Date.now() + 90 * 60 * 1000).toISOString(),
        status: "healthy",
      },
      {
        id: "weekly",
        label: "Weekly",
        window: "weekly",
        used: 2300000,
        limit: 6000000,
        remaining: 3700000,
        unit: "tokens",
        resetAt: new Date(Date.now() + 4 * 24 * 60 * 60 * 1000).toISOString(),
        status: "healthy",
      },
    ],
    quota: {
      used: 528000,
      limit: 1000000,
      remaining: 472000,
      unit: "tokens",
      resetAt: null,
    },
    message: null,
    fetchedAt: new Date().toISOString(),
  },
  {
    accountId: "codex-local",
    provider: "codex",
    label: "Codex",
    status: "warning",
    subscription: {
      plan: "pro",
      planLabel: "Pro",
      rateLimitTier: "chatgpt_pro",
      extraUsageEnabled: null,
      source: "codex-app-server",
    },
    usageBuckets: [
      {
        id: "5-hour",
        label: "5-hour",
        window: "5-hour",
        used: 81,
        limit: 100,
        remaining: 19,
        unit: "requests",
        resetAt: new Date(Date.now() + 42 * 60 * 1000).toISOString(),
        status: "warning",
      },
      {
        id: "weekly",
        label: "Weekly",
        window: "weekly",
        used: 210,
        limit: 500,
        remaining: 290,
        unit: "requests",
        resetAt: new Date(Date.now() + 5 * 24 * 60 * 60 * 1000).toISOString(),
        status: "healthy",
      },
    ],
    quota: {
      used: 81,
      limit: 100,
      remaining: 19,
      unit: "requests",
      resetAt: null,
    },
    message: null,
    fetchedAt: new Date().toISOString(),
  },
  {
    accountId: "openrouter-main",
    provider: "openrouter",
    label: "OpenRouter",
    status: "healthy",
    subscription: null,
    usageBuckets: [
      {
        id: "credits",
        label: "Credits",
        window: null,
        used: 7.25,
        limit: 25,
        remaining: 17.75,
        unit: "USD",
        resetAt: null,
        status: "healthy",
      },
    ],
    quota: {
      used: 7.25,
      limit: 25,
      remaining: 17.75,
      unit: "USD",
      resetAt: null,
    },
    message: null,
    fetchedAt: new Date().toISOString(),
  },
  {
    accountId: "runpod-main",
    provider: "runpod",
    label: "Runpod",
    status: "healthy",
    subscription: null,
    usageBuckets: [
      {
        id: "balance",
        label: "Balance",
        window: null,
        used: 0,
        limit: null,
        remaining: 42.5,
        unit: "USD",
        resetAt: null,
        status: "healthy",
      },
      {
        id: "current-burn",
        label: "Current burn",
        window: null,
        used: 1.25,
        limit: 10,
        remaining: null,
        unit: "USD/hr",
        resetAt: null,
        status: "healthy",
      },
      {
        id: "pods-24h",
        label: "Pods 24h",
        window: "24h",
        used: 3.4,
        limit: null,
        remaining: null,
        unit: "USD",
        resetAt: null,
        status: "healthy",
      },
    ],
    quota: {
      used: 0,
      limit: null,
      remaining: 42.5,
      unit: "USD",
      resetAt: null,
    },
    message: "balance $42.50 · burn $1.25/hr · runway 34.0h",
    fetchedAt: new Date().toISOString(),
  },
];

export async function loadDashboard(): Promise<DashboardState> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<DashboardState>("dashboard");
  }
  return {
    accounts: mockAccounts,
    snapshots: mockSnapshots,
    traySummary: {
      label: "Burnrate: 1 warning",
      status: "warning",
      criticalCount: 0,
      warningCount: 1,
      updatedAt: new Date().toISOString(),
    },
    settings: mockSettings,
  };
}

export async function saveAccount(input: AccountInput): Promise<AccountView[]> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<AccountView[]>("save_account", { input });
  }

  const now = new Date().toISOString();
  const id = input.id ?? `${input.provider}-${crypto.randomUUID()}`;
  const account: AccountView = {
    id,
    provider: input.provider,
    label: input.label,
    enabled: input.enabled,
    autoDetected: false,
    credentialPath: null,
    endpointOverride: input.endpointOverride ?? null,
    secretStorage: input.secretStorage,
    hasSecret: Boolean(input.secret),
    createdAt: now,
    updatedAt: now,
  };
  mockAccounts = [...mockAccounts.filter((item) => item.id !== id), account];
  return mockAccounts;
}

export async function removeAccount(id: string): Promise<AccountView[]> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<AccountView[]>("remove_account", { id });
  }
  mockAccounts = mockAccounts.filter((account) => account.id !== id);
  return mockAccounts;
}

export async function saveSettings(
  settings: AppSettings,
): Promise<AppSettings> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<AppSettings>("save_settings", { settings });
  }

  mockSettings = settings;
  return mockSettings;
}

export async function detectAccounts(): Promise<AccountView[]> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<AccountView[]>("detect_accounts");
  }
  return mockAccounts;
}

export async function refreshSnapshots(): Promise<UsageSnapshot[]> {
  return (await refreshDashboard()).snapshots;
}

export async function refreshDashboard(): Promise<DashboardState> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<DashboardState>("refresh_snapshots");
  }
  const snapshots = mockSnapshots.map((snapshot) => ({
    ...snapshot,
    fetchedAt: new Date().toISOString(),
  }));
  return {
    accounts: mockAccounts,
    snapshots,
    traySummary: summarizeMockSnapshots(snapshots),
    settings: mockSettings,
  };
}

export interface CachedDashboard {
  dashboard: DashboardState;
  fetchedAt: number;
}

export function readCachedDashboard(): CachedDashboard | null {
  try {
    const raw = sessionStorage.getItem(DASHBOARD_CACHE_KEY);
    if (!raw) {
      return null;
    }
    const parsed = JSON.parse(raw) as CachedDashboard;
    if (
      !parsed ||
      typeof parsed.fetchedAt !== "number" ||
      typeof parsed.dashboard !== "object" ||
      parsed.dashboard === null
    ) {
      return null;
    }
    return parsed;
  } catch {
    return null;
  }
}

export function writeCachedDashboard(
  dashboard: DashboardState,
  now: number = Date.now(),
): void {
  try {
    const entry: CachedDashboard = { dashboard, fetchedAt: now };
    sessionStorage.setItem(DASHBOARD_CACHE_KEY, JSON.stringify(entry));
  } catch {
    // sessionStorage may be unavailable (privacy mode) or full — caching is
    // best-effort, so a failure here is non-fatal.
  }
}

export function isStale(fetchedAt: number, now: number = Date.now()): boolean {
  return now - fetchedAt > STALE_THRESHOLD_MS;
}

let inFlightFetch: Promise<DashboardState> | null = null;
let lastFetchAt = 0;

/** Record a fetched dashboard in the cache and reset the throttle window. */
export function markFetched(
  dashboard: DashboardState,
  now: number = Date.now(),
): void {
  lastFetchAt = now;
  writeCachedDashboard(dashboard, now);
}

/**
 * Fetch the dashboard with burst protection. Concurrent calls share one
 * in-flight request; non-forced calls within `MIN_FETCH_INTERVAL_MS` of the
 * last fetch return cached data instead of hitting the backend. `force: true`
 * (manual refresh / post-save) bypasses the interval but still de-dupes against
 * an in-flight request.
 */
export async function guardedFetch(
  options: { force?: boolean } = {},
): Promise<DashboardState> {
  if (inFlightFetch) {
    return inFlightFetch;
  }
  if (!options.force) {
    const cached = readCachedDashboard();
    // Throttle against the persisted timestamp too: after an HMR/window reload
    // the module-scoped `lastFetchAt` resets to 0, but a fresh sessionStorage
    // cache should still suppress the next non-forced fetch.
    if (cached) {
      const lastAt = Math.max(lastFetchAt, cached.fetchedAt);
      if (Date.now() - lastAt < MIN_FETCH_INTERVAL_MS) {
        return cached.dashboard;
      }
    }
  }
  inFlightFetch = refreshDashboard()
    .then((dashboard) => {
      markFetched(dashboard);
      return dashboard;
    })
    .finally(() => {
      inFlightFetch = null;
    });
  return inFlightFetch;
}

/** Reset the module-scoped fetch guard. Exposed for deterministic tests. */
export function __resetFetchGuard(): void {
  inFlightFetch = null;
  lastFetchAt = 0;
}

export async function resizePreferencesToContent(
  width: number,
  height: number,
): Promise<void> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    await invoke("resize_preferences_to_content", { width, height });
  }
}

export async function resizeTrayToContent(height: number): Promise<void> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    await invoke("resize_tray_to_content", { height });
  }
}

export async function closePreferences(): Promise<void> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    await invoke("close_preferences");
  }
}

export async function onRefreshRequested(handler: () => void | Promise<void>) {
  /* v8 ignore next 3: native Tauri event path */
  if (isTauri) {
    return listen("burnrate-refresh-requested", handler);
  }

  function listener() {
    void handler();
  }
  window.addEventListener("burnrate-refresh-requested", listener);
  return () =>
    window.removeEventListener("burnrate-refresh-requested", listener);
}

export async function onDashboardUpdated(
  handler: (dashboard: DashboardState) => void | Promise<void>,
) {
  /* v8 ignore next 3: native Tauri event path */
  if (isTauri) {
    return listen<DashboardState>("burnrate-dashboard-updated", (event) =>
      handler(event.payload),
    );
  }

  return () => {};
}

export async function onSettingsUpdated(
  handler: (settings: AppSettings) => void | Promise<void>,
) {
  /* v8 ignore next 3: native Tauri event path */
  if (isTauri) {
    return listen<AppSettings>("burnrate-settings-updated", (event) =>
      handler(event.payload),
    );
  }

  return () => {};
}

export function summarizeMockSnapshots(snapshots: UsageSnapshot[]) {
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
    label,
    status,
    criticalCount,
    warningCount,
    updatedAt: new Date().toISOString(),
  } as const;
}
