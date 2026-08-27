import {
  AlertCircle,
  CheckCircle2,
  DownloadCloud,
  Plus,
  RefreshCw,
  Trash2,
  Wifi,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import { AccountModal, type AccountModalMode } from "./AccountModal";
import { InsightsPanel, type InsightsPanelProps } from "./InsightsPanel";
import {
  bucketMeterLabel,
  bucketPercent,
  displayBuckets,
  formatAgo,
  formatLimit,
  formatReset,
  hasAwsCostData,
} from "./format";
import { ProviderLogo } from "./ProviderLogo";
import { SortableList } from "./SortableList";
import { UpdateBanner } from "./UpdateBanner";
import { PROVIDERS, providerLabels } from "./constants";
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

export interface TraySettingsPanelProps {
  trayScale: number;
  onTrayScaleChange: (scale: number) => void;
}

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
  onSaveAccount,
  onToggleAccount,
  onDetect,
  onRefresh,
  onRemoveAccount,
  onStartLogin,
  onLogout,
  onReorderAccounts,
  settings,
  insights,
  updates,
}: {
  accounts: AccountView[];
  snapshots: UsageSnapshot[];
  summary: Summary;
  busy: boolean;
  error: string | null;
  /** Resolves on success; rejections render inline in the account modal. */
  onSaveAccount: (input: AccountInput) => Promise<void>;
  onToggleAccount: (account: AccountView) => void;
  onDetect: () => void;
  onRefresh: () => void;
  onRemoveAccount: (id: string) => void;
  onStartLogin: (provider: ProviderKind, accountId?: string) => void;
  onLogout: (id: string) => void;
  onReorderAccounts: (orderedIds: string[]) => void;
  settings: TraySettingsPanelProps;
  insights: InsightsPanelProps;
  updates: UpdatesPanelProps;
}) {
  const [modal, setModal] = useState<
    null | { kind: "add" } | { kind: "edit"; accountId: string }
  >(null);
  // Resolve the edit target from live account state so the modal reflects
  // backend updates (and vanishes if the account is removed elsewhere).
  const editingAccount =
    modal?.kind === "edit"
      ? accounts.find((account) => account.id === modal.accountId)
      : undefined;
  const modalMode: AccountModalMode | null =
    modal?.kind === "add"
      ? { kind: "add" }
      : editingAccount
        ? { kind: "edit", account: editingAccount }
        : null;
  // Mirrors the backend re-auth guard: browser sign-in is only safe for the
  // auto-detected system-default account or one with an isolated config dir.
  const canReauth =
    !editingAccount ||
    editingAccount.autoDetected ||
    Boolean(editingAccount.configDir);

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
                onClick={() => setModal({ kind: "add" })}
              >
                <Plus size={16} />
              </button>
            </div>
          </div>

          <SortableList
            items={accounts}
            onReorder={onReorderAccounts}
            ariaLabel="Account order"
            className="account-list"
            renderItem={(account, handle) => (
              <AccountButton
                account={account}
                handle={handle}
                active={
                  modal?.kind === "edit" && modal.accountId === account.id
                }
                onEdit={(target) =>
                  setModal({ kind: "edit", accountId: target.id })
                }
                onToggle={onToggleAccount}
                onRemove={onRemoveAccount}
              />
            )}
          />
          {accounts.length === 0 ? (
            <p className="muted">No accounts configured.</p>
          ) : null}
        </aside>

        <section className="prefs-main" aria-label="Usage and account settings">
          {accounts.length === 0 && snapshots.length === 0 && !busy ? (
            <FirstRunPanel
              onAdd={() => setModal({ kind: "add" })}
              onDetect={onDetect}
            />
          ) : (
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
          )}

          <InsightsPanel {...insights} />

          <TraySettings {...settings} />

          <UpdatesSettings {...updates} />
        </section>
      </section>

      {modalMode ? (
        <AccountModal
          key={
            modalMode.kind === "edit" ? `edit-${modalMode.account.id}` : "add"
          }
          mode={modalMode}
          busy={busy}
          canReauth={canReauth}
          onSave={onSaveAccount}
          onClose={() => setModal(null)}
          onStartLogin={onStartLogin}
          onLogout={onLogout}
          onRemove={onRemoveAccount}
        />
      ) : null}
    </main>
  );
}

/** Guided empty state for a fresh install: no accounts configured yet. */
function FirstRunPanel({
  onAdd,
  onDetect,
}: {
  onAdd: () => void;
  onDetect: () => void;
}) {
  return (
    <section className="prefs-usage first-run" aria-label="Get started">
      <SectionTitle title="Get started" detail="No accounts yet" />
      <div className="first-run-body">
        <div className="first-run-logos" aria-hidden="true">
          {PROVIDERS.map((provider) => (
            <ProviderLogo key={provider} provider={provider} size="sm" />
          ))}
        </div>
        <p className="muted">
          Burnrate watches quotas, credits, and spend across your AI providers
          from the menu bar. CLIs already signed in on this Mac (Claude Code,
          Codex, Copilot) can be detected automatically.
        </p>
        <div className="first-run-actions">
          <button className="primary" onClick={onAdd}>
            <Plus size={15} /> Add your first account
          </button>
          <button className="secondary" onClick={onDetect}>
            <Wifi size={15} /> Detect accounts
          </button>
        </div>
      </div>
    </section>
  );
}

function TraySettings({
  trayScale,
  onTrayScaleChange,
}: TraySettingsPanelProps) {
  const percent = Math.round(trayScale * 100);
  return (
    <section className="prefs-tray-settings" aria-label="Tray popover">
      <SectionTitle title="Tray popover" detail="Sizing" />
      <label className="settings-slider">
        <span>
          <strong>Tray content scale</strong>
          <small>
            {percent === 100
              ? "Native size — scaling disabled"
              : `${percent}% — scales down before showing an internal scrollbar`}
          </small>
        </span>
        <input
          type="range"
          min="0.5"
          max="1"
          step="0.05"
          value={trayScale}
          onChange={(event) => onTrayScaleChange(Number(event.target.value))}
        />
      </label>
    </section>
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
  // Check feedback (checking / up to date / failed) lives in the
  // UpdateDialog that `onCheck` opens; the banner owns the passive
  // "update available" affordance.
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
  onToggle,
  onRemove,
}: {
  account: AccountView;
  handle: ReactNode;
  active: boolean;
  onEdit: (account: AccountView) => void;
  onToggle: (account: AccountView) => void;
  onRemove: (id: string) => void;
}) {
  const [confirmRemove, setConfirmRemove] = useState(false);

  return (
    <div className={`account-row ${active ? "active" : ""}`}>
      {handle}
      <button
        className="account-main"
        title={
          account.email ? `${account.label} — ${account.email}` : account.label
        }
        onClick={() => onEdit(account)}
      >
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
      {confirmRemove ? (
        <span className="account-flags remove-confirm">
          <button
            className="danger-solid"
            title={`Confirm removing ${account.label}`}
            onClick={() => onRemove(account.id)}
          >
            Remove
          </button>
          <button
            className="secondary"
            title="Keep account"
            onClick={() => setConfirmRemove(false)}
          >
            Keep
          </button>
        </span>
      ) : (
        <span className="account-flags">
          <button
            type="button"
            role="switch"
            aria-checked={account.enabled}
            aria-label={account.enabled ? "Enabled" : "Disabled"}
            className={`row-switch${account.enabled ? " on" : ""}`}
            onClick={() => onToggle(account)}
          >
            <span />
          </button>
          <button
            className="icon-button danger"
            title="Remove account"
            onClick={() => setConfirmRemove(true)}
          >
            <Trash2 size={15} />
          </button>
        </span>
      )}
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
            {hasAwsCostData(snapshot) ? (
              <small className="snapshot-freshness">
                AWS cost data · {formatAgo(snapshot.fetchedAt)}
              </small>
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
