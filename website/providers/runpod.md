# Runpod

Burnrate shows your Runpod **prepaid balance**, **current burn rate**, the
resulting **runway**, plus active resources and recent Pods / Serverless /
storage costs.

## Setup

1. Create an API key in the
   [Runpod console](https://www.runpod.io/console/user/settings).
2. In **Preferences → Add account → Runpod**, paste the key.

The key is stored in the OS keyring by default (plaintext storage is an
explicit opt-in per account).

## Endpoints

Runpod data combines two APIs:

- REST — `https://rest.runpod.io/v1` (resources, billing)
- GraphQL — `https://api.runpod.io/graphql` (balance, burn state)

Override either for development or proxies with
`BURNRATE_RUNPOD_REST_URL` / `BURNRATE_RUNPOD_GRAPHQL_URL`.
