import { useState, type FormEvent } from "react";
import { AlertCircle, RefreshCw } from "lucide-react";
import { ProviderLogo } from "./ProviderLogo";
import type { ProviderKind } from "./types";
import type { LoginSession } from "./useLogin";

const providerNames: Record<ProviderKind, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  aws: "AWS",
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
  onSubmitCode,
}: {
  session: LoginSession;
  onCancel: () => void;
  onRetry: () => void;
  onSubmitCode: (code: string) => Promise<void>;
}) {
  const failed = session.status === "failed";
  const lastLine = session.lines[session.lines.length - 1];
  const [code, setCode] = useState("");
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const submitCode = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = code.trim();
    if (!trimmed) {
      setSubmitError("Paste the authentication code first.");
      return;
    }
    setSubmitting(true);
    setSubmitError(null);
    try {
      await onSubmitCode(trimmed);
      setCode("");
    } catch (error) {
      setSubmitError(String(error));
    } finally {
      setSubmitting(false);
    }
  };

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
              <button type="button" className="secondary" onClick={onCancel}>
                Close
              </button>
            </div>
          </>
        ) : (
          <>
            {!session.reauthId ? (
              <p className="login-note">
                <AlertCircle size={16} />
                <span>
                  Sign in with the account you want to <em>add</em>.
                  Re-authorizing an account that’s already signed in elsewhere
                  (such as your terminal {providerNames[session.provider]}) can
                  sign it out and force a re-login there. To add a different
                  account, switch accounts in the browser first.
                </span>
              </p>
            ) : null}
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
                {session.needsCode
                  ? "Need the code page again? "
                  : "Browser didn’t open? "}
                <a href={session.url} target="_blank" rel="noreferrer">
                  {session.needsCode
                    ? "Open the code page"
                    : "Open the sign-in page"}
                </a>
              </p>
            ) : null}
            {session.needsCode ? (
              <form className="login-code-form" onSubmit={submitCode}>
                <label htmlFor="login-auth-code">
                  Paste the full authentication code
                </label>
                <div className="login-code-row">
                  <input
                    id="login-auth-code"
                    type="password"
                    autoComplete="one-time-code"
                    value={code}
                    onChange={(event) => setCode(event.target.value)}
                    placeholder="authorization-code#state"
                  />
                  <button
                    type="submit"
                    className="primary"
                    disabled={submitting}
                  >
                    Submit code
                  </button>
                </div>
                {submitError ? (
                  <p className="login-code-error">{submitError}</p>
                ) : null}
              </form>
            ) : null}
            <div className="login-actions">
              <button type="button" className="secondary" onClick={onCancel}>
                Cancel
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
