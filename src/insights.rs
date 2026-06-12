//! claudex-backed local usage insights.
//!
//! Burnrate's quota providers report what a vendor says remains; this module
//! reports what the local CLIs actually consumed, computed by the `claudex`
//! library from on-disk session logs (Claude Code, Codex, Copilot). claudex
//! maintains a sqlite index at `~/.claudex/index.db` (`CLAUDEX_DIR` override)
//! shared with the claudex CLI — WAL plus a 30s busy timeout make concurrent
//! use safe.
//!
//! Everything here is local: no HTTP, no credentials. The first collection on
//! a machine with a large session history rebuilds the whole index and can
//! take minutes, so callers must keep it off the dashboard's critical path.
//! Local history also cannot be attributed to one of several accounts of the
//! same provider, which is why metrics are per provider, not per account.

use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use claudex::api::{Claudex, ClaudexConfig, Filter, Provider};
use claudex::index::{ModelUsageRow, TimelineRow};

use crate::models::{
    LocalDailyUsage, LocalModelUsage, LocalProjectCost, LocalUsageReport, ProviderKind,
    ProviderLocalUsage,
};

/// Days of daily cost history returned for sparklines.
const TIMELINE_DAYS: usize = 14;
/// Cap for the model-distribution and top-project lists.
const TOP_LIMIT: usize = 5;

/// The claudex providers backing a burnrate provider, or `None` for providers
/// with no local session source (their usage lives server-side).
pub(crate) fn claudex_kinds(provider: ProviderKind) -> Option<Vec<Provider>> {
    match provider {
        ProviderKind::ClaudeCode => Some(vec![Provider::Claude]),
        ProviderKind::Codex => Some(vec![Provider::Codex]),
        // The Copilot CLI carries token/premium-request metrics; VS Code
        // Copilot Chat sessions are indexed too and contribute session counts.
        ProviderKind::Copilot => Some(vec![Provider::Copilot, Provider::CopilotVscode]),
        ProviderKind::OpenRouter | ProviderKind::Runpod | ProviderKind::Aws => None,
    }
}

/// Build the local-usage report for `providers` (deduplicated, order
/// preserved). Runs claudex on a blocking thread: the rusqlite-backed client
/// is `Send` but not `Sync`, and the first sync may scan every session log on
/// disk. Never errors — failures degrade to `available: false` so a broken
/// index can't take the dashboard down with it.
pub(crate) async fn collect_local_usage(providers: Vec<ProviderKind>) -> LocalUsageReport {
    match tokio::task::spawn_blocking(move || collect_blocking(None, Vec::new(), &providers)).await
    {
        Ok(report) => report,
        Err(error) => unavailable(format!("local insights task failed: {error}")),
    }
}

fn unavailable(message: String) -> LocalUsageReport {
    LocalUsageReport {
        available: false,
        message: Some(message),
        providers: Vec::new(),
        generated_at: Utc::now(),
    }
}

/// `state_dir` and `scope` are test seams: a hermetic index directory and an
/// explicit claudex provider list instead of auto-discovering the machine's
/// real session logs. Production passes `None` / empty.
fn collect_blocking(
    state_dir: Option<PathBuf>,
    scope: Vec<Provider>,
    providers: &[ProviderKind],
) -> LocalUsageReport {
    match try_collect(state_dir, scope, providers) {
        Ok(report) => report,
        Err(error) => unavailable(format!("local insights unavailable: {error:#}")),
    }
}

fn try_collect(
    state_dir: Option<PathBuf>,
    scope: Vec<Provider>,
    providers: &[ProviderKind],
) -> Result<LocalUsageReport> {
    let mut client = Claudex::with_config(ClaudexConfig {
        state_dir,
        providers: scope,
    })?;
    let now = Local::now();

    let mut rows = Vec::new();
    let mut seen: Vec<ProviderKind> = Vec::new();
    for &provider in providers {
        if seen.contains(&provider) {
            continue;
        }
        seen.push(provider);
        let Some(kinds) = claudex_kinds(provider) else {
            continue;
        };
        if let Some(usage) = provider_usage(&mut client, provider, &kinds, now)? {
            rows.push(usage);
        }
    }

    let message = rows
        .is_empty()
        .then(|| "No local session history found yet.".to_string());
    Ok(LocalUsageReport {
        available: true,
        message,
        providers: rows,
        generated_at: Utc::now(),
    })
}

fn provider_usage(
    client: &mut Claudex,
    provider: ProviderKind,
    kinds: &[Provider],
    now: DateTime<Local>,
) -> Result<Option<ProviderLocalUsage>> {
    let since = |value: String| Filter {
        providers: kinds.to_vec(),
        since: Some(value),
        ..Filter::default()
    };

    let month_filter = since(start_of_month(now));
    let month = client.cost_summary(None, month_filter.clone())?;
    let week = client.cost_summary(None, since("7d".to_string()))?;
    // A provider with no activity this month or week gets no card at all;
    // ancient history alone is not worth tray space.
    if month.session_count == 0 && month.cost_usd <= 0.0 && week.session_count == 0 {
        return Ok(None);
    }

    let today = client.cost_summary(None, since(start_of_day(now)))?;
    let timeline = client.timeline(since(format!("{TIMELINE_DAYS}d")), false, TIMELINE_DAYS)?;
    let (top_model, model_distribution) =
        model_breakdown(client.models(None, month_filter.clone())?);
    let top_projects = client
        .costs_by_project(None, month_filter, TOP_LIMIT)?
        .into_iter()
        .map(|row| LocalProjectCost {
            project: row.project,
            sessions: row.session_count,
            cost_usd: row.cost_usd,
        })
        .collect();

    Ok(Some(ProviderLocalUsage {
        provider,
        today_cost_usd: today.cost_usd,
        today_sessions: today.session_count,
        week_cost_usd: week.cost_usd,
        month_cost_usd: month.cost_usd,
        projected_month_cost_usd: projected_month_cost(month.cost_usd, now.date_naive()),
        month_input_tokens: month.input_tokens,
        month_output_tokens: month.output_tokens,
        top_model,
        model_distribution,
        top_projects,
        daily: daily_series(timeline),
    }))
}

/// Map claudex timeline buckets (ISO dates) to the wire type, ascending by
/// date — the query's ordering is not part of its contract.
fn daily_series(rows: Vec<TimelineRow>) -> Vec<LocalDailyUsage> {
    let mut daily: Vec<LocalDailyUsage> = rows
        .into_iter()
        .map(|row| LocalDailyUsage {
            date: row.bucket,
            cost_usd: row.cost_usd,
            sessions: row.session_count,
        })
        .collect();
    daily.sort_by(|a, b| a.date.cmp(&b.date));
    daily
}

/// Top model by cost plus the cost-descending distribution, capped at
/// [`TOP_LIMIT`]. Rows with no sessions and no cost are noise and dropped.
fn model_breakdown(rows: Vec<ModelUsageRow>) -> (Option<String>, Vec<LocalModelUsage>) {
    let mut distribution: Vec<LocalModelUsage> = rows
        .into_iter()
        .filter(|row| row.session_count > 0 || row.cost_usd > 0.0)
        .map(|row| LocalModelUsage {
            model: row.model,
            sessions: row.session_count,
            cost_usd: row.cost_usd,
        })
        .collect();
    distribution.sort_by(|a, b| {
        b.cost_usd
            .total_cmp(&a.cost_usd)
            .then(b.sessions.cmp(&a.sessions))
    });
    distribution.truncate(TOP_LIMIT);
    let top_model = distribution.first().map(|row| row.model.clone());
    (top_model, distribution)
}

/// Linear month-end extrapolation: month-to-date cost scaled by
/// days-in-month over day-of-month. Predictable and honest about pace;
/// early-month values swing with each new day by construction. `None` when
/// there is no spend to extrapolate.
pub(crate) fn projected_month_cost(month_cost: f64, today: NaiveDate) -> Option<f64> {
    if month_cost <= 0.0 {
        return None;
    }
    let day = f64::from(today.day());
    let days = f64::from(days_in_month(today.year(), today.month()));
    Some(month_cost * days / day)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next_month_start = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next_month_start
        .and_then(|date| date.pred_opt())
        .map(|date| date.day())
        .expect("every month has a last day")
}

/// RFC3339 timestamp of local midnight today. claudex `since` values are
/// inclusive instants, so "today" means the user's local calendar day rather
/// than the UTC one.
fn start_of_day(now: DateTime<Local>) -> String {
    local_midnight(now.date_naive()).to_rfc3339()
}

/// RFC3339 timestamp of local midnight on the 1st of the current month.
fn start_of_month(now: DateTime<Local>) -> String {
    let first = now.date_naive().with_day(1).expect("day 1 exists");
    local_midnight(first).to_rfc3339()
}

/// Local midnight resolved to UTC. A DST transition can make midnight
/// ambiguous or nonexistent; take the earliest valid instant, falling back to
/// reading the date as UTC.
fn local_midnight(date: NaiveDate) -> DateTime<Utc> {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight exists");
    naive
        .and_local_timezone(Local)
        .earliest()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| naive.and_utc())
}

/// Month-to-date premium requests from local Copilot CLI session logs. Only
/// CLI sessions that shut down cleanly record the count, and only this
/// machine's sessions are visible — the result is a lower bound, and every UI
/// surface labels it an estimate.
pub(crate) async fn copilot_premium_requests_mtd() -> Result<u64> {
    match tokio::task::spawn_blocking(|| premium_requests_blocking(None)).await {
        Ok(result) => result,
        Err(error) => Err(anyhow::anyhow!("premium request scan task failed: {error}")),
    }
}

fn premium_requests_blocking(state_dir: Option<PathBuf>) -> Result<u64> {
    let mut client = Claudex::with_config(ClaudexConfig {
        state_dir,
        // Scope the sync to Copilot: this path runs per Copilot snapshot
        // fetch and must not pay for re-indexing every provider.
        providers: vec![Provider::Copilot],
    })?;
    let filter = Filter {
        providers: vec![Provider::Copilot],
        since: Some(start_of_month(Local::now())),
        ..Filter::default()
    };
    let sessions = client.sessions(None, None, filter, 100_000)?;
    Ok(sum_premium_requests(&sessions))
}

/// Sum the `premium_requests` counters claudex preserves in each Copilot
/// session's `extras` JSON. Sessions without the counter (crashed before
/// shutdown, VS Code Chat) contribute nothing.
pub(crate) fn sum_premium_requests(sessions: &[claudex::index::IndexedSession]) -> u64 {
    sessions
        .iter()
        .filter_map(|session| session.extras.as_deref())
        .filter_map(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .filter_map(|extras| extras.get("premium_requests").and_then(|v| v.as_u64()))
        .sum()
}

/// Shared helpers for tests (here and in `providers::copilot`) that point
/// claudex at hermetic fixture directories via its env overrides.
#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;

    use chrono::{DateTime, Utc};

    /// `CLAUDEX_DIR` / `CLAUDEX_COPILOT_DIR` are process-global; tests that
    /// set them must hold this lock so cargo's parallel test threads can't
    /// observe each other's fixture roots. Poison is irrelevant — the guard
    /// only orders env access.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run `body` with the claudex index (`CLAUDEX_DIR`) and Copilot data
    /// root (`CLAUDEX_COPILOT_DIR`) redirected to hermetic dirs, so no test
    /// can ever index fixture data into the machine's real `~/.claudex`.
    pub(crate) fn with_claudex_env<R>(
        state_dir: Option<&Path>,
        copilot_dir: Option<&Path>,
        body: impl FnOnce() -> R,
    ) -> R {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let apply = |key: &str, value: Option<&Path>| match value {
            Some(dir) => unsafe { std::env::set_var(key, dir) },
            None => unsafe { std::env::remove_var(key) },
        };
        apply("CLAUDEX_DIR", state_dir);
        apply("CLAUDEX_COPILOT_DIR", copilot_dir);
        // Clear on unwind too: a panicking (failing) test must not leak its
        // fixture roots into later tests. Declared after the lock guard so it
        // drops first — env resets while the lock is still held.
        struct ResetOnDrop;
        impl Drop for ResetOnDrop {
            fn drop(&mut self) {
                unsafe {
                    std::env::remove_var("CLAUDEX_DIR");
                    std::env::remove_var("CLAUDEX_COPILOT_DIR");
                }
            }
        }
        let _reset = ResetOnDrop;
        body()
    }

    /// Write a minimal-but-valid Copilot CLI session into
    /// `<base>/session-state/<id>/events.jsonl`, stamped `at`.
    pub(crate) fn write_copilot_fixture(
        base: &Path,
        session_id: &str,
        at: DateTime<Utc>,
        premium_requests: u64,
    ) {
        let dir = base.join("session-state").join(session_id);
        fs::create_dir_all(&dir).unwrap();
        let ts = at.to_rfc3339();
        let later = (at + chrono::Duration::minutes(5)).to_rfc3339();
        let events = format!(
            concat!(
                "{{\"type\":\"session.start\",\"timestamp\":\"{ts}\",\"data\":{{\"sessionId\":\"{id}\",",
                "\"selectedModel\":\"gpt-5\",\"context\":{{\"cwd\":\"/work/demo\"}}}}}}\n",
                "{{\"type\":\"user.message\",\"timestamp\":\"{ts}\",\"data\":{{\"content\":\"hi\"}}}}\n",
                "{{\"type\":\"session.shutdown\",\"timestamp\":\"{later}\",\"data\":{{",
                "\"totalPremiumRequests\":{premium},",
                "\"modelMetrics\":{{\"gpt-5\":{{\"usage\":{{\"inputTokens\":120,\"outputTokens\":40,",
                "\"cacheReadTokens\":0,\"cacheWriteTokens\":0}},\"requests\":{{\"count\":2}}}}}}}}}}\n",
            ),
            ts = ts,
            later = later,
            id = session_id,
            premium = premium_requests,
        );
        fs::write(dir.join("events.jsonl"), events).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::test_support::{with_claudex_env, write_copilot_fixture};
    use super::*;

    fn with_copilot_dir<R>(dir: &Path, body: impl FnOnce() -> R) -> R {
        with_claudex_env(None, Some(dir), body)
    }

    #[test]
    fn maps_burnrate_providers_to_claudex_kinds() {
        assert_eq!(
            claudex_kinds(ProviderKind::ClaudeCode),
            Some(vec![Provider::Claude])
        );
        assert_eq!(
            claudex_kinds(ProviderKind::Codex),
            Some(vec![Provider::Codex])
        );
        assert_eq!(
            claudex_kinds(ProviderKind::Copilot),
            Some(vec![Provider::Copilot, Provider::CopilotVscode])
        );
        assert_eq!(claudex_kinds(ProviderKind::OpenRouter), None);
        assert_eq!(claudex_kinds(ProviderKind::Runpod), None);
        assert_eq!(claudex_kinds(ProviderKind::Aws), None);
    }

    #[test]
    fn projects_month_cost_linearly() {
        let mid_june = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        assert_eq!(projected_month_cost(50.0, mid_june), Some(100.0));

        // Day one projects a full month at today's spend.
        let first = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        assert_eq!(projected_month_cost(3.0, first), Some(90.0));

        // Month end converges on the actual spend.
        let last = NaiveDate::from_ymd_opt(2026, 6, 30).unwrap();
        assert_eq!(projected_month_cost(90.0, last), Some(90.0));

        // February (leap year) uses 29 days.
        let leap = NaiveDate::from_ymd_opt(2028, 2, 29).unwrap();
        assert_eq!(projected_month_cost(29.0, leap), Some(29.0));

        assert_eq!(
            projected_month_cost(0.0, mid_june),
            None,
            "no spend, no projection"
        );
    }

    #[test]
    fn daily_series_sorts_ascending_by_date() {
        let rows = vec![
            timeline_row("2026-06-10", 2, 4.0),
            timeline_row("2026-06-08", 1, 1.0),
            timeline_row("2026-06-09", 3, 2.5),
        ];

        let daily = daily_series(rows);

        let dates: Vec<&str> = daily.iter().map(|d| d.date.as_str()).collect();
        assert_eq!(dates, vec!["2026-06-08", "2026-06-09", "2026-06-10"]);
        assert_eq!(daily[2].cost_usd, 4.0);
        assert_eq!(daily[2].sessions, 2);
    }

    #[test]
    fn model_breakdown_ranks_by_cost_and_caps_the_list() {
        let rows = vec![
            model_row("claude-haiku-4-5", 9, 1.0),
            model_row("claude-fable-5", 3, 42.0),
            model_row("idle-model", 0, 0.0),
            model_row("m3", 1, 5.0),
            model_row("m4", 1, 4.0),
            model_row("m5", 1, 3.0),
            model_row("m6", 1, 2.0),
        ];

        let (top, distribution) = model_breakdown(rows);

        assert_eq!(top.as_deref(), Some("claude-fable-5"));
        assert_eq!(distribution.len(), TOP_LIMIT, "capped at TOP_LIMIT");
        assert_eq!(distribution[0].model, "claude-fable-5");
        assert!(
            !distribution.iter().any(|m| m.model == "idle-model"),
            "zero-activity rows are dropped"
        );
    }

    #[test]
    fn month_and_day_starts_are_valid_local_instants() {
        let now = Local::now();
        let day = DateTime::parse_from_rfc3339(&start_of_day(now)).unwrap();
        let month = DateTime::parse_from_rfc3339(&start_of_month(now)).unwrap();

        assert!(day <= now.fixed_offset());
        assert!(month <= day);
        assert_eq!(month.with_timezone(&Local).day(), 1);
    }

    #[test]
    fn collects_copilot_usage_from_a_hermetic_fixture() {
        let state = tempdir().unwrap();
        let copilot_home = tempdir().unwrap();
        write_copilot_fixture(copilot_home.path(), "11111111-aaaa", Utc::now(), 7);

        // Root the claudex Copilot provider at the fixture instead of
        // ~/.copilot; the claudex client reads the var at construction.
        let report = with_copilot_dir(copilot_home.path(), || {
            collect_blocking(
                Some(state.path().to_path_buf()),
                vec![Provider::Copilot],
                &[ProviderKind::Copilot, ProviderKind::OpenRouter],
            )
        });

        assert!(report.available, "report: {:?}", report.message);
        assert_eq!(report.providers.len(), 1, "openrouter has no local source");
        let usage = &report.providers[0];
        assert_eq!(usage.provider, ProviderKind::Copilot);
        assert_eq!(usage.today_sessions, 1);
        assert!(usage.month_input_tokens > 0);
        assert_eq!(usage.top_model.as_deref(), Some("gpt-5"));
        assert!(!usage.daily.is_empty());
    }

    #[test]
    fn sums_premium_requests_from_session_extras() {
        let session = |extras: Option<&str>| claudex::index::IndexedSession {
            rowid: 0,
            provider: "copilot".to_string(),
            project_name: "demo".to_string(),
            session_id: None,
            file_path: String::new(),
            first_timestamp_ms: None,
            last_timestamp_ms: None,
            message_count: 0,
            duration_ms: 0,
            model: None,
            extras: extras.map(str::to_string),
            present_on_disk: true,
            archived_at: None,
        };

        let sessions = vec![
            session(Some(r#"{"premium_requests":12,"branch":"main"}"#)),
            session(Some(r#"{"premium_requests":30}"#)),
            session(Some(r#"{"branch":"no-counter"}"#)),
            session(Some("{not json")),
            session(None),
        ];

        assert_eq!(sum_premium_requests(&sessions), 42);
        assert_eq!(sum_premium_requests(&[]), 0);
    }

    #[test]
    fn counts_month_to_date_premium_requests_from_fixture_sessions() {
        let state = tempdir().unwrap();
        let copilot_home = tempdir().unwrap();
        let now = Utc::now();
        write_copilot_fixture(copilot_home.path(), "22222222-bbbb", now, 5);
        write_copilot_fixture(copilot_home.path(), "33333333-cccc", now, 9);
        // A session from a previous month must not count toward MTD. 40 days
        // is always outside the current month.
        write_copilot_fixture(
            copilot_home.path(),
            "44444444-dddd",
            now - chrono::Duration::days(40),
            100,
        );

        let counted = with_copilot_dir(copilot_home.path(), || {
            premium_requests_blocking(Some(state.path().to_path_buf()))
        })
        .unwrap();

        assert_eq!(counted, 14);
    }

    #[test]
    fn broken_state_dir_degrades_to_unavailable_instead_of_erroring() {
        let dir = tempdir().unwrap();
        // A file where the state dir should be makes IndexStore::open_at fail.
        let bogus = dir.path().join("not-a-dir");
        fs::write(&bogus, b"x").unwrap();

        let report = collect_blocking(Some(bogus), vec![], &[ProviderKind::ClaudeCode]);

        assert!(!report.available);
        assert!(report.message.is_some());
        assert!(report.providers.is_empty());
    }

    fn timeline_row(bucket: &str, sessions: i64, cost: f64) -> TimelineRow {
        TimelineRow {
            bucket: bucket.to_string(),
            session_count: sessions,
            cost_usd: cost,
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            tool_calls: 0,
            pr_count: 0,
            avg_turn_duration_ms: None,
        }
    }

    fn model_row(model: &str, sessions: i64, cost: f64) -> ModelUsageRow {
        ModelUsageRow {
            model: model.to_string(),
            session_count: sessions,
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: cost,
            avg_cost_per_session_usd: 0.0,
            avg_tokens_per_session: 0.0,
            service_tiers: Vec::new(),
            inference_geos: Vec::new(),
            avg_speed: None,
            total_iterations: 0,
        }
    }
}
