import type { UsageBucketSnapshot, UsageSnapshot } from "./types";

export function primaryBucket(
  snapshot: UsageSnapshot,
): UsageBucketSnapshot | null {
  return snapshot.usageBuckets[0] ?? bucketFromQuota(snapshot);
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
  if (!bucket.limit || bucket.remaining === null) return 0;
  return Math.max(0, Math.min(100, (bucket.remaining / bucket.limit) * 100));
}

export function formatNumber(value: number): string {
  return new Intl.NumberFormat(undefined, {
    maximumFractionDigits: value > 100 ? 0 : 2,
  }).format(value);
}

export function formatLimit(bucket: UsageBucketSnapshot): string {
  const remaining =
    bucket.remaining === null ? "Unknown" : formatNumber(bucket.remaining);
  const limit = bucket.limit === null ? "" : ` / ${formatNumber(bucket.limit)}`;
  return `${remaining}${limit}`;
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
