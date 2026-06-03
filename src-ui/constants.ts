import type { AccountInput, ProviderKind } from "./types";

export const OPENROUTER_DEFAULT_ENDPOINT =
  "https://openrouter.ai/api/v1/credits";
export const RUNPOD_DEFAULT_ENDPOINT = "https://rest.runpod.io/v1";

export const providerLabels: Record<ProviderKind, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  openrouter: "OpenRouter",
  runpod: "Runpod",
};

export const providerDefaultEndpoints: Partial<Record<ProviderKind, string>> = {
  openrouter: OPENROUTER_DEFAULT_ENDPOINT,
  runpod: RUNPOD_DEFAULT_ENDPOINT,
};

export const emptyForm: AccountInput = {
  provider: "openrouter",
  label: "OpenRouter",
  enabled: true,
  endpointOverride: OPENROUTER_DEFAULT_ENDPOINT,
  secretStorage: "keyring",
  secret: "",
};
