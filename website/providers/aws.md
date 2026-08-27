# AWS

Burnrate tracks AWS **month-to-date spend** from Cost Explorer, with an
optional monthly budget and configurable per-service buckets.

## Setup

Add AWS from **Preferences → Add account → AWS**. Burnrate uses the
official AWS SDK default credential chain:

- Leave the profile blank to use `AWS_PROFILE` / `default`.
- Or enter a shared config/credentials/SSO profile name.

No static AWS access keys are stored in Burnrate. Cost Explorer is a global
billing API; the SDK region defaults to `us-east-1` when none is
configured.

## What it shows

The primary bucket is current month-to-date `UnblendedCost` in USD from
Cost Explorer `GetCostAndUsage`. Configure an optional **monthly budget**
to turn raw spend into Burnrate-style remaining/warning/exhausted status.

**Category buckets** are user-editable Cost Explorer filters. Presets
include:

- Bedrock — `SERVICE = Amazon Bedrock`
- EC2 compute — `SERVICE = Amazon Elastic Compute Cloud - Compute`
- S3 — `SERVICE = Amazon Simple Storage Service`

plus custom dimension / tag / cost-category filters with optional group-by.

Burnrate groups its account-wide query by service and derives `SERVICE`
categories from that response. Each enabled tag or cost-category filter needs
an additional `GetCostAndUsage` query. Pagination adds another billable API
request per page, and Burnrate reports the actual request count in the AWS
snapshot message.

## Required permissions

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": ["ce:GetCostAndUsage", "sts:GetCallerIdentity"],
      "Resource": "*"
    }
  ]
}
```

Optional future features may require additional permissions only when
enabled: service discovery/autocomplete (`ce:GetDimensionValues`), Budgets
(`budgets:DescribeBudget`, `budgets:DescribeBudgets`), Free Tier
(`freetier:GetFreeTierUsage`), and Bedrock operational CloudWatch metrics
(`cloudwatch:GetMetricData`, `cloudwatch:ListMetrics`).

## Caveats

- AWS charges **$0.01 per paginated Cost Explorer API request** for a primary
  billing view. Burnrate caches an AWS snapshot for 24 hours, including manual
  refreshes during that window, because Cost Explorer data itself refreshes
  about daily. Other providers retain the normal five-minute cadence. See
  [AWS Cost Explorer pricing](https://aws.amazon.com/aws-cost-management/aws-cost-explorer/pricing/).
- Cost Explorer data is **not real time** — current-month data can be
  delayed, and AWS may mark fresh results as estimated. Burnrate surfaces
  estimated results and pagination in the account message.
- `UsageQuantity` is intentionally not used for the primary bucket because
  units aren't meaningful across mixed AWS services.
