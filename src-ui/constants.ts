import type {
  AccountInput,
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
