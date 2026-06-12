# Local insights

Quota buckets tell you what a vendor says **remains**; local insights show
what your CLIs actually **consumed**. Burnrate embeds the
[claudex](https://utensils.io/claudex/) library to index local session
logs — Claude Code, Codex, and GitHub Copilot — and renders per-provider
usage in both windows:

- **Tray popover** — under each provider's first card: a 14-day daily-cost
  sparkline, today's cost and session count, month-to-date cost, and a
  linear month-end projection.
- **Preferences → Local insights** — per-provider cards with
  Today/Week/Month/Projected stats, a daily cost timeline, the month's
  model distribution by cost, and your most expensive projects.

## How it works

claudex maintains a sqlite index at `~/.claudex/index.db` (`CLAUDEX_DIR`
overrides it), shared safely with the [claudex
CLI](https://github.com/utensils/claudex) if you use it. Collection always
runs off the dashboard path: quota cards render immediately and insights
hydrate when ready. The **first** collection on a machine with a large
session history builds the entire index and can take a few minutes; after
that it's incremental and cheap.

Costs are **API-price estimates** computed from token counts — useful for
trends and proportions, not your invoice (subscription plans don't bill
per token). Local history can't be split between multiple accounts of the
same provider, so insights are per **provider**, and the UI says so when
several accounts share them.

## Turning it off

Disable **Preferences → Local insights** and Burnrate stops reading
session logs and leaves the claudex index untouched. Headless inspection
is available via the hidden CLI:

```sh
burnrate debug insights
```
