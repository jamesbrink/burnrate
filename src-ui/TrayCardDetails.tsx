import { BarSeries } from "./Sparkline";
import {
  displayBuckets,
  formatAgo,
  formatLimit,
  formatTokenCount,
  formatUsd,
} from "./format";
import type { ProviderLocalUsage, UsageSnapshot } from "./types";

/**
 * Expanded drill-down for a tray usage card: snapshot facts the compact card
 * leaves out (freshness, rate-limit tier, value-less buckets) plus, on the
 * provider's first card, the local-insights detail (week cost, month tokens,
 * daily bars, model and project breakdowns). Local data is provider-level,
 * never per-account — the summary's caption above already says so.
 */
export function TrayCardDetails({
  snapshot,
  localUsage,
}: {
  snapshot: UsageSnapshot;
  localUsage: ProviderLocalUsage | null;
}) {
  const shown = new Set(displayBuckets(snapshot).map((bucket) => bucket.id));
  const hiddenBuckets = snapshot.usageBuckets.filter(
    (bucket) => !shown.has(bucket.id),
  );
  const tier = snapshot.subscription?.rateLimitTier ?? null;

  return (
    <div className="tray-card-details">
      <div className="tray-detail-list">
        <DetailRow label="Updated" value={formatAgo(snapshot.fetchedAt)} />
        {tier ? (
          <DetailRow label="Tier" value={tier.replace(/_/g, " ")} />
        ) : null}
        {hiddenBuckets.map((bucket) => (
          <DetailRow
            key={bucket.id}
            label={bucket.label}
            value={`${formatLimit(bucket)} ${bucket.unit}`}
          />
        ))}
      </div>

      {localUsage ? (
        <div className="tray-detail-local">
          <span className="tray-detail-heading">This month · local</span>
          <div className="tray-detail-list">
            <DetailRow
              label="Past week"
              value={formatUsd(localUsage.weekCostUsd)}
            />
            <DetailRow
              label="Tokens"
              value={`${formatTokenCount(localUsage.monthInputTokens)} in · ${formatTokenCount(localUsage.monthOutputTokens)} out`}
            />
          </div>
          {localUsage.daily.length > 0 ? (
            <BarSeries
              items={localUsage.daily.map((day) => ({
                label: `${day.date} · ${formatUsd(day.costUsd)}`,
                value: day.costUsd,
              }))}
              height={40}
              label={`Daily local cost, last ${localUsage.daily.length} days`}
            />
          ) : null}
          {localUsage.modelDistribution.length > 0 ? (
            <BreakdownList
              heading="Models"
              items={localUsage.modelDistribution.map((entry) => ({
                key: entry.model,
                name: entry.model,
                detail: `${formatUsd(entry.costUsd)} · ${entry.sessions} sess`,
              }))}
            />
          ) : null}
          {localUsage.topProjects.length > 0 ? (
            <BreakdownList
              heading="Projects"
              items={localUsage.topProjects.map((entry) => ({
                key: entry.project,
                // Project identifiers are paths; the tray only has room for
                // the leaf directory.
                name: entry.project.split("/").filter(Boolean).pop() ??
                  entry.project,
                detail: `${formatUsd(entry.costUsd)} · ${entry.sessions} sess`,
              }))}
            />
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  if (!value) {
    return null;
  }
  return (
    <div className="tray-detail-row">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function BreakdownList({
  heading,
  items,
}: {
  heading: string;
  items: { key: string; name: string; detail: string }[];
}) {
  return (
    <div className="tray-detail-breakdown">
      <span className="tray-detail-heading">{heading}</span>
      <ul>
        {items.slice(0, 5).map((item) => (
          <li key={item.key}>
            <span>{item.name}</span>
            <small>{item.detail}</small>
          </li>
        ))}
      </ul>
    </div>
  );
}
