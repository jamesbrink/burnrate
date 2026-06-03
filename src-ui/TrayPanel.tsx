import { AlertCircle, Clock3, RefreshCw, ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import {
  bucketMeterLabel,
  bucketPercent,
  displayBuckets,
  formatLimit,
  formatReset,
} from "./format";
import { ProviderLogo } from "./ProviderLogo";
import { SortableList, reorderWithinSubset } from "./SortableList";
import type {
  AccountView,
  DashboardState,
  SnapshotStatus,
  UsageBucketSnapshot,
  UsageSnapshot,
} from "./types";

const providerLabels = {
  "claude-code": "Claude",
  codex: "Codex",
  openrouter: "OpenRouter",
  runpod: "Runpod",
} as const;

const statusLabels: Record<SnapshotStatus, string> = {
  healthy: "OK",
  warning: "Low",
  exhausted: "Critical",
  error: "Error",
  stale: "Stale",
  "not-configured": "No accounts",
};

export function TrayPanel({
  state,
  snapshots,
  busy,
  error,
  onRefresh,
  onReorderAccounts,
}: {
  state: DashboardState | null;
  snapshots: UsageSnapshot[];
  busy: boolean;
  error: string | null;
  onRefresh: () => void;
  onReorderAccounts: (orderedIds: string[]) => void;
}) {
  const accounts = state?.accounts ?? [];
  const summary = summarize(snapshots);
  const cardItems = snapshots.map((snapshot) => ({
    id: snapshot.accountId,
    snapshot,
  }));

  return (
    <main className="tray-panel">
      <header className="tray-header">
        <div>
          <h1>Burnrate</h1>
          <p>{summary}</p>
        </div>
        <button
          className="icon-button tray-refresh"
          onClick={onRefresh}
          disabled={busy}
          title="Refresh"
        >
          <RefreshCw size={16} className={busy ? "spin" : ""} />
        </button>
      </header>

      {error ? (
        <div className="tray-notice" role="alert">
          <AlertCircle size={16} />
          <span>{error}</span>
        </div>
      ) : null}

      <section className="tray-section" aria-label="Usage">
        {cardItems.length > 0 ? (
          <SortableList
            items={cardItems}
            ariaLabel="Usage order"
            onReorder={(subsetIds) =>
              onReorderAccounts(
                reorderWithinSubset(
                  accounts.map((account) => account.id),
                  subsetIds,
                ),
              )
            }
            renderItem={(item, handle) => (
              <TraySnapshot snapshot={item.snapshot} handle={handle} />
            )}
          />
        ) : (
          <div className="tray-empty">No enabled accounts.</div>
        )}
      </section>

      {accounts.length > 0 ? (
        <section className="tray-section tray-accounts" aria-label="Accounts">
          {accounts.map((account) => (
            <TrayAccount key={account.id} account={account} />
          ))}
        </section>
      ) : null}
    </main>
  );
}

function TraySnapshot({
  snapshot,
  handle,
}: {
  snapshot: UsageSnapshot;
  handle: ReactNode;
}) {
  const buckets = displayBuckets(snapshot);
  const plan = snapshot.subscription?.planLabel;

  return (
    <article className={`tray-card ${snapshot.status}`}>
      <div className="tray-card-head">
        <div className="tray-provider">
          {handle}
          <ProviderLogo provider={snapshot.provider} size="sm" />
          <div>
            <strong>{snapshot.label}</strong>
            <span>{providerLabels[snapshot.provider]}</span>
            {snapshot.email ? (
              <span className="tray-email">{snapshot.email}</span>
            ) : null}
          </div>
        </div>
        <span className={`tray-status ${snapshot.status}`}>
          {statusLabels[snapshot.status]}
        </span>
      </div>

      {plan ? (
        <div className="tray-plan">
          <ShieldCheck size={14} />
          <span>{plan}</span>
          {snapshot.subscription?.extraUsageEnabled ? (
            <small>extra usage</small>
          ) : null}
        </div>
      ) : null}

      <div className="bucket-list">
        {buckets.map((bucket) => (
          <BucketRow key={bucket.id} bucket={bucket} />
        ))}
      </div>

      {snapshot.message ? (
        <p className="tray-message">{snapshot.message}</p>
      ) : null}
    </article>
  );
}

function BucketRow({ bucket }: { bucket: UsageBucketSnapshot }) {
  return (
    <div className={`bucket-row ${bucket.status}`}>
      <div className="bucket-meta">
        <span>{bucket.label}</span>
        <strong>
          {formatLimit(bucket)} {bucket.unit}
        </strong>
      </div>
      <div className="mini-meter" aria-label={bucketMeterLabel(bucket)}>
        <span style={{ width: `${bucketPercent(bucket)}%` }} />
      </div>
      {bucket.resetAt ? (
        <small>
          <Clock3 size={12} />
          {formatReset(bucket.resetAt)}
        </small>
      ) : null}
    </div>
  );
}

function TrayAccount({ account }: { account: AccountView }) {
  return (
    <div className="tray-account">
      <span className="tray-account-provider">
        <ProviderLogo provider={account.provider} size="sm" />
        <span>
          <span>{account.label}</span>
          {account.email ? (
            <small className="tray-email">{account.email}</small>
          ) : null}
        </span>
      </span>
      <small>{account.enabled ? "Enabled" : "Disabled"}</small>
    </div>
  );
}

function summarize(snapshots: UsageSnapshot[]) {
  if (
    snapshots.some((snapshot) =>
      ["exhausted", "error"].includes(snapshot.status),
    )
  ) {
    return "Critical usage";
  }
  if (snapshots.some((snapshot) => snapshot.status === "warning")) {
    return "Approaching a limit";
  }
  if (snapshots.some((snapshot) => snapshot.status === "stale")) {
    return "Usage data is stale";
  }
  return snapshots.length > 0 ? "All quotas healthy" : "No enabled accounts";
}
