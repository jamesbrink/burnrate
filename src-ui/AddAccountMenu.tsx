import { useState } from "react";
import { ArrowLeft, KeyRound, LogIn } from "lucide-react";
import { ProviderLogo } from "./ProviderLogo";
import { providerLabels } from "./constants";
import type { ProviderKind } from "./types";

const PROVIDERS: ProviderKind[] = [
  "claude-code",
  "codex",
  "copilot",
  "aws",
  "openrouter",
  "runpod",
];

/** Providers that support interactive browser sign-in. */
const LOGIN_PROVIDERS: ProviderKind[] = ["claude-code", "codex"];

/**
 * Two-step "Add account" chooser: pick a provider, then (for Claude Code / Codex)
 * pick browser sign-in vs. manual token entry. OpenRouter / Runpod skip straight
 * to the manual form.
 */
export function AddAccountMenu({
  onStartLogin,
  onManual,
  onClose,
}: {
  onStartLogin: (provider: ProviderKind) => void;
  onManual: (provider: ProviderKind) => void;
  onClose: () => void;
}) {
  const [provider, setProvider] = useState<ProviderKind | null>(null);

  if (provider && LOGIN_PROVIDERS.includes(provider)) {
    return (
      <div className="add-account-menu" role="menu" aria-label="Sign-in method">
        <button
          type="button"
          className="add-account-back"
          onClick={() => setProvider(null)}
        >
          <ArrowLeft size={14} /> Back
        </button>
        <button
          type="button"
          className="add-account-option"
          role="menuitem"
          onClick={() => {
            onStartLogin(provider);
            onClose();
          }}
        >
          <LogIn size={16} />
          <span>
            <strong>Sign in with browser</strong>
            <small>Recommended</small>
          </span>
        </button>
        <button
          type="button"
          className="add-account-option"
          role="menuitem"
          onClick={() => {
            onManual(provider);
            onClose();
          }}
        >
          <KeyRound size={16} />
          <span>
            <strong>Enter a token manually</strong>
            <small>Advanced</small>
          </span>
        </button>
      </div>
    );
  }

  return (
    <div
      className="add-account-menu"
      role="menu"
      aria-label="Choose a provider"
    >
      {PROVIDERS.map((option) => (
        <button
          key={option}
          type="button"
          className="add-account-option"
          role="menuitem"
          onClick={() => {
            if (LOGIN_PROVIDERS.includes(option)) {
              setProvider(option);
            } else {
              onManual(option);
              onClose();
            }
          }}
        >
          <ProviderLogo provider={option} size="sm" />
          <span>
            <strong>{providerLabels[option]}</strong>
          </span>
        </button>
      ))}
    </div>
  );
}
