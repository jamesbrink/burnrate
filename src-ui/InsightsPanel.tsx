import { formatUsd } from "./format";
import { ProviderLogo } from "./ProviderLogo";
import { BarSeries } from "./Sparkline";
import { providerLabels } from "./constants";
import type { LocalUsageReport, ProviderLocalUsage } from "./types";

export interface InsightsPanelProps {
  report: LocalUsageReport | null;
  enabled: boolean;
  onToggle: (enabled: boolean) => void;
}

/**
 * Preferences "Local insights" section: claudex-backed per-provider usage with
 * a daily cost timeline, model distribution, and top projects — plus the
 * setting toggle that controls collection.
 */
export function InsightsPanel({ report, enabled, onToggle }: InsightsPanelProps) {
  return (
    <section className="prefs-insights" aria-label="Local insights">
      <div className="section-heading">
        <h2>Local insights</h2>
        <label className="toggle compact insights-toggle">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(event) => onToggle(event.target.checked)}
          />
          Enabled
        </label>
      </div>

      {!enabled ? (
        <p className="muted">
          Local insights are off. Burnrate stops reading CLI session logs and
          leaves the claudex index untouched.
        </p>
      ) : report === null ? (
        <p className="muted">
          Collecting local usage… the first run indexes your CLI session
          history and can take a few minutes.
        </p>
      ) : !report.available || report.providers.length === 0 ? (
        <p className="muted">
          {report.message ?? "No local session history found yet."}
        </p>
      ) : (
        <>
          <div className="insights-grid">
            {report.providers.map((usage) => (
              <InsightsCard key={usage.provider} usage={usage} />
            ))}
          </div>
          <p className="muted insights-footnote">
            Computed locally from CLI session logs via claudex. Costs are
            API-price estimates; multiple accounts of one provider share this
            history.
          </p>
        </>
      )}
    </section>
  );
}

function InsightsCard({ usage }: { usage: ProviderLocalUsage }) {
  return (
    <article className="insights-card">
      <div className="insights-card-head">
        <ProviderLogo provider={usage.provider} size="sm" />
        <strong>{providerLabels[usage.provider]}</strong>
        {usage.topModel ? <small>{usage.topModel}</small> : null}
      </div>

      <dl className="insights-stats">
        <Stat label="Today" value={formatUsd(usage.todayCostUsd)} />
        <Stat label="Week" value={formatUsd(usage.weekCostUsd)} />
        <Stat label="Month" value={formatUsd(usage.monthCostUsd)} />
        <Stat
          label="Projected"
          value={
            usage.projectedMonthCostUsd !== null
              ? `~${formatUsd(usage.projectedMonthCostUsd)}`
              : "—"
          }
        />
      </dl>

      <BarSeries
        items={usage.daily.map((day) => ({
          label: day.date,
          value: day.costUsd,
        }))}
        label={`Daily local cost for ${providerLabels[usage.provider]}, last ${usage.daily.length} days`}
      />

      {usage.modelDistribution.length > 0 ? (
        <ul className="insights-list" aria-label="Models this month">
          {usage.modelDistribution.map((model) => (
            <li key={model.model}>
              <span>{model.model}</span>
              <small>
                {model.sessions} sess · {formatUsd(model.costUsd)}
              </small>
            </li>
          ))}
        </ul>
      ) : null}

      {usage.topProjects.length > 0 ? (
        <ul className="insights-list" aria-label="Top projects this month">
          {usage.topProjects.map((project) => (
            <li key={project.project}>
              <span>{project.project}</span>
              <small>
                {project.sessions} sess · {formatUsd(project.costUsd)}
              </small>
            </li>
          ))}
        </ul>
      ) : null}
    </article>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="insights-stat">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}
