import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { cloneDefaultAwsCategories } from "./constants";
import type {
  AccountInput,
  AccountView,
  AppSettings,
  DashboardState,
  LoginComplete,
  LoginFailed,
  LoginProgress,
  ProviderKind,
  UpdateChannel,
  UpdateInfo,
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
    email: "dev@example.com",
    configDir: null,
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
    email: "codex@example.com",
    configDir: null,
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
    email: null,
    configDir: null,
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
    email: null,
    configDir: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  },
  {
    id: "aws-main",
    provider: "aws",
    label: "AWS",
    enabled: true,
    autoDetected: false,
    credentialPath: null,
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: false,
    email: null,
    configDir: null,
    awsProfile: "work",
    awsRegion: "us-east-1",
    awsMonthlyBudgetUsd: 200,
    awsCategories: cloneDefaultAwsCategories(),
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  },
];

let mockSettings: AppSettings = {
  hideFromDock: true,
  updateChannel: "stable",
};

const mockSnapshots: UsageSnapshot[] = [
  {
    accountId: "claude-code-local",
    provider: "claude-code",
    label: "Claude Code",
    status: "healthy",
    email: "dev@example.com",
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
    email: "codex@example.com",
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
    accountId: "aws-main",
    provider: "aws",
    label: "AWS",
    status: "warning",
    subscription: null,
    usageBuckets: [
      {
        id: "aws-mtd",
        label: "AWS month-to-date",
        window: "month-to-date",
        used: 164.25,
        limit: 200,
        remaining: 35.75,
        unit: "USD",
        resetAt: null,
        status: "warning",
      },
      {
        id: "aws-category-bedrock",
        label: "Bedrock",
        window: "month-to-date",
        used: 48.5,
        limit: null,
        remaining: null,
        unit: "USD",
        resetAt: null,
        status: "healthy",
      },
      {
        id: "aws-category-ec2-compute",
        label: "EC2 compute",
        window: "month-to-date",
        used: 72.1,
        limit: null,
        remaining: null,
        unit: "USD",
        resetAt: null,
        status: "healthy",
      },
    ],
    quota: {
      used: 164.25,
      limit: 200,
      remaining: 35.75,
      unit: "USD",
      resetAt: null,
    },
    message:
      "AWS account 123456789012 · Cost Explorer marks current data as estimated",
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
    traySummary: summarizeMockSnapshots(mockSnapshots),
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
    email: null,
    configDir: null,
    awsProfile: input.awsProfile ?? null,
    awsRegion: input.awsRegion ?? null,
    awsMonthlyBudgetUsd: input.awsMonthlyBudgetUsd ?? null,
    awsCategories: input.awsCategories ?? [],
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

/**
 * Persist a new global account order. `ids` is the full ordered list; the
 * backend re-normalizes and returns the updated views. Outside Tauri, reorders
 * the mock array (unknown ids dropped, unnamed accounts appended).
 */
export async function reorderAccounts(ids: string[]): Promise<AccountView[]> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<AccountView[]>("reorder_accounts", { ids });
  }
  const byId = new Map(mockAccounts.map((account) => [account.id, account]));
  const ordered = ids
    .map((id) => byId.get(id))
    .filter((account): account is AccountView => Boolean(account));
  const rest = mockAccounts.filter((account) => !ids.includes(account.id));
  mockAccounts = [...ordered, ...rest];
  return mockAccounts;
}

/**
 * Start an interactive browser sign-in for a Claude Code / Codex account. The
 * backend returns a disabled placeholder account immediately and then emits
 * `burnrate-login-progress` / `-complete` / `-failed`. Outside Tauri this drives
 * a simulated login via window CustomEvents so dev:web and tests work.
 */
export async function startAccountLogin(
  provider: ProviderKind,
  label: string,
  accountId?: string | null,
): Promise<AccountView> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<AccountView>("start_account_login", {
      provider,
      label,
      accountId: accountId ?? null,
    });
  }
  const now = new Date().toISOString();
  const id = `${provider}-${crypto.randomUUID()}`;
  queueMockLogin(id, provider, label);
  return {
    id,
    provider,
    label,
    enabled: false,
    autoDetected: false,
    credentialPath: null,
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: false,
    email: null,
    configDir: `mock/cli/${provider}/${id}`,
    createdAt: now,
    updatedAt: now,
  };
}

export async function cancelAccountLogin(id: string): Promise<AccountView[]> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<AccountView[]>("cancel_account_login", { id });
  }
  clearMockLogin(id);
  mockAccounts = mockAccounts.filter((account) => account.id !== id);
  return mockAccounts;
}

export async function submitLoginCode(id: string, code: string): Promise<void> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<void>("submit_account_login_code", { id, code });
  }
}

export async function logoutAccount(id: string): Promise<AccountView[]> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<AccountView[]>("logout_account", { id });
  }
  mockAccounts = mockAccounts.filter((account) => account.id !== id);
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

export async function openPreferences(): Promise<void> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    await invoke("open_preferences");
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

// --- Auto-updater ----------------------------------------------------------

// Outside Tauri (dev:web / vitest) the updater is simulated so the banner and
// install flow can be exercised without the Rust backend. `VITE_MOCK_UPDATE`
// (set in dev:web) makes the mock advertise an available update; tests leave it
// unset so the banner stays hidden unless they opt in.
const MOCK_UPDATE: UpdateInfo = {
  version: "9.9.9",
  currentVersion: "0.0.0",
  body: "Mock release notes for local preview.",
  date: null,
};

export async function getAppVersion(): Promise<string> {
  /* v8 ignore next 4: native Tauri path */
  if (isTauri) {
    const { getVersion } = await import("@tauri-apps/api/app");
    return getVersion();
  }
  return "dev";
}

export async function updaterAvailable(): Promise<boolean> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<boolean>("updater_available");
  }
  return import.meta.env.VITE_MOCK_UPDATE === "1";
}

export async function checkForUpdates(
  channel: UpdateChannel,
): Promise<UpdateInfo | null> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<UpdateInfo | null>("check_for_updates", { channel });
  }
  return import.meta.env.VITE_MOCK_UPDATE === "1" ? MOCK_UPDATE : null;
}

export async function installUpdate(version: string): Promise<void> {
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    await invoke("install_pending_update", { version });
    return;
  }
  // Mock: stream a few progress ticks so the UI animates, then stop (a real
  // install restarts the app, so there's nothing after 100%).
  for (let pct = 0; pct <= 100; pct += 25) {
    setTimeout(() => {
      window.dispatchEvent(
        new CustomEvent<number>("burnrate-update-progress", { detail: pct }),
      );
    }, pct * 4);
  }
}

export async function onUpdateProgress(
  handler: (percent: number) => void | Promise<void>,
) {
  return onLoginEvent<number>("burnrate-update-progress", handler);
}

export async function onCheckUpdateRequested(
  handler: () => void | Promise<void>,
) {
  /* v8 ignore next 3: native Tauri event path */
  if (isTauri) {
    return listen("burnrate-check-update-requested", () => handler());
  }
  function listener() {
    void handler();
  }
  window.addEventListener("burnrate-check-update-requested", listener);
  return () =>
    window.removeEventListener("burnrate-check-update-requested", listener);
}

/** Subscribe to a login event, bridging Tauri events and mock CustomEvents. */
function onLoginEvent<T>(
  name: string,
  handler: (payload: T) => void | Promise<void>,
) {
  /* v8 ignore next 3: native Tauri event path */
  if (isTauri) {
    return listen<T>(name, (event) => handler(event.payload));
  }
  function listener(event: Event) {
    void handler((event as CustomEvent<T>).detail);
  }
  window.addEventListener(name, listener);
  return () => window.removeEventListener(name, listener);
}

export async function onLoginProgress(
  handler: (progress: LoginProgress) => void | Promise<void>,
) {
  return onLoginEvent<LoginProgress>("burnrate-login-progress", handler);
}

export async function onLoginComplete(
  handler: (complete: LoginComplete) => void | Promise<void>,
) {
  return onLoginEvent<LoginComplete>("burnrate-login-complete", handler);
}

export async function onLoginFailed(
  handler: (failed: LoginFailed) => void | Promise<void>,
) {
  return onLoginEvent<LoginFailed>("burnrate-login-failed", handler);
}

// Mock login driver: outside Tauri, simulate the backend's event sequence so
// dev:web shows a real-feeling sign-in and tests can drive it with fake timers.
const mockLoginTimers = new Map<string, ReturnType<typeof setTimeout>[]>();

function queueMockLogin(id: string, provider: ProviderKind, label: string) {
  const progress = setTimeout(() => {
    window.dispatchEvent(
      new CustomEvent<LoginProgress>("burnrate-login-progress", {
        detail: {
          id,
          line:
            provider === "claude-code"
              ? "Copy the authentication code from the browser, then paste it here."
              : "Opening your browser to finish signing in…",
          url: "https://example.com/oauth/authorize?code=MOCK",
          needsCode: provider === "claude-code",
        },
      }),
    );
  }, 50);
  const complete = setTimeout(() => {
    const now = new Date().toISOString();
    const account: AccountView = {
      id,
      provider,
      label,
      enabled: true,
      autoDetected: false,
      credentialPath: `mock/cli/${provider}/${id}`,
      endpointOverride: null,
      secretStorage: "keyring",
      hasSecret: false,
      email: `${provider}@example.com`,
      configDir: `mock/cli/${provider}/${id}`,
      createdAt: now,
      updatedAt: now,
    };
    mockAccounts = [...mockAccounts.filter((item) => item.id !== id), account];
    mockLoginTimers.delete(id);
    window.dispatchEvent(
      new CustomEvent<LoginComplete>("burnrate-login-complete", {
        detail: { id, account },
      }),
    );
  }, 120);
  mockLoginTimers.set(id, [progress, complete]);
}

function clearMockLogin(id: string) {
  for (const timer of mockLoginTimers.get(id) ?? []) {
    clearTimeout(timer);
  }
  mockLoginTimers.delete(id);
}

/** Cancel any pending mock logins. Exposed for deterministic tests. */
export function __resetMockLogins(): void {
  for (const id of [...mockLoginTimers.keys()]) {
    clearMockLogin(id);
  }
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
