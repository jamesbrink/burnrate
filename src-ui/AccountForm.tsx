import { LogIn, LogOut, Plus, RotateCcw } from "lucide-react";
import type { Dispatch, FormEvent, SetStateAction } from "react";
import {
  AWS_DEFAULT_REGION,
  cloneDefaultAwsCategories,
  providerDefaultEndpoints,
  providerLabels,
} from "./constants";
import type {
  AccountInput,
  AwsCategoryConfig,
  AwsCostFilter,
  ProviderKind,
  SecretStorageMode,
} from "./types";

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
  const isAwsProvider = form.provider === "aws";
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
            onChange={(event) => {
              const provider = event.target.value as ProviderKind;
              setForm((current) => ({
                ...current,
                provider,
                label: providerLabels[provider],
                endpointOverride: providerDefaultEndpoints[provider] ?? "",
                secret: provider === "aws" ? "" : current.secret,
                awsRegion:
                  provider === "aws"
                    ? current.awsRegion || AWS_DEFAULT_REGION
                    : current.awsRegion,
                awsCategories:
                  provider === "aws" && !current.awsCategories?.length
                    ? cloneDefaultAwsCategories()
                    : current.awsCategories,
              }));
            }}
          >
            <option value="openrouter">OpenRouter</option>
            <option value="runpod">Runpod</option>
            <option value="aws">AWS</option>
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

      {isAwsProvider ? (
        <AwsFields form={form} setForm={setForm} />
      ) : (
        <>
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
        </>
      )}

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

function AwsFields({
  form,
  setForm,
}: {
  form: AccountInput;
  setForm: Dispatch<SetStateAction<AccountInput>>;
}) {
  const categories = form.awsCategories?.length
    ? form.awsCategories
    : cloneDefaultAwsCategories();

  function updateCategory(
    index: number,
    updater: (category: AwsCategoryConfig) => AwsCategoryConfig,
  ) {
    setForm((current) => {
      const currentCategories = current.awsCategories?.length
        ? current.awsCategories
        : cloneDefaultAwsCategories();
      return {
        ...current,
        awsCategories: currentCategories.map((category, i) =>
          i === index ? updater(category) : category,
        ),
      };
    });
  }

  function addCustomService() {
    setForm((current) => ({
      ...current,
      awsCategories: [
        ...(current.awsCategories?.length
          ? current.awsCategories
          : cloneDefaultAwsCategories()),
        {
          id: `custom-${Date.now()}`,
          label: "Custom service",
          enabled: false,
          filter: { kind: "dimension", key: "SERVICE", values: [""] },
          groupBy: null,
        },
      ],
    }));
  }

  return (
    <div className="aws-fields">
      <p className="form-help">
        AWS uses the SDK default credential chain (AWS_PROFILE, shared config,
        SSO, environment, or instance role). Burnrate does not store AWS access
        keys.
      </p>
      <div className="form-grid">
        <label>
          AWS profile
          <input
            value={form.awsProfile ?? ""}
            placeholder="default / AWS_PROFILE"
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                awsProfile: event.target.value,
              }))
            }
          />
        </label>
        <label>
          Region
          <input
            value={form.awsRegion ?? AWS_DEFAULT_REGION}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                awsRegion: event.target.value,
              }))
            }
          />
        </label>
      </div>
      <label>
        Monthly budget (USD, optional)
        <input
          type="number"
          min="0"
          step="0.01"
          value={form.awsMonthlyBudgetUsd ?? ""}
          onChange={(event) =>
            setForm((current) => ({
              ...current,
              awsMonthlyBudgetUsd: event.target.value
                ? Number(event.target.value)
                : null,
            }))
          }
        />
      </label>

      <div className="aws-category-list">
        <div className="aws-category-heading">
          <strong>AWS categories</strong>
          <button
            type="button"
            className="secondary"
            onClick={addCustomService}
          >
            Add custom service
          </button>
        </div>
        {categories.map((category, index) => (
          <AwsCategoryRow
            key={category.id}
            category={category}
            onChange={(next) => updateCategory(index, () => next)}
          />
        ))}
      </div>
    </div>
  );
}

function AwsCategoryRow({
  category,
  onChange,
}: {
  category: AwsCategoryConfig;
  onChange: (category: AwsCategoryConfig) => void;
}) {
  const filter = category.filter;
  const value = filter.values[0] ?? "";

  function updateFilter(next: Partial<AwsCostFilter> & { value?: string }) {
    const kind = next.kind ?? filter.kind;
    const key = next.key ?? filter.key;
    const values = next.value !== undefined ? [next.value] : filter.values;
    onChange({
      ...category,
      filter:
        kind === "tag"
          ? { kind, key, values }
          : kind === "costCategory"
            ? { kind, key, values }
            : { kind: "dimension", key, values },
    });
  }

  return (
    <div className="aws-category-row">
      <label className="toggle compact">
        <input
          type="checkbox"
          checked={category.enabled}
          onChange={(event) =>
            onChange({ ...category, enabled: event.target.checked })
          }
        />
        Enabled
      </label>
      <div className="form-grid">
        <label>
          Label
          <input
            value={category.label}
            onChange={(event) =>
              onChange({ ...category, label: event.target.value })
            }
          />
        </label>
        <label>
          Filter type
          <select
            value={filter.kind}
            onChange={(event) =>
              updateFilter({
                kind: event.target.value as AwsCostFilter["kind"],
              })
            }
          >
            <option value="dimension">Dimension</option>
            <option value="tag">Tag</option>
            <option value="costCategory">Cost category</option>
          </select>
        </label>
      </div>
      <div className="form-grid">
        <label>
          Filter key
          <input
            value={filter.key}
            placeholder="SERVICE, TAG key, or cost category name"
            onChange={(event) => updateFilter({ key: event.target.value })}
          />
        </label>
        <label>
          Filter value
          <input
            value={value}
            placeholder="Amazon Bedrock"
            onChange={(event) => updateFilter({ value: event.target.value })}
          />
        </label>
      </div>
    </div>
  );
}
