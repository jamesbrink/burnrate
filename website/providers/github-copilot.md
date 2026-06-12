# GitHub Copilot

Burnrate tracks GitHub Copilot **premium requests** against your plan's
monthly allowance. The allowance resets on the **1st of each month at
00:00 UTC**.

## Setup

If you use the **Copilot CLI**, Burnrate auto-detects it from
`~/.copilot/session-state` — no configuration needed. You can also add it
manually via **Preferences → Add account → GitHub Copilot**.

Pick your **plan** so usage becomes a quota with warning/critical status:

| Plan       | Premium requests / month |
| ---------- | ------------------------ |
| Free       | 50                       |
| Pro        | 300                      |
| Pro+       | 1,500                    |
| Business   | 300                      |
| Enterprise | 1,000                    |
| Custom     | your own limit           |

Without a plan, Burnrate shows usage without a limit.

## Two data sources

**Local estimate (default, no credentials).** Burnrate counts premium
requests from Copilot CLI session logs on this machine, indexed by the
[claudex](https://utensils.io/claudex/) library. This is a **lower
bound**: IDE and web usage isn't visible locally, and only CLI sessions
that shut down cleanly record their counters. Every surface labels this an
estimate.

**GitHub billing (optional, exact).** Enter a GitHub **personal access
token (classic)** in the account form and Burnrate fetches your exact
month-to-date premium request usage from GitHub's billing API
(`GET /users/{username}/settings/billing/premium_request/usage`). Note
that GitHub's billing endpoints do **not** support fine-grained tokens,
and they only report usage billed to your personal account — a license
managed by an organization or enterprise is not included. If the billing
call fails (revoked token, network), the snapshot quietly falls back to
the local estimate with an explanatory note instead of erroring.

## Caveats

- The local estimate counts **Copilot CLI sessions on this Mac only**.
- `CLAUDEX_COPILOT_DIR` overrides the detected `~/.copilot` location for
  both detection and indexing.
- Copilot usage also appears in [Local insights](/guide/local-insights)
  alongside Claude Code and Codex.
