export type ProviderKind = "claude-code" | "codex" | "openrouter";
export type SecretStorageMode = "keyring" | "plaintext";
export type SnapshotStatus =
  | "healthy"
  | "warning"
  | "exhausted"
  | "error"
  | "stale"
  | "not-configured";
export type SubscriptionPlan =
  | "free"
  | "pro"
  | "max"
  | "team"
  | "enterprise"
  | "unknown";

export interface AccountView {
  id: string;
  provider: ProviderKind;
  label: string;
  enabled: boolean;
  autoDetected: boolean;
  credentialPath: string | null;
  endpointOverride: string | null;
  secretStorage: SecretStorageMode;
  hasSecret: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface AccountInput {
  id?: string;
  provider: ProviderKind;
  label: string;
  enabled: boolean;
  endpointOverride?: string | null;
  secretStorage: SecretStorageMode;
  secret?: string | null;
}

export interface QuotaSnapshot {
  used: number;
  limit: number | null;
  remaining: number | null;
  unit: string;
  resetAt: string | null;
}

export interface SubscriptionSnapshot {
  plan: SubscriptionPlan;
  planLabel: string;
  rateLimitTier: string | null;
  extraUsageEnabled: boolean | null;
  source: string;
}

export interface UsageBucketSnapshot {
  id: string;
  label: string;
  window: string | null;
  used: number;
  limit: number | null;
  remaining: number | null;
  unit: string;
  resetAt: string | null;
  status: SnapshotStatus;
}

export interface UsageSnapshot {
  accountId: string;
  provider: ProviderKind;
  label: string;
  status: SnapshotStatus;
  subscription?: SubscriptionSnapshot | null;
  usageBuckets: UsageBucketSnapshot[];
  quota: QuotaSnapshot | null;
  message: string | null;
  fetchedAt: string;
}

export interface TraySummary {
  label: string;
  status: SnapshotStatus;
  criticalCount: number;
  warningCount: number;
  updatedAt: string;
}

export interface AppSettings {
  hideFromDock: boolean;
}

export interface DashboardState {
  accounts: AccountView[];
  snapshots: UsageSnapshot[];
  traySummary: TraySummary;
  settings: AppSettings;
}
