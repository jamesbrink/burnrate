import type {
  AccountInput,
  AccountView,
  AwsCategoryConfig,
  CopilotPlan,
  ProviderKind,
} from "./types";

export const OPENROUTER_DEFAULT_ENDPOINT =
  "https://openrouter.ai/api/v1/credits";
export const RUNPOD_DEFAULT_ENDPOINT = "https://rest.runpod.io/v1";
export const AWS_DEFAULT_REGION = "us-east-1";

export const providerLabels: Record<ProviderKind, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  openrouter: "OpenRouter",
  runpod: "Runpod",
  aws: "AWS",
  copilot: "GitHub Copilot",
};

export const providerDefaultEndpoints: Partial<Record<ProviderKind, string>> = {
  openrouter: OPENROUTER_DEFAULT_ENDPOINT,
  runpod: RUNPOD_DEFAULT_ENDPOINT,
};

/** GitHub Copilot plans and their monthly premium-request allowances
 *  (mirrors `CopilotPlan::monthly_limit` in src/models.rs). */
export const COPILOT_PLANS: {
  value: CopilotPlan;
  label: string;
  limit: number | null;
}[] = [
  { value: "free", label: "Free (50/mo)", limit: 50 },
  { value: "pro", label: "Pro (300/mo)", limit: 300 },
  { value: "pro-plus", label: "Pro+ (1,500/mo)", limit: 1500 },
  { value: "business", label: "Business (300/mo)", limit: 300 },
  { value: "enterprise", label: "Enterprise (1,000/mo)", limit: 1000 },
  { value: "custom", label: "Custom limit", limit: null },
];

export const defaultAwsCategories: AwsCategoryConfig[] = [
  {
    id: "all-aws",
    label: "All AWS",
    enabled: false,
    filter: { kind: "dimension", key: "SERVICE", values: [""] },
    groupBy: null,
  },
  {
    id: "bedrock",
    label: "Bedrock",
    enabled: true,
    filter: { kind: "dimension", key: "SERVICE", values: ["Amazon Bedrock"] },
    groupBy: { kind: "dimension", key: "USAGE_TYPE" },
  },
  {
    id: "ec2-compute",
    label: "EC2 compute",
    enabled: true,
    filter: {
      kind: "dimension",
      key: "SERVICE",
      values: ["Amazon Elastic Compute Cloud - Compute"],
    },
    groupBy: { kind: "dimension", key: "REGION" },
  },
  {
    id: "s3",
    label: "S3",
    enabled: false,
    filter: {
      kind: "dimension",
      key: "SERVICE",
      values: ["Amazon Simple Storage Service"],
    },
    groupBy: null,
  },
];

export function cloneDefaultAwsCategories(): AwsCategoryConfig[] {
  return defaultAwsCategories.map((category) => ({
    ...category,
    filter: { ...category.filter, values: [...category.filter.values] },
    groupBy: category.groupBy ? { ...category.groupBy } : null,
  }));
}

export const emptyForm: AccountInput = {
  provider: "openrouter",
  label: "OpenRouter",
  enabled: true,
  endpointOverride: OPENROUTER_DEFAULT_ENDPOINT,
  secretStorage: "keyring",
  secret: "",
  awsProfile: null,
  awsRegion: AWS_DEFAULT_REGION,
  awsMonthlyBudgetUsd: null,
  awsCategories: [],
  copilotPlan: null,
  copilotCustomLimit: null,
};

/** Display order for provider pickers (Add-account grid, first-run logos). */
export const PROVIDERS: ProviderKind[] = [
  "claude-code",
  "codex",
  "copilot",
  "aws",
  "openrouter",
  "runpod",
];

/** Providers authenticated through their CLI rather than a pasted API key. */
export const CLI_PROVIDERS: ProviderKind[] = ["claude-code", "codex"];

/** Fresh form state for adding a `provider` account. Picking a provider always
 *  starts clean — a secret typed for one provider never carries to another. */
export function formForProvider(provider: ProviderKind): AccountInput {
  return {
    provider,
    label: providerLabels[provider],
    enabled: true,
    endpointOverride: providerDefaultEndpoints[provider] ?? "",
    secretStorage: "keyring",
    secret: "",
    awsProfile: null,
    awsRegion: provider === "aws" ? AWS_DEFAULT_REGION : null,
    awsMonthlyBudgetUsd: null,
    awsCategories: provider === "aws" ? cloneDefaultAwsCategories() : [],
    copilotPlan: null,
    copilotCustomLimit: null,
  };
}

/** Form state prefilled from an existing account for editing. The secret is
 *  always blank — saving with it blank keeps the stored one. */
export function formFromAccount(account: AccountView): AccountInput {
  return {
    id: account.id,
    provider: account.provider,
    label: account.label,
    enabled: account.enabled,
    endpointOverride:
      account.endpointOverride ??
      providerDefaultEndpoints[account.provider] ??
      "",
    secretStorage: account.secretStorage,
    secret: "",
    awsProfile: account.awsProfile ?? null,
    // Only AWS accounts get the region default — seeding it for other
    // providers would persist AWS-only fields on every save/toggle.
    awsRegion:
      account.awsRegion ??
      (account.provider === "aws" ? AWS_DEFAULT_REGION : null),
    awsMonthlyBudgetUsd: account.awsMonthlyBudgetUsd ?? null,
    awsCategories:
      account.provider === "aws"
        ? account.awsCategories?.length
          ? account.awsCategories
          : cloneDefaultAwsCategories()
        : [],
    copilotPlan: account.copilotPlan ?? null,
    copilotCustomLimit: account.copilotCustomLimit ?? null,
  };
}
