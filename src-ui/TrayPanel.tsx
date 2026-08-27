import {
  AlertCircle,
  ChevronRight,
  Clock3,
  Download,
  RefreshCw,
  Settings,
  ShieldCheck,
} from "lucide-react";
import {
  useEffect,
  useRef,
  useState,
  type MutableRefObject,
  type PointerEvent,
  type ReactNode,
} from "react";
import {
  currentCursorPosition,
  moveCurrentWindow,
  startWindowDrag,
  windowDragSnapshot,
  type WindowDragSnapshot,
} from "./api";
import {
  bucketMeterLabel,
  bucketPercent,
  displayBuckets,
  formatAgo,
  formatLimit,
  formatReset,
  hasAwsCostData,
} from "./format";
import { LocalUsageSummary } from "./LocalUsageSummary";
import { ProviderLogo } from "./ProviderLogo";
import { SortableList, reorderWithinSubset } from "./SortableList";
import { TrayCardDetails } from "./TrayCardDetails";
import type {
  AccountView,
  DashboardState,
  LocalUsageReport,
  ProviderLocalUsage,
  SnapshotStatus,
  UsageBucketSnapshot,
  UsageSnapshot,
} from "./types";

const providerLabels = {
  "claude-code": "Claude",
  codex: "Codex",
  aws: "AWS",
  openrouter: "OpenRouter",
  runpod: "Runpod",
  copilot: "Copilot",
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
  localUsage = null,
  updateAvailable = false,
  onRefresh,
  onOpenPreferences,
  onReorderAccounts,
}: {
  state: DashboardState | null;
  snapshots: UsageSnapshot[];
  busy: boolean;
  error: string | null;
  localUsage?: LocalUsageReport | null;
  updateAvailable?: boolean;
  onRefresh: () => void;
  onOpenPreferences: () => void;
  onReorderAccounts: (orderedIds: string[]) => void;
}) {
  const accounts = state?.accounts ?? [];
  const summary = summarize(snapshots);
  // Single-expand accordion: one card's drill-down open at a time keeps the
  // popover height bounded and makes a second tap always collapse.
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const cardItems = snapshots.map((snapshot) => ({
    id: snapshot.accountId,
    snapshot,
  }));
  const disabledCount = accounts.filter((account) => !account.enabled).length;
  const isDense = snapshots.length >= 8;
  const updatedAgo = formatAgo(newestFetchedAt(snapshots));
  // Re-render periodically so the relative "Updated …" label stays honest
  // while the popover sits open.
  const [, setFreshnessTick] = useState(0);
  const manualDrag = useRef<ManualWindowDrag | null>(null);
  const manualDragToken = useRef(0);
  useEffect(() => {
    if (snapshots.length === 0) {
      return;
    }
    const timer = setInterval(() => setFreshnessTick((n) => n + 1), 30_000);
    return () => clearInterval(timer);
  }, [snapshots.length]);
  // Local usage is provider-level: render it once, under the provider's first
  // card, with a multi-account caption when several cards share the history.
  const firstCardByProvider = new Map<string, string>();
  const providerCardCounts = new Map<string, number>();
  for (const snapshot of snapshots) {
    if (!firstCardByProvider.has(snapshot.provider)) {
      firstCardByProvider.set(snapshot.provider, snapshot.accountId);
    }
    providerCardCounts.set(
      snapshot.provider,
      (providerCardCounts.get(snapshot.provider) ?? 0) + 1,
    );
  }
  const usageFor = (snapshot: UsageSnapshot): ProviderLocalUsage | null => {
    if (
      !localUsage?.available ||
      firstCardByProvider.get(snapshot.provider) !== snapshot.accountId
    ) {
      return null;
    }
    return (
      localUsage.providers.find(
        (usage) => usage.provider === snapshot.provider,
      ) ?? null
    );
  };

  return (
    <main
      className={`tray-panel${isDense ? " tray-panel-dense" : ""}`}
      data-tauri-drag-region
    >
      <header
        className="tray-header"
        data-tauri-drag-region
        onPointerDown={(event) => {
          startTrayHeaderDrag(event, manualDrag, manualDragToken);
        }}
        onPointerMove={() => {
          void moveTrayHeaderDrag(manualDrag);
        }}
        onPointerUp={(event) => {
          stopTrayHeaderDrag(event, manualDrag, manualDragToken);
        }}
        onPointerCancel={(event) => {
          stopTrayHeaderDrag(event, manualDrag, manualDragToken);
        }}
      >
        <div data-tauri-drag-region>
          <h1>Burnrate</h1>
          <p>{summary}</p>
          {updatedAgo ? (
            <p className="tray-updated">Updated {updatedAgo}</p>
          ) : null}
        </div>
        <div className="tray-header-actions">
          {updateAvailable ? (
            <button
              className="tray-update-pill"
              onClick={onOpenPreferences}
              title="Update available — open Preferences"
            >
              <Download size={12} />
              <span>Update</span>
            </button>
          ) : null}
          <button
            className="icon-button tray-refresh"
            onClick={onRefresh}
            disabled={busy}
            title="Refresh"
          >
            <RefreshCw size={16} className={busy ? "spin" : ""} />
          </button>
          <button
            className="icon-button tray-settings"
            onClick={onOpenPreferences}
            title="Settings"
          >
            <Settings size={16} />
          </button>
        </div>
      </header>

      {error ? (
        <div className="tray-notice" role="alert">
          <AlertCircle size={16} />
          <span>{error}</span>
        </div>
      ) : null}

      <div className="tray-scroll">
        <section className="tray-section" aria-label="Usage">
          {cardItems.length > 0 ? (
            <SortableList
              items={cardItems}
              ariaLabel="Usage order"
              onReorder={(subsetIds) =>
                onReorderAccounts(
                  orderTrayAccountsFromUsageSubset(accounts, subsetIds),
                )
              }
              renderItem={(item, handle) => (
                <TraySnapshot
                  snapshot={item.snapshot}
                  handle={handle}
                  localUsage={usageFor(item.snapshot)}
                  multiAccount={
                    (providerCardCounts.get(item.snapshot.provider) ?? 0) > 1
                  }
                  expanded={expandedId === item.id}
                  onToggle={() =>
                    setExpandedId((current) =>
                      current === item.id ? null : item.id,
                    )
                  }
                />
              )}
            />
          ) : (
            <div className="tray-empty">
              <span>No enabled accounts.</span>
              <button type="button" onClick={onOpenPreferences}>
                Open Preferences
              </button>
            </div>
          )}
        </section>

        {disabledCount > 0 ? (
          <div className="tray-footer">
            <span>
              {disabledCount === 1
                ? "1 account off"
                : `${disabledCount} accounts off`}
            </span>
            <button type="button" onClick={onOpenPreferences}>
              Manage
            </button>
          </div>
        ) : null}
      </div>
    </main>
  );
}

type ManualWindowDrag = {
  pointerId: number;
  start: WindowDragSnapshot;
  frame: number | null;
  moving: boolean;
};

function startTrayHeaderDrag(
  event: PointerEvent<HTMLElement>,
  manualDrag: MutableRefObject<ManualWindowDrag | null>,
  manualDragToken: MutableRefObject<number>,
) {
  if (event.button !== 0 || isInteractiveDragTarget(event.target)) {
    return;
  }

  const token = manualDragToken.current + 1;
  manualDragToken.current = token;
  event.currentTarget.setPointerCapture?.(event.pointerId);
  void startWindowDrag();
  void windowDragSnapshot()
    .then((start) => {
      if (!start || manualDragToken.current !== token) {
        return;
      }
      manualDrag.current = {
        pointerId: event.pointerId,
        start,
        frame: null,
        moving: false,
      };
    })
    .catch(() => {});
}

async function moveTrayHeaderDrag(
  manualDrag: MutableRefObject<ManualWindowDrag | null>,
) {
  const drag = manualDrag.current;
  if (!drag || drag.frame !== null || drag.moving) {
    return;
  }

  drag.frame = window.requestAnimationFrame(() => {
    drag.frame = null;
    drag.moving = true;
    void moveDragFrame(drag)
      .catch(() => {})
      .finally(() => {
        drag.moving = false;
      });
  });
}

async function moveDragFrame(drag: ManualWindowDrag) {
  const cursor = await currentCursorPosition();
  if (!cursor) {
    return;
  }

  await moveCurrentWindow({
    x: drag.start.window.x + cursor.x - drag.start.cursor.x,
    y: drag.start.window.y + cursor.y - drag.start.cursor.y,
  });
}

function stopTrayHeaderDrag(
  event: PointerEvent<HTMLElement>,
  manualDrag: MutableRefObject<ManualWindowDrag | null>,
  manualDragToken: MutableRefObject<number>,
) {
  manualDragToken.current += 1;
  const drag = manualDrag.current;
  if (drag && drag.frame !== null) {
    window.cancelAnimationFrame(drag.frame);
  }
  manualDrag.current = null;
  if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
    event.currentTarget.releasePointerCapture(event.pointerId);
  }
}

function isInteractiveDragTarget(target: EventTarget): boolean {
  return (
    target instanceof Element &&
    Boolean(
      target.closest(
        "button, a, input, textarea, select, [role='button'], [data-no-window-drag]",
      ),
    )
  );
}

export function orderTrayAccountsFromUsageSubset(
  accounts: AccountView[],
  subsetIds: string[],
): string[] {
  return reorderWithinSubset(
    accounts.map((account) => account.id),
    subsetIds,
  );
}

function newestFetchedAt(snapshots: UsageSnapshot[]): string | null {
  let newest: string | null = null;
  for (const snapshot of snapshots) {
    if (!newest || snapshot.fetchedAt > newest) {
      newest = snapshot.fetchedAt;
    }
  }
  return newest;
}

function TraySnapshot({
  snapshot,
  handle,
  localUsage = null,
  multiAccount = false,
  expanded,
  onToggle,
}: {
  snapshot: UsageSnapshot;
  handle: ReactNode;
  localUsage?: ProviderLocalUsage | null;
  multiAccount?: boolean;
  expanded: boolean;
  onToggle: () => void;
}) {
  const buckets = displayBuckets(snapshot);
  const plan = snapshot.subscription?.planLabel;

  return (
    <article className={`tray-card ${snapshot.status}`}>
      <div className="tray-card-head">
        {handle}
        <button
          type="button"
          className="tray-card-toggle"
          aria-expanded={expanded}
          onClick={onToggle}
          title={expanded ? "Hide details" : "Show details"}
        >
          <div className="tray-provider">
            <ProviderLogo provider={snapshot.provider} size="sm" />
            <div>
              <strong>{snapshot.label}</strong>
              <span>{providerLabels[snapshot.provider]}</span>
              {snapshot.email ? (
                <span className="tray-email">{snapshot.email}</span>
              ) : null}
              {hasAwsCostData(snapshot) ? (
                <span>AWS cost data · {formatAgo(snapshot.fetchedAt)}</span>
              ) : null}
            </div>
          </div>
          <span className={`tray-status ${snapshot.status}`}>
            {statusLabels[snapshot.status]}
          </span>
          <ChevronRight
            size={13}
            className="tray-card-chevron"
            aria-hidden="true"
          />
        </button>
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

      {localUsage ? (
        <LocalUsageSummary usage={localUsage} multiAccount={multiAccount} />
      ) : null}

      {expanded ? (
        <TrayCardDetails snapshot={snapshot} localUsage={localUsage} />
      ) : null}

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
