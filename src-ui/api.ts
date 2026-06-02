import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AccountInput, AccountView, AppSettings, DashboardState, UsageSnapshot } from "./types";

const isTauri = "__TAURI_INTERNALS__" in window;

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
];

let mockSettings: AppSettings = {
  hideFromDock: false,
};

const mockSnapshots: UsageSnapshot[] = [
  {
    accountId: "claude-code-local",
    provider: "claude-code",
    label: "Claude Code",
    status: "healthy",
    quota: { used: 528000, limit: 1000000, remaining: 472000, unit: "tokens", resetAt: null },
    burnRate: { perHour: 22000, projectedDepletionAt: null },
    message: null,
    fetchedAt: new Date().toISOString(),
  },
  {
    accountId: "codex-local",
    provider: "codex",
    label: "Codex",
    status: "warning",
    quota: { used: 81, limit: 100, remaining: 19, unit: "requests", resetAt: null },
    burnRate: { perHour: 3.4, projectedDepletionAt: null },
    message: null,
    fetchedAt: new Date().toISOString(),
  },
  {
    accountId: "openrouter-main",
    provider: "openrouter",
    label: "OpenRouter",
    status: "healthy",
    quota: { used: 7.25, limit: 25, remaining: 17.75, unit: "credits", resetAt: null },
    burnRate: { perHour: 0.3, projectedDepletionAt: null },
    message: null,
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

export async function saveSettings(settings: AppSettings): Promise<AppSettings> {
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
  /* v8 ignore next 3: native Tauri invoke path */
  if (isTauri) {
    return invoke<UsageSnapshot[]>("refresh_snapshots");
  }
  return mockSnapshots.map((snapshot) => ({ ...snapshot, fetchedAt: new Date().toISOString() }));
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
  return () => window.removeEventListener("burnrate-refresh-requested", listener);
}
