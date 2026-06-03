import { LogIn, LogOut, Plus, RotateCcw } from "lucide-react";
import type { Dispatch, FormEvent, SetStateAction } from "react";
import { providerDefaultEndpoints, providerLabels } from "./constants";
import type { AccountInput, ProviderKind, SecretStorageMode } from "./types";

/** Providers authenticated through the CLI rather than a pasted API key. */
const CLI_PROVIDERS: ProviderKind[] = ["claude-code", "codex"];

/**
 * Add / edit form for an account. Browser sign-in is the primary path for
 * Claude Code / Codex (offered here in edit mode and via the Add menu); the
 * manual token fields remain available for every provider.
 */
export function AccountForm({
  form,
  setForm,
  activeId,
  busy,
  onSubmit,
  onReset,
  onStartLogin,
  onLogout,
}: {
  form: AccountInput;
  setForm: Dispatch<SetStateAction<AccountInput>>;
  activeId: string | null;
  busy: boolean;
  onSubmit: (event: FormEvent) => void;
  onReset: () => void;
  onStartLogin?: (provider: ProviderKind, accountId?: string) => void;
  onLogout?: (id: string) => void;
}) {
  const isCliProvider = CLI_PROVIDERS.includes(form.provider);
  const showSignInControls = Boolean(activeId) && isCliProvider;

  return (
    <form className="account-form prefs-form" onSubmit={onSubmit}>
      <div className="form-heading">
        <h2>{activeId ? "Edit Account" : "Add Account"}</h2>
        {activeId ? (
          <button
            type="button"
            className="icon-button subtle"
            title="Reset form"
            onClick={onReset}
          >
            <RotateCcw size={16} />
          </button>
        ) : null}
      </div>

      {showSignInControls ? (
        <div className="account-auth-actions">
          <button
            type="button"
            className="secondary"
            onClick={() => onStartLogin?.(form.provider, activeId ?? undefined)}
          >
            <LogIn size={15} /> Sign in again
          </button>
          {activeId ? (
            <button
              type="button"
              className="icon-button danger-text"
              onClick={() => onLogout?.(activeId)}
            >
              <LogOut size={15} /> Sign out
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="form-grid">
        <label>
          Provider
          <select
            value={form.provider}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                provider: event.target.value as ProviderKind,
                label: providerLabels[event.target.value as ProviderKind],
                endpointOverride:
                  providerDefaultEndpoints[
                    event.target.value as ProviderKind
                  ] ?? "",
              }))
            }
          >
            <option value="openrouter">OpenRouter</option>
            <option value="runpod">Runpod</option>
            <option value="claude-code">Claude Code</option>
            <option value="codex">Codex</option>
          </select>
        </label>

        <label>
          Label
          <input
            value={form.label}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                label: event.target.value,
              }))
            }
            required
          />
        </label>
      </div>

      <div className="segmented" role="group" aria-label="Secret storage">
        {(["keyring", "plaintext"] satisfies SecretStorageMode[]).map(
          (mode) => (
            <button
              key={mode}
              type="button"
              className={form.secretStorage === mode ? "active" : ""}
              onClick={() =>
                setForm((current) => ({
                  ...current,
                  secretStorage: mode,
                }))
              }
            >
              {mode === "keyring" ? "Keyring" : "Plaintext"}
            </button>
          ),
        )}
      </div>

      <div className="form-grid">
        <label>
          {isCliProvider ? "API Key (optional)" : "API Key"}
          <input
            type="password"
            value={form.secret ?? ""}
            placeholder={activeId ? "Leave blank to keep existing" : ""}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                secret: event.target.value,
              }))
            }
          />
        </label>

        <label>
          Endpoint
          <input
            value={form.endpointOverride ?? ""}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                endpointOverride: event.target.value,
              }))
            }
          />
        </label>
      </div>

      <label className="toggle">
        <input
          type="checkbox"
          checked={form.enabled}
          onChange={(event) =>
            setForm((current) => ({
              ...current,
              enabled: event.target.checked,
            }))
          }
        />
        Enabled
      </label>

      <button className="primary" type="submit" disabled={busy}>
        <Plus size={17} />
        {activeId ? "Save" : "Add"}
      </button>
    </form>
  );
}
