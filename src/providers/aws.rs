use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use aws_config::{BehaviorVersion, meta::region::RegionProviderChain};
use aws_sdk_costexplorer::{
    Client as CostExplorerClient,
    operation::get_cost_and_usage::GetCostAndUsageOutput,
    types::{
        CostCategoryValues, DateInterval, Dimension, DimensionValues, Expression, Granularity,
        GroupDefinition, GroupDefinitionType, MetricValue, TagValues,
    },
};
use aws_sdk_sts::Client as StsClient;
use aws_types::region::Region;
use chrono::{Datelike, NaiveDate, Utc};

use crate::models::{
    AccountConfig, AwsCategoryConfig, AwsCostFilter, AwsGroupBy, AwsGroupByKind, ProviderKind,
    SnapshotStatus, UsageBucketSnapshot, UsageSnapshot,
};

use super::{overall_status, primary_quota};

const COST_METRIC: &str = "UnblendedCost";
const DEFAULT_REGION: &str = "us-east-1";
const USD: &str = "USD";

#[derive(Debug, Clone, Default, PartialEq)]
struct CostQueryResult {
    amount: f64,
    unit: String,
    estimated: bool,
    pages: usize,
    groups: Vec<CostGroup>,
}

#[derive(Debug, Clone, PartialEq)]
struct CostGroup {
    keys: Vec<String>,
    amount: f64,
    unit: String,
}

pub(crate) async fn fetch(account: &AccountConfig) -> Result<UsageSnapshot> {
    let shared_config = sdk_config(account).await;
    let identity = caller_identity(&shared_config).await?;
    let client = CostExplorerClient::new(&shared_config);
    let period = current_month_period()?;

    let overall = query_cost(&client, period.clone(), None, None)
        .await
        .context("AWS Cost Explorer GetCostAndUsage failed for all AWS spend")?;

    let categories = enabled_categories(account);
    let mut category_results = Vec::new();
    for category in &categories {
        let filter = category_filter_expression(category)?;
        let group_by = group_definition(category.group_by.as_ref());
        let result = query_cost(&client, period.clone(), filter, group_by)
            .await
            .with_context(|| {
                format!("AWS Cost Explorer failed for category '{}'", category.label)
            })?;
        category_results.push((category, result));
    }

    Ok(snapshot_from_costs(
        account,
        &identity,
        overall,
        &category_results,
    ))
}

fn snapshot_from_costs(
    account: &AccountConfig,
    identity: &str,
    overall: CostQueryResult,
    category_results: &[(&AwsCategoryConfig, CostQueryResult)],
) -> UsageSnapshot {
    let mut any_estimated = overall.estimated;
    let mut total_pages = overall.pages;

    let mut buckets = Vec::new();
    buckets.push(cost_bucket(
        "aws-mtd",
        "AWS month-to-date",
        overall.amount,
        overall.unit.as_str(),
        account.aws_monthly_budget_usd,
    ));

    for (category, result) in category_results {
        any_estimated |= result.estimated;
        total_pages += result.pages;
        buckets.push(cost_bucket(
            format!("aws-category-{}", category.id),
            category.label.clone(),
            result.amount,
            result.unit.as_str(),
            None,
        ));
    }

    let status = overall_status(&buckets);
    let quota = primary_quota(&buckets);
    let message = status_message(
        account,
        identity,
        any_estimated,
        total_pages,
        category_results.len(),
    );

    UsageSnapshot {
        account_id: account.id.clone(),
        provider: ProviderKind::Aws,
        label: account.label.clone(),
        status,
        email: None,
        subscription: None,
        usage_buckets: buckets,
        quota,
        message,
        fetched_at: Utc::now(),
    }
}

async fn sdk_config(account: &AccountConfig) -> aws_types::SdkConfig {
    let region = account
        .aws_region
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_REGION)
        .to_string();
    let region_provider = RegionProviderChain::first_try(Some(Region::new(region)));
    let mut loader = aws_config::defaults(BehaviorVersion::latest()).region(region_provider);
    if let Some(profile) = account
        .aws_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        loader = loader.profile_name(profile);
    }
    loader.load().await
}

async fn caller_identity(config: &aws_types::SdkConfig) -> Result<String> {
    let identity = StsClient::new(config)
        .get_caller_identity()
        .send()
        .await
        .map_err(|error| anyhow!(aws_permission_message(error)))?;
    let account = identity.account().unwrap_or("unknown-account");
    let arn = identity.arn().unwrap_or("unknown-arn");
    Ok(format!("{account} ({arn})"))
}

async fn query_cost(
    client: &CostExplorerClient,
    period: DateInterval,
    filter: Option<Expression>,
    group_by: Option<GroupDefinition>,
) -> Result<CostQueryResult> {
    let mut next_page_token: Option<String> = None;
    let mut aggregate = CostQueryResult {
        unit: USD.to_string(),
        ..CostQueryResult::default()
    };

    loop {
        let mut request = client
            .get_cost_and_usage()
            .time_period(period.clone())
            .granularity(Granularity::Monthly)
            .metrics(COST_METRIC);
        if let Some(filter) = filter.clone() {
            request = request.filter(filter);
        }
        if let Some(group_by) = group_by.clone() {
            request = request.group_by(group_by);
        }
        if let Some(token) = next_page_token.clone() {
            request = request.next_page_token(token);
        }

        let page = request
            .send()
            .await
            .map_err(|error| anyhow!(aws_permission_message(error)))?;
        merge_page(&mut aggregate, parse_cost_page(&page));
        next_page_token = page.next_page_token().map(ToString::to_string);
        if next_page_token.is_none() {
            break;
        }
    }

    Ok(aggregate)
}

fn current_month_period() -> Result<DateInterval> {
    let now = Utc::now().date_naive();
    let start = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .ok_or_else(|| anyhow!("failed to build current month start date"))?;
    let end = now
        .succ_opt()
        .ok_or_else(|| anyhow!("failed to build Cost Explorer end date"))?;
    DateInterval::builder()
        .start(start.format("%Y-%m-%d").to_string())
        .end(end.format("%Y-%m-%d").to_string())
        .build()
        .context("failed to build AWS Cost Explorer time period")
}

fn enabled_categories(account: &AccountConfig) -> Vec<AwsCategoryConfig> {
    account
        .aws_categories
        .iter()
        .filter(|category| category.enabled)
        .cloned()
        .collect()
}

fn category_filter_expression(category: &AwsCategoryConfig) -> Result<Option<Expression>> {
    if category.id == "all-aws" {
        Ok(None)
    } else {
        filter_expression(&category.filter).map(Some)
    }
}

fn filter_expression(filter: &AwsCostFilter) -> Result<Expression> {
    match filter {
        AwsCostFilter::Dimension { key, values } => {
            let values = clean_values(values, key)?;
            let mut dimensions = DimensionValues::builder().key(Dimension::from(key.as_str()));
            for value in values {
                dimensions = dimensions.values(value);
            }
            Ok(Expression::builder().dimensions(dimensions.build()).build())
        }
        AwsCostFilter::Tag { key, values } => {
            let values = clean_values(values, key)?;
            let mut tags = TagValues::builder().key(key);
            for value in values {
                tags = tags.values(value);
            }
            Ok(Expression::builder().tags(tags.build()).build())
        }
        AwsCostFilter::CostCategory { key, values } => {
            let values = clean_values(values, key)?;
            let mut cost_category = CostCategoryValues::builder().key(key);
            for value in values {
                cost_category = cost_category.values(value);
            }
            Ok(Expression::builder()
                .cost_categories(cost_category.build())
                .build())
        }
    }
}

fn clean_values(values: &[String], key: &str) -> Result<Vec<String>> {
    let values: Vec<String> = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect();
    if values.is_empty() {
        Err(anyhow!(
            "AWS Cost Explorer filter '{key}' must include at least one value"
        ))
    } else {
        Ok(values)
    }
}

fn group_definition(group_by: Option<&AwsGroupBy>) -> Option<GroupDefinition> {
    group_by.map(|group| {
        let group_type = match group.kind {
            AwsGroupByKind::Dimension => GroupDefinitionType::Dimension,
            AwsGroupByKind::Tag => GroupDefinitionType::Tag,
            AwsGroupByKind::CostCategory => GroupDefinitionType::CostCategory,
        };
        GroupDefinition::builder()
            .r#type(group_type)
            .key(group.key.clone())
            .build()
    })
}

fn parse_cost_page(output: &GetCostAndUsageOutput) -> CostQueryResult {
    let mut result = CostQueryResult {
        unit: USD.to_string(),
        pages: 1,
        ..CostQueryResult::default()
    };

    for time in output.results_by_time() {
        result.estimated |= time.estimated();
        let total = time.total().and_then(metric_amount);
        let mut group_sum = 0.0;
        let mut group_unit: Option<String> = None;
        for group in time.groups() {
            if let Some(metrics) = group.metrics()
                && let Some((amount, unit)) = metric_amount(metrics)
            {
                group_sum += amount;
                group_unit = Some(unit.clone());
                result.groups.push(CostGroup {
                    keys: group.keys().to_vec(),
                    amount,
                    unit,
                });
            }
        }
        if let Some((amount, unit)) = total {
            result.amount += amount;
            result.unit = unit;
        } else if group_sum > 0.0 {
            result.amount += group_sum;
            result.unit = group_unit.unwrap_or_else(|| USD.to_string());
        }
    }

    result
}

fn metric_amount(metrics: &HashMap<String, MetricValue>) -> Option<(f64, String)> {
    let metric = metrics.get(COST_METRIC)?;
    let amount = metric.amount()?.parse::<f64>().ok()?;
    let unit = metric.unit().unwrap_or(USD).to_string();
    Some((amount, unit))
}

fn merge_page(total: &mut CostQueryResult, page: CostQueryResult) {
    total.amount += page.amount;
    total.estimated |= page.estimated;
    total.pages += page.pages;
    if !page.unit.is_empty() {
        total.unit = page.unit;
    }
    total.groups.extend(page.groups);
}

fn cost_bucket(
    id: impl Into<String>,
    label: impl Into<String>,
    used: f64,
    unit: &str,
    limit: Option<f64>,
) -> UsageBucketSnapshot {
    let remaining = limit.map(|limit| (limit - used).max(0.0));
    let status = budget_status(limit, used);
    UsageBucketSnapshot {
        id: id.into(),
        label: label.into(),
        window: Some("month-to-date".to_string()),
        used,
        limit,
        remaining,
        unit: unit.to_string(),
        reset_at: None,
        status,
    }
}

fn budget_status(limit: Option<f64>, used: f64) -> SnapshotStatus {
    match limit {
        Some(limit) if limit > 0.0 && (limit - used).max(0.0) / limit <= 0.05 => {
            SnapshotStatus::Exhausted
        }
        Some(limit) if limit > 0.0 && (limit - used).max(0.0) / limit <= 0.2 => {
            SnapshotStatus::Warning
        }
        _ => SnapshotStatus::Healthy,
    }
}

fn status_message(
    account: &AccountConfig,
    identity: &str,
    estimated: bool,
    pages: usize,
    category_count: usize,
) -> Option<String> {
    let mut parts = vec![format!("AWS account {identity}")];
    if let Some(profile) = account
        .aws_profile
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("profile {profile}"));
    }
    if estimated {
        parts.push("Cost Explorer marks current data as estimated".to_string());
    }
    if pages > 1 {
        parts.push(format!("read {pages} Cost Explorer pages"));
    }
    if category_count > 0 {
        parts.push(format!("{} enabled AWS categories", category_count));
    }
    Some(parts.join(" · "))
}

fn aws_permission_message(error: impl std::fmt::Display) -> String {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("accessdenied") || lower.contains("unauthorized") {
        return "AWS credentials lack required read-only permissions. Grant ce:GetCostAndUsage and sts:GetCallerIdentity for Cost Explorer snapshots.".to_string();
    }
    if lower.contains("expired") || lower.contains("sso") || lower.contains("credential") {
        return "AWS credentials are unavailable or expired. Run `aws sso login` or verify AWS_PROFILE/shared credentials, then refresh Burnrate.".to_string();
    }
    if lower.contains("data unavailable") || lower.contains("bill") {
        return "AWS Cost Explorer data is unavailable yet. Enable Cost Explorer/billing access and allow AWS billing data to refresh.".to_string();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_costexplorer::types::{Group, ResultByTime};

    fn metric(amount: &str) -> MetricValue {
        MetricValue::builder().amount(amount).unit(USD).build()
    }

    fn account() -> AccountConfig {
        AccountConfig {
            id: "aws-main".to_string(),
            provider: ProviderKind::Aws,
            label: "AWS".to_string(),
            enabled: true,
            auto_detected: false,
            credential_path: None,
            endpoint_override: None,
            secret_storage: crate::models::SecretStorageMode::Keyring,
            keyring_account: None,
            plaintext_secret: None,
            email: None,
            config_dir: None,
            aws_profile: Some("work".to_string()),
            aws_region: Some(DEFAULT_REGION.to_string()),
            aws_monthly_budget_usd: None,
            aws_categories: vec![AwsCategoryConfig {
                id: "bedrock".to_string(),
                label: "Bedrock".to_string(),
                enabled: true,
                filter: AwsCostFilter::Dimension {
                    key: "SERVICE".to_string(),
                    values: vec!["Amazon Bedrock".to_string()],
                },
                group_by: None,
            }],
            order_index: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn parses_cost_explorer_totals_groups_estimated_and_pagination() {
        let page = GetCostAndUsageOutput::builder()
            .next_page_token("next")
            .results_by_time(
                ResultByTime::builder()
                    .estimated(true)
                    .total(COST_METRIC, metric("12.34"))
                    .groups(
                        Group::builder()
                            .keys("us-east-1")
                            .metrics(COST_METRIC, metric("2.50"))
                            .build(),
                    )
                    .build(),
            )
            .build();

        let parsed = parse_cost_page(&page);

        assert_eq!(parsed.amount, 12.34);
        assert_eq!(parsed.unit, USD);
        assert!(parsed.estimated);
        assert_eq!(parsed.pages, 1);
        assert_eq!(parsed.groups[0].keys, vec!["us-east-1"]);
        assert_eq!(parsed.groups[0].amount, 2.50);
    }

    #[test]
    fn parses_group_amounts_when_total_is_absent() {
        let page = GetCostAndUsageOutput::builder()
            .results_by_time(
                ResultByTime::builder()
                    .groups(
                        Group::builder()
                            .keys("Amazon Bedrock")
                            .metrics(COST_METRIC, metric("2.50"))
                            .build(),
                    )
                    .groups(
                        Group::builder()
                            .keys("Amazon S3")
                            .metrics(COST_METRIC, metric("1.25"))
                            .build(),
                    )
                    .build(),
            )
            .build();

        let parsed = parse_cost_page(&page);

        assert_eq!(parsed.amount, 3.75);
        assert_eq!(parsed.unit, USD);
        assert_eq!(parsed.groups.len(), 2);
    }

    #[test]
    fn builds_current_month_period() {
        let period = current_month_period().unwrap();
        assert!(period.start().ends_with("-01"));
        assert!(period.end() >= period.start());
    }

    #[test]
    fn all_aws_category_uses_no_filter() {
        let category = AwsCategoryConfig {
            id: "all-aws".to_string(),
            label: "All AWS".to_string(),
            enabled: true,
            filter: AwsCostFilter::Dimension {
                key: "SERVICE".to_string(),
                values: vec![String::new()],
            },
            group_by: None,
        };

        assert!(category_filter_expression(&category).unwrap().is_none());
    }

    #[test]
    fn builds_dimension_filter_and_grouping() {
        let filter = AwsCostFilter::Dimension {
            key: "SERVICE".to_string(),
            values: vec!["Amazon Bedrock".to_string(), "Amazon S3".to_string()],
        };
        let expression = filter_expression(&filter).unwrap();
        let dimensions = expression.dimensions().unwrap();
        assert_eq!(dimensions.key(), Some(&Dimension::Service));
        assert_eq!(dimensions.values().len(), 2);

        let group = group_definition(Some(&AwsGroupBy {
            kind: AwsGroupByKind::Dimension,
            key: "USAGE_TYPE".to_string(),
        }))
        .unwrap();
        assert_eq!(group.r#type(), Some(&GroupDefinitionType::Dimension));
        assert_eq!(group.key(), Some("USAGE_TYPE"));
    }

    #[test]
    fn builds_tag_and_cost_category_filters_and_grouping() {
        let tag = filter_expression(&AwsCostFilter::Tag {
            key: "Team".to_string(),
            values: vec!["AI".to_string()],
        })
        .unwrap();
        let tags = tag.tags().unwrap();
        assert_eq!(tags.key(), Some("Team"));
        assert_eq!(tags.values(), ["AI"]);

        let cost_category = filter_expression(&AwsCostFilter::CostCategory {
            key: "BusinessUnit".to_string(),
            values: vec!["Platform".to_string()],
        })
        .unwrap();
        let cost_categories = cost_category.cost_categories().unwrap();
        assert_eq!(cost_categories.key(), Some("BusinessUnit"));
        assert_eq!(cost_categories.values(), ["Platform"]);

        let tag_group = group_definition(Some(&AwsGroupBy {
            kind: AwsGroupByKind::Tag,
            key: "Team".to_string(),
        }))
        .unwrap();
        assert_eq!(tag_group.r#type(), Some(&GroupDefinitionType::Tag));

        let category_group = group_definition(Some(&AwsGroupBy {
            kind: AwsGroupByKind::CostCategory,
            key: "BusinessUnit".to_string(),
        }))
        .unwrap();
        assert_eq!(
            category_group.r#type(),
            Some(&GroupDefinitionType::CostCategory)
        );
    }

    #[test]
    fn rejects_blank_filter_values() {
        let error = filter_expression(&AwsCostFilter::Tag {
            key: "Team".to_string(),
            values: vec![" ".to_string()],
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("must include at least one value"));
    }

    #[test]
    fn builds_usage_snapshot_from_overall_and_category_costs() {
        let account = AccountConfig {
            aws_monthly_budget_usd: Some(10.0),
            ..account()
        };
        let category = account.aws_categories[0].clone();
        let category_result = CostQueryResult {
            amount: 2.0,
            unit: USD.to_string(),
            estimated: true,
            pages: 2,
            groups: Vec::new(),
        };
        let snapshot = snapshot_from_costs(
            &account,
            "123456789012 (arn)",
            CostQueryResult {
                amount: 9.0,
                unit: USD.to_string(),
                estimated: false,
                pages: 1,
                groups: Vec::new(),
            },
            &[(&category, category_result)],
        );

        assert_eq!(snapshot.account_id, "aws-main");
        assert_eq!(snapshot.provider, ProviderKind::Aws);
        assert_eq!(snapshot.status, SnapshotStatus::Warning);
        assert_eq!(snapshot.usage_buckets.len(), 2);
        assert_eq!(snapshot.quota.unwrap().remaining, Some(1.0));
        assert!(
            snapshot
                .message
                .unwrap()
                .contains("read 3 Cost Explorer pages")
        );
    }

    #[test]
    fn merges_cost_pages_and_updates_status_message() {
        let mut total = CostQueryResult {
            amount: 2.0,
            unit: USD.to_string(),
            estimated: false,
            pages: 1,
            groups: Vec::new(),
        };
        merge_page(
            &mut total,
            CostQueryResult {
                amount: 3.0,
                unit: "EUR".to_string(),
                estimated: true,
                pages: 2,
                groups: vec![CostGroup {
                    keys: vec!["Team".to_string()],
                    amount: 3.0,
                    unit: "EUR".to_string(),
                }],
            },
        );

        assert_eq!(total.amount, 5.0);
        assert_eq!(total.unit, "EUR");
        assert!(total.estimated);
        assert_eq!(total.pages, 3);
        assert_eq!(total.groups.len(), 1);

        let message = status_message(&account(), "123456789012 (arn)", true, 3, 1).unwrap();
        assert!(message.contains("profile work"));
        assert!(message.contains("estimated"));
        assert!(message.contains("read 3 Cost Explorer pages"));
        assert!(message.contains("1 enabled AWS categories"));
    }

    #[test]
    fn metric_amount_rejects_missing_and_invalid_values() {
        assert!(metric_amount(&HashMap::new()).is_none());
        let mut metrics = HashMap::new();
        metrics.insert(
            COST_METRIC.to_string(),
            MetricValue::builder().amount("not-a-number").build(),
        );
        assert!(metric_amount(&metrics).is_none());
    }

    #[test]
    fn no_limit_cost_bucket_is_healthy_without_remaining() {
        let bucket = cost_bucket("aws-mtd", "AWS", 42.0, USD, None);
        assert_eq!(bucket.status, SnapshotStatus::Healthy);
        assert_eq!(bucket.limit, None);
        assert_eq!(bucket.remaining, None);
    }

    #[test]
    fn budget_limit_maps_warning_and_exhausted_statuses() {
        assert_eq!(budget_status(Some(100.0), 79.0), SnapshotStatus::Healthy);
        assert_eq!(budget_status(Some(100.0), 80.0), SnapshotStatus::Warning);
        assert_eq!(budget_status(Some(100.0), 95.0), SnapshotStatus::Exhausted);
        assert_eq!(budget_status(Some(100.0), 100.0), SnapshotStatus::Exhausted);
    }

    #[test]
    fn enabled_categories_skip_disabled_entries() {
        let mut account = account();
        account.aws_categories.push(AwsCategoryConfig {
            id: "ec2".to_string(),
            label: "EC2".to_string(),
            enabled: false,
            filter: AwsCostFilter::Dimension {
                key: "SERVICE".to_string(),
                values: vec!["Amazon Elastic Compute Cloud - Compute".to_string()],
            },
            group_by: None,
        });

        let categories = enabled_categories(&account);
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].id, "bedrock");
    }

    #[test]
    fn permission_and_credential_errors_are_clear() {
        let access_denied = aws_permission_message("AccessDeniedException: denied");
        assert!(access_denied.contains("ce:GetCostAndUsage"));
        assert!(!access_denied.contains("ce:GetDimensionValues"));
        assert!(aws_permission_message("SSO token expired").contains("aws sso login"));
        assert!(
            aws_permission_message("Data unavailable for billing")
                .contains("Cost Explorer data is unavailable")
        );
    }
}
