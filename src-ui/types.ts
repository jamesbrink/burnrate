export type ProviderKind =
  | "claude-code"
  | "codex"
  | "openrouter"
  | "runpod"
  | "aws";
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
  email: string | null;
  configDir: string | null;
  awsProfile?: string | null;
  awsRegion?: string | null;
  awsMonthlyBudgetUsd?: number | null;
  awsCategories?: AwsCategoryConfig[];
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
  awsProfile?: string | null;
  awsRegion?: string | null;
  awsMonthlyBudgetUsd?: number | null;
  awsCategories?: AwsCategoryConfig[];
}

export type AwsCostFilter =
  | { kind: "dimension"; key: string; values: string[] }
  | { kind: "tag"; key: string; values: string[] }
  | { kind: "costCategory"; key: string; values: string[] };

export type AwsGroupByKind = "dimension" | "tag" | "cost-category";

export interface AwsGroupBy {
  kind: AwsGroupByKind;
  key: string;
}

export interface AwsCategoryConfig {
  id: string;
  label: string;
  enabled: boolean;
  filter: AwsCostFilter;
  groupBy?: AwsGroupBy | null;
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
  email?: string | null;
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

export type UpdateChannel = "stable" | "nightly";

export interface AppSettings {
  hideFromDock: boolean;
  updateChannel: UpdateChannel;
  trayScale: number;
}

/** Available update returned by the `check_for_updates` command. */
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  body: string | null;
  date: string | null;
}

export interface DashboardState {
  accounts: AccountView[];
  snapshots: UsageSnapshot[];
  traySummary: TraySummary;
  settings: AppSettings;
}

/** Streamed sign-in progress (`burnrate-login-progress`). */
export interface LoginProgress {
  id: string;
  line: string;
  url?: string | null;
  needsCode?: boolean;
}

/** Successful sign-in (`burnrate-login-complete`). */
export interface LoginComplete {
  id: string;
  account: AccountView;
}

/** Failed or cancelled sign-in (`burnrate-login-failed`). */
export interface LoginFailed {
  id: string;
  error: string;
}
