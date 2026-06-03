import { AlertCircle, RefreshCw } from "lucide-react";
import { ProviderLogo } from "./ProviderLogo";
import type { ProviderKind } from "./types";
import type { LoginSession } from "./useLogin";

const providerNames: Record<ProviderKind, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  openrouter: "OpenRouter",
  runpod: "Runpod",
};

/**
 * Presentational sign-in modal. All state lives in {@link useLogin}; this only
 * renders the current session and forwards Cancel / Retry.
 */
export function LoginModal({
  session,
  onCancel,
  onRetry,
}: {
  session: LoginSession;
  onCancel: () => void;
  onRetry: () => void;
}) {
  const failed = session.status === "failed";
  const lastLine = session.lines[session.lines.length - 1];

  return (
    <div className="login-overlay" role="presentation" onClick={onCancel}>
      <div
        className="login-modal"
        role="dialog"
        aria-modal="true"
        aria-label={`Sign in to ${providerNames[session.provider]}`}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="login-modal-head">
          <ProviderLogo provider={session.provider} size="md" />
          <h2>Sign in to {providerNames[session.provider]}</h2>
        </header>

        {failed ? (
          <>
            <p className="login-error">
              <AlertCircle size={16} />
              <span>{session.error ?? "Sign-in failed."}</span>
            </p>
            <div className="login-actions">
              <button type="button" className="primary" onClick={onRetry}>
                Try again
              </button>
              <button
                type="button"
                className="icon-button subtle"
                onClick={onCancel}
              >
                Close
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="login-status">
              <RefreshCw size={16} className="spin" />
              <span>
                {session.status === "starting"
                  ? "Opening your browser…"
                  : (lastLine ??
                    "Waiting for you to authorize in the browser…")}
              </span>
            </p>
            {session.url ? (
              <p className="login-url">
                Browser didn’t open?{" "}
                <a href={session.url} target="_blank" rel="noreferrer">
                  Open the sign-in page
                </a>
              </p>
            ) : null}
            <div className="login-actions">
              <button
                type="button"
                className="icon-button subtle"
                onClick={onCancel}
              >
                Cancel
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
