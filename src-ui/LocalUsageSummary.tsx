import { formatUsd } from "./format";
import { Sparkline } from "./Sparkline";
import { providerLabels } from "./constants";
import type { ProviderLocalUsage } from "./types";

/**
 * Compact per-provider local-usage line for the tray popover: a 14-day daily
 * cost sparkline next to today / month-to-date / projected numbers. Rendered
 * once per provider (under its first card) because local session history is
 * provider-level — the caption says so when several accounts share it.
 */
export function LocalUsageSummary({
  usage,
  multiAccount,
}: {
  usage: ProviderLocalUsage;
  multiAccount: boolean;
}) {
  const sessions = `${usage.todaySessions} session${
    usage.todaySessions === 1 ? "" : "s"
  }`;
  const projected =
    usage.projectedMonthCostUsd !== null
      ? ` → ~${formatUsd(usage.projectedMonthCostUsd)} proj.`
      : "";

  return (
    <div className="tray-local-usage">
      <Sparkline
        values={usage.daily.map((day) => day.costUsd)}
        titles={usage.daily.map(
          (day) => `${day.date} · ${formatUsd(day.costUsd)}`,
        )}
        label={`Daily local cost, last ${usage.daily.length} days`}
      />
      <div className="tray-local-stats">
        <span>
          Today {formatUsd(usage.todayCostUsd)} · {sessions}
        </span>
        <span>
          MTD {formatUsd(usage.monthCostUsd)}
          {projected}
        </span>
        <small>
          {multiAccount
            ? `local · all ${providerLabels[usage.provider]} accounts on this Mac`
            : "local estimate"}
        </small>
      </div>
    </div>
  );
}
