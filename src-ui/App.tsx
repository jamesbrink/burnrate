import {
  AlertCircle,
  CheckCircle2,
  KeyRound,
  Plus,
  RefreshCw,
  RotateCcw,
  Trash2,
  Wifi,
} from "lucide-react";
import { FormEvent, useEffect, useMemo, useState } from "react";
import {
  detectAccounts,
  loadDashboard,
  refreshSnapshots,
  removeAccount,
  saveAccount,
} from "./api";
import type {
  AccountInput,
  AccountView,
  DashboardState,
  ProviderKind,
  SecretStorageMode,
  SnapshotStatus,
  UsageSnapshot,
} from "./types";

const providerLabels: Record<ProviderKind, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  openrouter: "OpenRouter",
};

const statusLabels: Record<SnapshotStatus, string> = {
  healthy: "Healthy",
  warning: "Warning",
  exhausted: "Critical",
  error: "Error",
  stale: "Stale",
  "not-configured": "No accounts",
};

const emptyForm: AccountInput = {
  provider: "openrouter",
  label: "OpenRouter",
  enabled: true,
  endpointOverride: "",
  secretStorage: "keyring",
  secret: "",
};

export function App() {
  const [state, setState] = useState<DashboardState | null>(null);
  const [snapshots, setSnapshots] = useState<UsageSnapshot[]>([]);
  const [form, setForm] = useState<AccountInput>(emptyForm);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function reload() {
    setBusy(true);
    setError(null);
    try {
      const dashboard = await loadDashboard();
      setState(dashboard);
      setSnapshots(dashboard.snapshots);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function refreshOnly() {
    setBusy(true);
    setError(null);
    try {
      setSnapshots(await refreshSnapshots());
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void reload();
  }, []);

  useEffect(() => {
    function onRefresh() {
      void refreshOnly();
    }
    window.addEventListener("burnrate-refresh-requested", onRefresh);
    return () => window.removeEventListener("burnrate-refresh-requested", onRefresh);
  }, []);

  const accounts = state?.accounts ?? [];
  const summary = useMemo(() => summarize(snapshots), [snapshots]);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const accounts = await saveAccount({
        ...form,
        endpointOverride: form.endpointOverride?.trim() || null,
        secret: form.secret?.trim() || null,
      });
      setState((previous) =>
        previous
          ? { ...previous, accounts }
          : { accounts, snapshots: [], traySummary: summary },
      );
      setForm(emptyForm);
      setActiveId(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  function editAccount(account: AccountView) {
    setActiveId(account.id);
    setForm({
      id: account.id,
      provider: account.provider,
      label: account.label,
      enabled: account.enabled,
      endpointOverride: account.endpointOverride ?? "",
      secretStorage: account.secretStorage,
      secret: "",
    });
  }

  async function onRemove(id: string) {
    setBusy(true);
    setError(null);
    try {
      const accounts = await removeAccount(id);
      setState((previous) =>
        previous
          ? { ...previous, accounts }
          : { accounts, snapshots: [], traySummary: summary },
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onDetect() {
    setBusy(true);
    setError(null);
    try {
      const accounts = await detectAccounts();
      setState((previous) =>
        previous
          ? { ...previous, accounts }
          : { accounts, snapshots: [], traySummary: summary },
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="shell">
      <section className="topbar" aria-label="Burnrate summary">
        <div>
          <h1>Burnrate</h1>
          <p>{summary.label}</p>
        </div>
        <div className="toolbar">
          <button className="icon-button" onClick={onDetect} title="Detect accounts">
            <Wifi size={18} />
          </button>
          <button className="icon-button" onClick={refreshOnly} disabled={busy} title="Refresh">
            <RefreshCw size={18} className={busy ? "spin" : ""} />
          </button>
        </div>
      </section>

      {error ? (
        <div className="notice error" role="alert">
          <AlertCircle size={18} />
          <span>{error}</span>
        </div>
      ) : null}

      <section className="metrics" aria-label="Quota snapshots">
        {snapshots.map((snapshot) => (
          <SnapshotCard key={snapshot.accountId} snapshot={snapshot} />
        ))}
        {!busy && snapshots.length === 0 ? (
          <div className="empty-state">
            <KeyRound size={22} />
            <span>Add or detect an account to start monitoring quota.</span>
          </div>
        ) : null}
      </section>

      <section className="workspace">
        <AccountTable accounts={accounts} onEdit={editAccount} onRemove={onRemove} />
        <form className="account-form" onSubmit={onSubmit}>
          <div className="form-heading">
            <h2>{activeId ? "Edit Account" : "Add Account"}</h2>
            {activeId ? (
              <button
                type="button"
                className="icon-button subtle"
                title="Reset form"
                onClick={() => {
                  setForm(emptyForm);
                  setActiveId(null);
                }}
              >
                <RotateCcw size={17} />
              </button>
            ) : null}
          </div>

          <label>
            Provider
            <select
              value={form.provider}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  provider: event.target.value as ProviderKind,
                  label: providerLabels[event.target.value as ProviderKind],
                }))
              }
            >
              <option value="openrouter">OpenRouter</option>
              <option value="claude-code">Claude Code</option>
              <option value="codex">Codex</option>
            </select>
          </label>

          <label>
            Label
            <input
              value={form.label}
              onChange={(event) => setForm((current) => ({ ...current, label: event.target.value }))}
              required
            />
          </label>

          <div className="segmented" role="group" aria-label="Secret storage">
            {(["keyring", "plaintext"] satisfies SecretStorageMode[]).map((mode) => (
              <button
                key={mode}
                type="button"
                className={form.secretStorage === mode ? "active" : ""}
                onClick={() => setForm((current) => ({ ...current, secretStorage: mode }))}
              >
                {mode === "keyring" ? "Keyring" : "Plaintext"}
              </button>
            ))}
          </div>

          <label>
            API Key
            <input
              type="password"
              value={form.secret ?? ""}
              placeholder={activeId ? "Leave blank to keep existing" : ""}
              onChange={(event) => setForm((current) => ({ ...current, secret: event.target.value }))}
            />
          </label>

          <label>
            Endpoint
            <input
              value={form.endpointOverride ?? ""}
              onChange={(event) =>
                setForm((current) => ({ ...current, endpointOverride: event.target.value }))
              }
            />
          </label>

          <label className="toggle">
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(event) => setForm((current) => ({ ...current, enabled: event.target.checked }))}
            />
            Enabled
          </label>

          <button className="primary" type="submit" disabled={busy}>
            <Plus size={17} />
            {activeId ? "Save" : "Add"}
          </button>
        </form>
      </section>
    </main>
  );
}

function SnapshotCard({ snapshot }: { snapshot: UsageSnapshot }) {
  const percent =
    snapshot.quota?.limit && snapshot.quota.remaining !== null
      ? Math.max(0, Math.min(100, (snapshot.quota.remaining / snapshot.quota.limit) * 100))
      : null;

  return (
    <article className={`snapshot ${snapshot.status}`}>
      <div className="snapshot-head">
        <span>{snapshot.label}</span>
        <StatusBadge status={snapshot.status} />
      </div>
      <div className="quota-number">
        {snapshot.quota?.remaining !== null && snapshot.quota?.remaining !== undefined
          ? formatNumber(snapshot.quota.remaining)
          : "Unknown"}
      </div>
      <div className="quota-meta">
        <span>{snapshot.quota?.unit ?? "quota"} remaining</span>
        <span>{snapshot.burnRate ? `${formatNumber(snapshot.burnRate.perHour)}/hr` : ""}</span>
      </div>
      <div className="meter" aria-label={`${snapshot.label} remaining`}>
        <span style={{ width: `${percent ?? 0}%` }} />
      </div>
      {snapshot.message ? <p className="snapshot-message">{snapshot.message}</p> : null}
    </article>
  );
}

function AccountTable({
  accounts,
  onEdit,
  onRemove,
}: {
  accounts: AccountView[];
  onEdit: (account: AccountView) => void;
  onRemove: (id: string) => void;
}) {
  return (
    <section className="accounts" aria-label="Accounts">
      <div className="section-heading">
        <h2>Accounts</h2>
        <span>{accounts.length}</span>
      </div>
      <div className="table">
        {accounts.map((account) => (
          <div key={account.id} className="account-row">
            <button className="account-main" onClick={() => onEdit(account)}>
              <strong>{account.label}</strong>
              <small>{providerLabels[account.provider]}</small>
            </button>
            <span className="account-flags">
              {account.autoDetected ? <span>Auto</span> : null}
              {account.hasSecret ? <KeyRound size={15} /> : null}
              <StatusDot enabled={account.enabled} />
              <button
                className="icon-button danger"
                title="Remove account"
                onClick={(event) => {
                  event.stopPropagation();
                  onRemove(account.id);
                }}
              >
                <Trash2 size={16} />
              </button>
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}

function StatusBadge({ status }: { status: SnapshotStatus }) {
  const Icon = status === "healthy" ? CheckCircle2 : AlertCircle;
  return (
    <span className={`status ${status}`}>
      <Icon size={15} />
      {statusLabels[status]}
    </span>
  );
}

function StatusDot({ enabled }: { enabled: boolean }) {
  return <span className={`dot ${enabled ? "enabled" : ""}`} aria-label={enabled ? "Enabled" : "Disabled"} />;
}

function summarize(snapshots: UsageSnapshot[]) {
  const criticalCount = snapshots.filter((snapshot) =>
    ["exhausted", "error"].includes(snapshot.status),
  ).length;
  const warningCount = snapshots.filter((snapshot) => snapshot.status === "warning").length;
  const label =
    criticalCount > 0
      ? `Burnrate: ${criticalCount} critical`
      : warningCount > 0
        ? `Burnrate: ${warningCount} warning`
        : snapshots.length > 0
          ? "Burnrate: all quotas healthy"
          : "Burnrate: no enabled accounts";

  return {
    label,
    status: criticalCount > 0 ? "exhausted" : warningCount > 0 ? "warning" : "healthy",
    criticalCount,
    warningCount,
    updatedAt: new Date().toISOString(),
  } as const;
}

function formatNumber(value: number) {
  return new Intl.NumberFormat(undefined, {
    maximumFractionDigits: value > 100 ? 0 : 2,
  }).format(value);
}
