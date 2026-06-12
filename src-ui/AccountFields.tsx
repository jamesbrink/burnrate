import type { Dispatch, SetStateAction } from "react";
import {
  AWS_DEFAULT_REGION,
  CLI_PROVIDERS,
  cloneDefaultAwsCategories,
  COPILOT_PLANS,
} from "./constants";
import type {
  AccountInput,
  AwsCategoryConfig,
  AwsCostFilter,
  CopilotPlan,
  SecretStorageMode,
} from "./types";

/**
 * Provider-specific account fields, shared by the Add wizard and the Edit
 * sheet (`AccountModal`). The provider itself is chosen before this renders —
 * in the wizard's provider step, or fixed for an existing account — so there
 * is no provider selector here.
 */
export function AccountFields({
  form,
  setForm,
  isEdit,
}: {
  form: AccountInput;
  setForm: Dispatch<SetStateAction<AccountInput>>;
  isEdit: boolean;
}) {
  const isCliProvider = CLI_PROVIDERS.includes(form.provider);
  const isAwsProvider = form.provider === "aws";
  const isCopilotProvider = form.provider === "copilot";

  return (
    <>
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

      {isAwsProvider ? (
        <AwsFields form={form} setForm={setForm} />
      ) : isCopilotProvider ? (
        <CopilotFields form={form} setForm={setForm} isEdit={isEdit} />
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
                placeholder={isEdit ? "Leave blank to keep existing" : ""}
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
    </>
  );
}

function CopilotFields({
  form,
  setForm,
  isEdit,
}: {
  form: AccountInput;
  setForm: Dispatch<SetStateAction<AccountInput>>;
  isEdit: boolean;
}) {
  const plan = form.copilotPlan ?? "";

  return (
    <div className="copilot-fields">
      <p className="form-help">
        Without a token, usage is estimated from Copilot CLI session logs on
        this Mac — IDE and web usage is not counted. A GitHub token (classic)
        fetches your exact premium-request usage from GitHub billing.
      </p>
      <div className="form-grid">
        <label>
          Plan
          <select
            value={plan}
            onChange={(event) => {
              const next = (event.target.value || null) as CopilotPlan | null;
              setForm((current) => ({
                ...current,
                copilotPlan: next,
                copilotCustomLimit:
                  next === "custom" ? current.copilotCustomLimit : null,
              }));
            }}
          >
            <option value="">No plan (usage only)</option>
            {COPILOT_PLANS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>

        <label>
          GitHub token (optional)
          <input
            type="password"
            value={form.secret ?? ""}
            placeholder={isEdit ? "Leave blank to keep existing" : "ghp_…"}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                secret: event.target.value,
              }))
            }
          />
        </label>
      </div>

      {plan === "custom" ? (
        <label>
          Monthly premium requests
          <input
            type="number"
            min="1"
            step="1"
            value={form.copilotCustomLimit ?? ""}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                copilotCustomLimit: event.target.value
                  ? Number(event.target.value)
                  : null,
              }))
            }
          />
        </label>
      ) : null}
    </div>
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
