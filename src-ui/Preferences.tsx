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
import type { Dispatch, FormEvent, SetStateAction } from "react";
import {
  bucketPercent,
  formatLimit,
  formatReset,
  primaryBucket,
} from "./format";
import type {
  AccountInput,
  AccountView,
  AppSettings,
  ProviderKind,
  SecretStorageMode,
  SnapshotStatus,
  UsageBucketSnapshot,
  UsageSnapshot,
} from "./types";

export const providerLabels: Record<ProviderKind, string> = {
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

export const emptyForm: AccountInput = {
  provider: "openrouter",
  label: "OpenRouter",
  enabled: true,
  endpointOverride: "",
  secretStorage: "keyring",
  secret: "",
};

type Summary = {
  label: string;
  shortLabel: string;
  status: SnapshotStatus;
  criticalCount: number;
  warningCount: number;
  updatedAt: string;
};

export function Preferences({
  accounts,
  snapshots,
  settings,
  summary,
  busy,
  error,
  form,
  activeId,
  setForm,
  setActiveId,
  onSubmit,
  onDetect,
  onRefresh,
  onSettingsChange,
  onEditAccount,
  onRemoveAccount,
}: {
  accounts: AccountView[];
  snapshots: UsageSnapshot[];
  settings: AppSettings;
  summary: Summary;
  busy: boolean;
  error: string | null;
  form: AccountInput;
  activeId: string | null;
  setForm: Dispatch<SetStateAction<AccountInput>>;
  setActiveId: Dispatch<SetStateAction<string | null>>;
  onSubmit: (event: FormEvent) => void;
  onDetect: () => void;
  onRefresh: () => void;
  onSettingsChange: (settings: AppSettings) => void;
  onEditAccount: (account: AccountView) => void;
  onRemoveAccount: (id: string) => void;
}) {
  return (
    <main className="prefs-shell">
      <header className="prefs-header">
        <div>
          <h1>Preferences</h1>
          <p>{summary.label}</p>
        </div>
        <div className="toolbar">
          <label
            className="dock-toggle"
            title="Hide Burnrate from the macOS Dock"
          >
            <input
              type="checkbox"
              checked={settings.hideFromDock}
              disabled={busy}
              onChange={(event) =>
                onSettingsChange({ hideFromDock: event.target.checked })
              }
            />
            Hide Dock Icon
          </label>
          <button
            className="icon-button"
            onClick={onDetect}
            disabled={busy}
            title="Detect accounts"
          >
            <Wifi size={17} />
          </button>
          <button
            className="icon-button"
            onClick={onRefresh}
            disabled={busy}
            title="Refresh"
          >
            <RefreshCw size={17} className={busy ? "spin" : ""} />
          </button>
        </div>
      </header>

      {error ? (
        <div className="notice error" role="alert">
          <AlertCircle size={18} />
          <span>{error}</span>
        </div>
      ) : null}

      <section className="prefs-layout">
        <aside className="prefs-list" aria-label="Accounts">
          <SectionTitle title="Accounts" detail={`${accounts.length}`} />
          {accounts.map((account) => (
            <AccountButton
              key={account.id}
              account={account}
              active={account.id === activeId}
              onEdit={onEditAccount}
              onRemove={onRemoveAccount}
            />
          ))}
          {accounts.length === 0 ? (
            <p className="muted">No accounts configured.</p>
          ) : null}
        </aside>

        <section className="prefs-main" aria-label="Usage and account settings">
          <section className="prefs-usage">
            <SectionTitle
              title="Usage"
              detail={snapshots.length > 0 ? summary.shortLabel : "Idle"}
            />
            <div className="usage-list">
              {snapshots.map((snapshot) => (
                <UsageRow key={snapshot.accountId} snapshot={snapshot} />
              ))}
              {!busy && snapshots.length === 0 ? (
                <div className="empty-state">
                  Add or detect an account to start monitoring quota.
                </div>
              ) : null}
            </div>
          </section>

          <form className="account-form prefs-form" onSubmit={onSubmit}>
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
                  <RotateCcw size={16} />
                </button>
              ) : null}
            </div>

            <div className="form-grid">
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
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      label: event.target.value,
                    }))
                  }
                  required
                />
              </label>
            </div>

            <div className="segmented" role="group" aria-label="Secret storage">
              {(["keyring", "plaintext"] satisfies SecretStorageMode[]).map(
                (mode) => (
                  <button
                    key={mode}
                    type="button"
                    className={form.secretStorage === mode ? "active" : ""}
                    onClick={() =>
                      setForm((current) => ({
                        ...current,
                        secretStorage: mode,
                      }))
                    }
                  >
                    {mode === "keyring" ? "Keyring" : "Plaintext"}
                  </button>
                ),
              )}
            </div>

            <div className="form-grid">
              <label>
                API Key
                <input
                  type="password"
                  value={form.secret ?? ""}
                  placeholder={activeId ? "Leave blank to keep existing" : ""}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      secret: event.target.value,
                    }))
                  }
                />
              </label>

              <label>
                Endpoint
                <input
                  value={form.endpointOverride ?? ""}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      endpointOverride: event.target.value,
                    }))
                  }
                />
              </label>
            </div>

            <label className="toggle">
              <input
                type="checkbox"
                checked={form.enabled}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    enabled: event.target.checked,
                  }))
                }
              />
              Enabled
            </label>

            <button className="primary" type="submit" disabled={busy}>
              <Plus size={17} />
              {activeId ? "Save" : "Add"}
            </button>
          </form>
        </section>
      </section>
    </main>
  );
}

function SectionTitle({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="section-heading">
      <h2>{title}</h2>
      <span>{detail}</span>
    </div>
  );
}

function AccountButton({
  account,
  active,
  onEdit,
  onRemove,
}: {
  account: AccountView;
  active: boolean;
  onEdit: (account: AccountView) => void;
  onRemove: (id: string) => void;
}) {
  return (
    <div className={`account-row ${active ? "active" : ""}`}>
      <button className="account-main" onClick={() => onEdit(account)}>
        <strong>{account.label}</strong>
        <small>
          {providerLabels[account.provider]}
          {account.autoDetected ? " · Auto" : ""}
        </small>
      </button>
      <span className="account-flags">
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
          <Trash2 size={15} />
        </button>
      </span>
    </div>
  );
}

function UsageRow({ snapshot }: { snapshot: UsageSnapshot }) {
  const buckets = snapshot.usageBuckets.length > 0 ? snapshot.usageBuckets : [];
  const primary = primaryBucket(snapshot);
  const plan = snapshot.subscription?.planLabel ?? "Unknown plan";

  return (
    <article className={`usage-row ${snapshot.status}`}>
      <div className="usage-head">
        <div>
          <strong>{snapshot.label}</strong>
          <small>
            {providerLabels[snapshot.provider]} · {plan}
          </small>
        </div>
        <StatusBadge status={snapshot.status} />
      </div>
      {buckets.length > 0 ? (
        <div className="usage-buckets">
          {buckets.map((bucket) => (
            <BucketLine key={bucket.id} bucket={bucket} />
          ))}
        </div>
      ) : primary ? (
        <div className="usage-buckets">
          <BucketLine bucket={primary} />
        </div>
      ) : (
        <p className="snapshot-message">
          {snapshot.message ?? "Usage unavailable."}
        </p>
      )}
      {snapshot.message ? (
        <p className="snapshot-message">{snapshot.message}</p>
      ) : null}
    </article>
  );
}

function BucketLine({ bucket }: { bucket: UsageBucketSnapshot }) {
  return (
    <div className={`bucket-line ${bucket.status}`}>
      <div>
        <span>{bucket.label}</span>
        <strong>
          {formatLimit(bucket)} {bucket.unit}
        </strong>
      </div>
      <div className="meter" aria-label={`${bucket.label} remaining`}>
        <span style={{ width: `${bucketPercent(bucket)}%` }} />
      </div>
      <small>{formatReset(bucket.resetAt)}</small>
    </div>
  );
}

function StatusBadge({ status }: { status: SnapshotStatus }) {
  const Icon = status === "healthy" ? CheckCircle2 : AlertCircle;
  return (
    <span className={`status ${status}`}>
      <Icon size={14} />
      {statusLabels[status]}
    </span>
  );
}

function StatusDot({ enabled }: { enabled: boolean }) {
  return (
    <span
      className={`dot ${enabled ? "enabled" : ""}`}
      aria-label={enabled ? "Enabled" : "Disabled"}
    />
  );
}
