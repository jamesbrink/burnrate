import type { UsageBucketSnapshot, UsageSnapshot } from "./types";

export function primaryBucket(
  snapshot: UsageSnapshot,
): UsageBucketSnapshot | null {
  return snapshot.usageBuckets[0] ?? bucketFromQuota(snapshot);
}

export function displayBuckets(snapshot: UsageSnapshot): UsageBucketSnapshot[] {
  const buckets = snapshot.usageBuckets.filter(hasBucketValue);
  if (buckets.length > 0) return buckets;
  const fallback = bucketFromQuota(snapshot);
  return fallback && hasBucketValue(fallback) ? [fallback] : [];
}

export function hasBucketValue(bucket: UsageBucketSnapshot): boolean {
  return (
    bucket.limit !== null || bucket.remaining !== null || bucket.used !== 0
  );
}

export function bucketFromQuota(
  snapshot: UsageSnapshot,
): UsageBucketSnapshot | null {
  if (!snapshot.quota) return null;
  return {
    id: "quota",
    label: "Quota",
    window: null,
    used: snapshot.quota.used,
    limit: snapshot.quota.limit,
    remaining: snapshot.quota.remaining,
    unit: snapshot.quota.unit,
    resetAt: snapshot.quota.resetAt,
    status: snapshot.status,
  };
}

export function bucketPercent(bucket: UsageBucketSnapshot): number {
  if (!bucket.limit) return 0;
  const value = bucket.remaining === null ? bucket.used : bucket.remaining;
  return Math.max(0, Math.min(100, (value / bucket.limit) * 100));
}

export function bucketMeterLabel(bucket: UsageBucketSnapshot): string {
  return bucket.remaining === null
    ? `${bucket.label} usage`
    : `${bucket.label} remaining`;
}

export function formatNumber(value: number): string {
  return new Intl.NumberFormat(undefined, {
    maximumFractionDigits: value > 100 ? 0 : 2,
  }).format(value);
}

export function formatLimit(bucket: UsageBucketSnapshot): string {
  const value =
    bucket.remaining === null
      ? bucket.limit === null && bucket.used === 0
        ? "Unknown"
        : formatBucketNumber(bucket.used, bucket.unit)
      : formatBucketNumber(bucket.remaining, bucket.unit);
  const limit =
    bucket.limit === null
      ? ""
      : ` / ${formatBucketNumber(bucket.limit, bucket.unit)}`;
  return `${value}${limit}`;
}

function formatBucketNumber(value: number, unit: string): string {
  const formatted = formatNumber(value);
  return unit.startsWith("USD") ? `$${formatted}` : formatted;
}

export function formatReset(value: string | null): string {
  if (!value) return "";
  const reset = new Date(value);
  if (Number.isNaN(reset.getTime())) return "";
  const minutes = Math.max(
    0,
    Math.round((reset.getTime() - Date.now()) / 60000),
  );
  if (minutes < 60) return `resets in ${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `resets in ${hours}h`;
  return `resets ${reset.toLocaleDateString(undefined, { month: "short", day: "numeric" })}`;
}

/** Compact USD for insights surfaces: cents matter under $10, whole dollars
 *  past $100 (`$1.84`, `$42.10`, `$96`). */
export function formatUsd(value: number): string {
  const digits = value < 10 ? 2 : value < 100 ? 2 : 0;
  return `$${new Intl.NumberFormat(undefined, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value)}`;
}
