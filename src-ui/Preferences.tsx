import {
  AlertCircle,
  CheckCircle2,
  DownloadCloud,
  KeyRound,
  Plus,
  RefreshCw,
  Trash2,
  Wifi,
} from "lucide-react";
import {
  type Dispatch,
  type FormEvent,
  type ReactNode,
  type SetStateAction,
  useState,
} from "react";
import { AccountForm } from "./AccountForm";
import { AddAccountMenu } from "./AddAccountMenu";
import {
  bucketMeterLabel,
  bucketPercent,
  displayBuckets,
  formatLimit,
  formatReset,
} from "./format";
import { ProviderLogo } from "./ProviderLogo";
import { SortableList } from "./SortableList";
import { UpdateBanner } from "./UpdateBanner";
import {
  OPENROUTER_DEFAULT_ENDPOINT,
  RUNPOD_DEFAULT_ENDPOINT,
  emptyForm,
  providerLabels,
} from "./constants";
import type {
  AccountInput,
  AccountView,
  ProviderKind,
  SnapshotStatus,
  UpdateChannel,
  UsageBucketSnapshot,
  UsageSnapshot,
} from "./types";
import type { UpdaterState } from "./useUpdater";

/** Update-channel + auto-updater wiring passed down from `App`. */
export interface UpdatesPanelProps {
  channel: UpdateChannel;
  state: UpdaterState;
  appVersion: string;
  onChannelChange: (channel: UpdateChannel) => void;
  onCheck: () => void;
  onInstall: () => void;
  onDismiss: () => void;
}

// Re-exported for existing importers (App.tsx). Source of truth: ./constants.
export {
  OPENROUTER_DEFAULT_ENDPOINT,
  RUNPOD_DEFAULT_ENDPOINT,
  emptyForm,
  providerLabels,
};

const statusLabels: Record<SnapshotStatus, string> = {
  healthy: "Healthy",
  warning: "Warning",
  exhausted: "Critical",
  error: "Error",
  stale: "Stale",
  "not-configured": "No accounts",
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
  onEditAccount,
  onRemoveAccount,
  onStartLogin,
  onManualAdd,
  onLogout,
  onReorderAccounts,
  updates,
}: {
  accounts: AccountView[];
  snapshots: UsageSnapshot[];
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
  onEditAccount: (account: AccountView) => void;
  onRemoveAccount: (id: string) => void;
  onStartLogin: (provider: ProviderKind, accountId?: string) => void;
  onManualAdd: (provider: ProviderKind) => void;
  onLogout: (id: string) => void;
  onReorderAccounts: (orderedIds: string[]) => void;
  updates: UpdatesPanelProps;
}) {
  const [addMenuOpen, setAddMenuOpen] = useState(false);

  return (
    <main className="prefs-shell">
      <header className="prefs-header">
        <div>
          <h1>Preferences</h1>
          <p>{summary.label}</p>
        </div>
        <div className="toolbar">
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

      <UpdateBanner
        state={updates.state}
        channel={updates.channel}
        onInstall={updates.onInstall}
        onDismiss={updates.onDismiss}
        onRetry={updates.onInstall}
      />

      {error ? (
        <div className="notice error" role="alert">
          <AlertCircle size={18} />
          <span>{error}</span>
        </div>
      ) : null}

      <section className="prefs-layout">
        <aside className="prefs-list" aria-label="Accounts">
          <div className="section-heading">
            <h2>Accounts</h2>
            <div className="section-actions">
              <span>{accounts.length}</span>
              <button
                className="icon-button subtle"
                title="Add account"
                aria-expanded={addMenuOpen}
                onClick={() => setAddMenuOpen((open) => !open)}
              >
                <Plus size={16} />
              </button>
            </div>
          </div>

          {addMenuOpen ? (
            <AddAccountMenu
              onStartLogin={onStartLogin}
              onManual={onManualAdd}
              onClose={() => setAddMenuOpen(false)}
            />
          ) : null}

          <SortableList
            items={accounts}
            onReorder={onReorderAccounts}
            ariaLabel="Account order"
            className="account-list"
            renderItem={(account, handle) => (
              <AccountButton
                account={account}
                handle={handle}
                active={account.id === activeId}
                onEdit={onEditAccount}
                onRemove={onRemoveAccount}
              />
            )}
          />
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

          <AccountForm
            form={form}
            setForm={setForm}
            activeId={activeId}
            busy={busy}
            onSubmit={onSubmit}
            onReset={() => {
              setForm(emptyForm);
              setActiveId(null);
            }}
            onStartLogin={onStartLogin}
            onLogout={onLogout}
          />

          <UpdatesSettings {...updates} />
        </section>
      </section>
    </main>
  );
}

function UpdatesSettings({
  channel,
  state,
  appVersion,
  onChannelChange,
  onCheck,
  onInstall,
}: UpdatesPanelProps) {
  // The banner owns the "update available" affordance; here we surface the
  // steady-state result of a check inline next to the button.
  const status = state.checking
    ? "Checking…"
    : state.error
      ? state.error
      : state.available
        ? `Version ${state.version} ready to install`
        : state.hasChecked
          ? "You're up to date."
          : "";

  return (
    <section className="prefs-updates" aria-label="Updates">
      <SectionTitle title="Updates" detail={`v${appVersion}`} />
      <div className="updates-body">
        <label className="updates-channel">
          <span>Release channel</span>
          <select
            value={channel}
            onChange={(event) =>
              onChannelChange(event.target.value as UpdateChannel)
            }
          >
            <option value="stable">Stable</option>
            <option value="nightly">Nightly (latest, less stable)</option>
          </select>
        </label>
        <p className="muted updates-note">
          Stable tracks signed releases. Nightly follows the rolling{" "}
          <code>nightly</code> build — newer features, fewer guarantees.
        </p>
        <div className="updates-actions">
          <button
            className="secondary"
            onClick={onCheck}
            disabled={state.checking || state.downloading}
          >
            <RefreshCw size={14} className={state.checking ? "spin" : ""} />
            Check for updates
          </button>
          {state.available ? (
            <button
              className="primary"
              onClick={onInstall}
              disabled={state.downloading}
            >
              <DownloadCloud size={14} />
              {state.downloading
                ? `Installing… ${state.progress}%`
                : "Install & Restart"}
            </button>
          ) : null}
        </div>
        {status ? (
          <p className={`updates-status ${state.error ? "error" : ""}`}>
            {status}
          </p>
        ) : null}
      </div>
    </section>
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
  handle,
  active,
  onEdit,
  onRemove,
}: {
  account: AccountView;
  handle: ReactNode;
  active: boolean;
  onEdit: (account: AccountView) => void;
  onRemove: (id: string) => void;
}) {
  return (
    <div className={`account-row ${active ? "active" : ""}`}>
      {handle}
      <button className="account-main" onClick={() => onEdit(account)}>
        <ProviderLogo provider={account.provider} size="sm" />
        <span>
          <strong>{account.label}</strong>
          <small>
            {providerLabels[account.provider]}
            {account.autoDetected ? " · Auto" : ""}
          </small>
          {account.email ? (
            <small className="account-email">{account.email}</small>
          ) : null}
        </span>
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
  const buckets = displayBuckets(snapshot);
  const plan = snapshot.subscription?.planLabel;

  return (
    <article className={`usage-row ${snapshot.status}`}>
      <div className="usage-head">
        <div className="usage-provider">
          <ProviderLogo provider={snapshot.provider} size="sm" />
          <span>
            <strong>{snapshot.label}</strong>
            <small>
              {providerLabels[snapshot.provider]}
              {plan ? ` · ${plan}` : ""}
            </small>
            {snapshot.email ? (
              <small className="account-email">{snapshot.email}</small>
            ) : null}
          </span>
        </div>
        <StatusBadge status={snapshot.status} />
      </div>
      {buckets.length > 0 ? (
        <div className="usage-buckets">
          {buckets.map((bucket) => (
            <BucketLine key={bucket.id} bucket={bucket} />
          ))}
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
      <div className="meter" aria-label={bucketMeterLabel(bucket)}>
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
