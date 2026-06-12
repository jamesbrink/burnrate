import {
  AlertCircle,
  ArrowLeft,
  KeyRound,
  LogIn,
  LogOut,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { AccountFields } from "./AccountFields";
import { ProviderLogo } from "./ProviderLogo";
import {
  CLI_PROVIDERS,
  PROVIDERS,
  emptyForm,
  formForProvider,
  formFromAccount,
  providerLabels,
} from "./constants";
import type { AccountInput, AccountView, ProviderKind } from "./types";

export type AccountModalMode =
  | { kind: "add" }
  | { kind: "edit"; account: AccountView };

type Step = "provider" | "method" | "fields";

/**
 * The add/edit account dialog. Adding walks a short wizard — provider grid,
 * then (for CLI providers) browser sign-in vs. manual entry, then the fields.
 * Editing opens straight on the fields with the provider fixed: switching an
 * existing account's provider is never valid. Save failures stay inline so
 * the typed input isn't lost.
 */
export function AccountModal({
  mode,
  busy,
  canReauth,
  onSave,
  onClose,
  onStartLogin,
  onLogout,
  onRemove,
}: {
  mode: AccountModalMode;
  busy: boolean;
  /** Edit mode only: whether the backend would accept an in-place re-auth. */
  canReauth: boolean;
  /** Resolves on success (the modal closes itself); a rejection is shown inline. */
  onSave: (input: AccountInput) => Promise<void>;
  onClose: () => void;
  onStartLogin: (provider: ProviderKind, accountId?: string) => void;
  onLogout: (id: string) => void;
  onRemove: (id: string) => void;
}) {
  const isEdit = mode.kind === "edit";
  const [step, setStep] = useState<Step>(isEdit ? "fields" : "provider");
  const [form, setForm] = useState<AccountInput>(() =>
    isEdit ? formFromAccount(mode.account) : emptyForm,
  );
  const [saveError, setSaveError] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    }
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () =>
      window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [onClose]);

  function pickProvider(provider: ProviderKind) {
    // Always a fresh form: a secret typed for one provider must never carry
    // over to another (the Back button makes re-picking possible).
    setForm(formForProvider(provider));
    setSaveError(null);
    setStep(CLI_PROVIDERS.includes(provider) ? "method" : "fields");
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setSaveError(null);
    try {
      await onSave(form);
      onClose();
    } catch (error) {
      setSaveError(String(error));
    }
  }

  const isCliProvider = CLI_PROVIDERS.includes(form.provider);

  return (
    <div className="login-overlay" role="presentation" onClick={onClose}>
      <div
        className="login-modal account-modal"
        role="dialog"
        aria-modal="true"
        aria-label={isEdit ? `Edit ${mode.account.label}` : "Add Account"}
        onClick={(event) => event.stopPropagation()}
      >
        {step === "provider" ? (
          <>
            <header className="login-modal-head">
              <h2>Add Account</h2>
              <button
                type="button"
                className="modal-back modal-close"
                title="Close"
                onClick={onClose}
              >
                <X size={15} />
              </button>
            </header>
            <div className="provider-grid" role="menu" aria-label="Provider">
              {PROVIDERS.map((provider) => (
                <button
                  key={provider}
                  type="button"
                  role="menuitem"
                  className="provider-tile"
                  onClick={() => pickProvider(provider)}
                >
                  <ProviderLogo provider={provider} size="md" />
                  <span>{providerLabels[provider]}</span>
                </button>
              ))}
            </div>
          </>
        ) : null}

        {step === "method" ? (
          <>
            <header className="login-modal-head">
              <button
                type="button"
                className="modal-back"
                title="Back"
                onClick={() => setStep("provider")}
              >
                <ArrowLeft size={15} />
              </button>
              <ProviderLogo provider={form.provider} size="md" />
              <h2>{providerLabels[form.provider]}</h2>
              <button
                type="button"
                className="modal-back modal-close"
                title="Close"
                onClick={onClose}
              >
                <X size={15} />
              </button>
            </header>
            <div role="menu" aria-label="Sign-in method" className="method-list">
              <button
                type="button"
                className="add-account-option"
                role="menuitem"
                onClick={() => {
                  onStartLogin(form.provider);
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
                onClick={() => setStep("fields")}
              >
                <KeyRound size={16} />
                <span>
                  <strong>Enter a token manually</strong>
                  <small>Advanced</small>
                </span>
              </button>
            </div>
          </>
        ) : null}

        {step === "fields" ? (
          <form className="account-form account-modal-form" onSubmit={handleSubmit}>
            <header className="login-modal-head">
              {!isEdit ? (
                <button
                  type="button"
                  className="modal-back"
                  title="Back"
                  onClick={() =>
                    setStep(isCliProvider ? "method" : "provider")
                  }
                >
                  <ArrowLeft size={15} />
                </button>
              ) : null}
              <ProviderLogo provider={form.provider} size="md" />
              <h2>
                {isEdit ? mode.account.label : providerLabels[form.provider]}
              </h2>
              {isEdit && mode.account.email ? (
                <span className="account-email">{mode.account.email}</span>
              ) : null}
              <button
                type="button"
                className="modal-back modal-close"
                title="Close"
                onClick={onClose}
              >
                <X size={15} />
              </button>
            </header>

            <div className="account-modal-body">
              <AccountFields form={form} setForm={setForm} isEdit={isEdit} />

              {isEdit && isCliProvider ? (
                <div className="account-auth-actions">
                  {canReauth ? (
                    <button
                      type="button"
                      className="secondary"
                      onClick={() => {
                        onStartLogin(form.provider, mode.account.id);
                        onClose();
                      }}
                    >
                      <LogIn size={15} /> Sign in again
                    </button>
                  ) : null}
                  <button
                    type="button"
                    className="icon-button danger-text"
                    onClick={() => {
                      onLogout(mode.account.id);
                      onClose();
                    }}
                  >
                    <LogOut size={15} /> Sign out
                  </button>
                </div>
              ) : null}
            </div>

            {saveError ? (
              <p className="login-error">
                <AlertCircle size={16} />
                <span>{saveError}</span>
              </p>
            ) : null}

            <div className="login-actions account-modal-actions">
              {isEdit ? (
                confirmRemove ? (
                  <span className="remove-confirm">
                    <span>Remove this account?</span>
                    <button
                      type="button"
                      className="danger-solid"
                      onClick={() => {
                        onRemove(mode.account.id);
                        onClose();
                      }}
                    >
                      <Trash2 size={14} /> Remove
                    </button>
                    <button
                      type="button"
                      className="secondary"
                      onClick={() => setConfirmRemove(false)}
                    >
                      Keep
                    </button>
                  </span>
                ) : (
                  <button
                    type="button"
                    className="icon-button danger-text"
                    onClick={() => setConfirmRemove(true)}
                  >
                    <Trash2 size={14} /> Remove account…
                  </button>
                )
              ) : null}
              <span className="account-modal-spacer" />
              <button type="button" className="secondary" onClick={onClose}>
                Cancel
              </button>
              <button className="primary" type="submit" disabled={busy}>
                {isEdit ? "Save" : "Add"}
              </button>
            </div>
          </form>
        ) : null}
      </div>
    </div>
  );
}
