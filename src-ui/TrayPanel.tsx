import { AlertCircle, RefreshCw } from "lucide-react";
import type { AccountView, DashboardState, SnapshotStatus, UsageSnapshot } from "./types";

const statusLabels: Record<SnapshotStatus, string> = {
  healthy: "Healthy",
  warning: "Warning",
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
}: {
  state: DashboardState | null;
  snapshots: UsageSnapshot[];
  busy: boolean;
  error: string | null;
  onRefresh: () => void;
}) {
  const accounts = state?.accounts ?? [];
  const summary = summarize(snapshots);

  return (
    <main className="tray-panel">
      <header className="tray-panel-head">
        <div>
          <h1>Burnrate</h1>
          <p>{summary}</p>
        </div>
        <button className="icon-button" onClick={onRefresh} disabled={busy} title="Refresh">
          <RefreshCw size={17} className={busy ? "spin" : ""} />
        </button>
      </header>

      {error ? (
        <div className="notice error compact" role="alert">
          <AlertCircle size={17} />
          <span>{error}</span>
        </div>
      ) : null}

      <section className="tray-panel-section" aria-label="Usage">
        {snapshots.length > 0 ? (
          snapshots.map((snapshot) => <TraySnapshot key={snapshot.accountId} snapshot={snapshot} />)
        ) : (
          <p className="tray-empty">No enabled accounts.</p>
        )}
      </section>

      <section className="tray-panel-section" aria-label="Accounts">
        {accounts.map((account) => (
          <TrayAccount key={account.id} account={account} />
        ))}
      </section>
    </main>
  );
}

function TraySnapshot({ snapshot }: { snapshot: UsageSnapshot }) {
  const remaining =
    snapshot.quota?.remaining !== null && snapshot.quota?.remaining !== undefined
      ? formatNumber(snapshot.quota.remaining)
      : "Unknown";

  return (
    <article className={`tray-snapshot ${snapshot.status}`}>
      <div>
        <strong>{snapshot.label}</strong>
        <span>{statusLabels[snapshot.status]}</span>
      </div>
      <p>
        {remaining} {snapshot.quota?.unit ?? "quota"} left
      </p>
    </article>
  );
}

function TrayAccount({ account }: { account: AccountView }) {
  return (
    <div className="tray-account">
      <span>{account.label}</span>
      <small>{account.enabled ? "Enabled" : "Disabled"}</small>
    </div>
  );
}

function summarize(snapshots: UsageSnapshot[]) {
  if (snapshots.some((snapshot) => ["exhausted", "error"].includes(snapshot.status))) {
    return "Burnrate: critical usage";
  }
  if (snapshots.some((snapshot) => snapshot.status === "warning")) {
    return "Burnrate: warning";
  }
  return snapshots.length > 0 ? "Burnrate: all quotas healthy" : "Burnrate: no enabled accounts";
}

function formatNumber(value: number) {
  return new Intl.NumberFormat(undefined, {
    maximumFractionDigits: value > 100 ? 0 : 2,
  }).format(value);
}
