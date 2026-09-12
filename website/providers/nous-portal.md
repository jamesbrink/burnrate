# Nous Portal

Burnrate shows reported Nous Portal credit balances using your existing
Hermes login. It reads `GET /api/oauth/account` and displays total available,
subscription, and top-up credits, plus the plan name and email when provided.

## Setup and profiles

Sign in to Nous in Hermes, then launch Burnrate or use **Detect accounts**.
Burnrate discovers one Nous Portal account. No token entry is needed.

Credentials are read in this order:

1. The shared Nous store, normally `~/.hermes/shared/nous_auth.json`.
   Hermes writes this store when signing in or refreshing from a named
   profile too, so the default profile does not need to use Nous.
2. `providers.nous` in the selected Hermes home's `auth.json`, as a fallback
   when the shared store has no readable access token.

Burnrate honors `HERMES_SHARED_AUTH_DIR` for the shared store directory and
`HERMES_HOME` for the selected profile. For a custom home of the form
`<root>/profiles/<name>`, the shared store defaults to `<root>/shared`.
Otherwise a custom home outside `~/.hermes` is its own root. Without an
explicit home, the profile fallback is `~/.hermes/auth.json`.

Burnrate does not scan sibling profiles or infer the active profile. If an
older login exists only in a named profile's auth file, sign in through
Hermes to populate the shared store, or launch Burnrate with `HERMES_HOME`
pointing to that profile. Environment variables must be available to the
Burnrate process; a Finder launch does not inherit your shell configuration.

## Read-only access

Hermes owns login and token refresh. Burnrate rereads the access token on
uncached fetches, never refreshes it, and never writes Hermes auth files.
The refresh-token field is ignored. Discovered credentials are not copied to
Burnrate's database or keyring and are not sent to the frontend.

The Portal URL stored with the credential is used when present, otherwise
`https://portal.nousresearch.com`. HTTPS is required except for localhost.

## Balance semantics

- **Total available** uses `paid_service_access.total_usable_credits`.
- **Subscription credits** uses
  `paid_service_access.subscription_credits_remaining`, falling back to
  `subscription.credits_remaining`. A reported `current_period_end` is
  shown only on this bucket.
- **Purchased credits** uses `paid_service_access.purchased_credits_remaining`.

Only reported balances are shown. Burnrate does not sum an inferred total,
clamp balances to a monthly allowance, derive spending, or attach a reset
time to purchased credits. Only subscription credits have a meter, measured
against the reported monthly allowance. Rollover can put the balance above
that allowance: the dollar amount is preserved and the fill stops at 100%.
Purchased credits include unspent purchases from earlier periods and are displayed as a number.
Total available and reported subscription rollover appear in expanded credit details.
Rollover metadata is never added to the spendable balance. Missing or zero allowances
also omit the subscription meter. There are no percentage-based warning
thresholds. A reported zero total marks the account exhausted; a positive
total marks it healthy. When the total is missing, a positive reported
component keeps the account healthy. This status describes credit balances,
not a guarantee that inference is permitted.

No billing fallback endpoints, organization spend caps, rollover detail,
or local usage analytics are included. Missing or invalid balance data
produces an error rather than a healthy empty card.

## Troubleshooting

If the token expires or is rejected, refresh your Nous login in Hermes,
then refresh Burnrate. Permission-denied responses are reported separately;
check your Nous account access in Hermes. Burnrate cannot renew the login
while Hermes is inactive.
