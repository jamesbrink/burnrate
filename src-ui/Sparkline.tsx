/**
 * Dependency-free inline-SVG micro-charts for the local insights surfaces.
 * Both scale to their own series maximum (per-card scaling): every chart's
 * shape stays readable regardless of magnitude, at the cost of cross-chart
 * height comparability — magnitudes are conveyed by the adjacent numbers.
 */

export function Sparkline({
  values,
  width = 88,
  height = 22,
  label,
  titles,
}: {
  values: number[];
  width?: number;
  height?: number;
  label: string;
  /** Optional per-point hover tooltips (parallel to `values`). */
  titles?: string[];
}) {
  if (values.length < 2) {
    return null;
  }
  const max = Math.max(...values);
  const pad = 1.5;
  const innerWidth = width - pad * 2;
  const innerHeight = height - pad * 2;
  const step = innerWidth / (values.length - 1);
  const y = (value: number) =>
    pad + innerHeight - (max > 0 ? (value / max) * innerHeight : 0);
  const points = values.map(
    (value, index) =>
      `${(pad + index * step).toFixed(2)},${y(value).toFixed(2)}`,
  );
  const area = [
    `M${pad},${(pad + innerHeight).toFixed(2)}`,
    ...points.map((point) => `L${point}`),
    `L${(pad + innerWidth).toFixed(2)},${(pad + innerHeight).toFixed(2)}`,
    "Z",
  ].join(" ");

  return (
    <svg
      className="sparkline"
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
      role="img"
      aria-label={label}
      focusable="false"
    >
      <path className="sparkline-area" d={area} />
      <polyline className="sparkline-line" points={points.join(" ")} />
      {titles && titles.length === values.length
        ? values.map((_, index) => (
            // Invisible full-height hit areas so hovering any point of the
            // line shows that day's tooltip (a polyline itself has no
            // per-point hover target).
            <rect
              key={index}
              x={Math.max(0, pad + index * step - step / 2)}
              y={0}
              width={step}
              height={height}
              fill="transparent"
            >
              <title>{titles[index]}</title>
            </rect>
          ))
        : null}
    </svg>
  );
}

export function BarSeries({
  items,
  height = 56,
  label,
}: {
  items: { label: string; value: number }[];
  height?: number;
  label: string;
}) {
  if (items.length === 0) {
    return null;
  }
  const max = Math.max(...items.map((item) => item.value));
  const gap = 2;
  const barWidth = 8;
  const width = items.length * (barWidth + gap) - gap;

  return (
    <svg
      className="insight-bars"
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
      role="img"
      aria-label={label}
      focusable="false"
    >
      {items.map((item, index) => {
        // Any non-zero day stays visible (min 2px); zero days render a 1px
        // baseline tick so the timeline doesn't read as missing data.
        const barHeight =
          max > 0 && item.value > 0
            ? Math.max((item.value / max) * height, 2)
            : 1;
        return (
          <rect
            key={`${item.label}-${index}`}
            x={index * (barWidth + gap)}
            y={height - barHeight}
            width={barWidth}
            height={barHeight}
          >
            <title>{`${item.label}: ${item.value.toFixed(2)}`}</title>
          </rect>
        );
      })}
    </svg>
  );
}
