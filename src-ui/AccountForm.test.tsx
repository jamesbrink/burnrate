import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState, type FormEvent } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { AccountForm } from "./AccountForm";
import { cloneDefaultAwsCategories, emptyForm } from "./constants";
import type { AccountInput } from "./types";

afterEach(() => cleanup());

function Harness({ initial }: { initial: AccountInput }) {
  const [form, setForm] = useState<AccountInput>(initial);
  return (
    <>
      <AccountForm
        form={form}
        setForm={setForm}
        activeId={null}
        busy={false}
        onSubmit={(event) => event.preventDefault()}
        onReset={vi.fn()}
      />
      <output data-testid="form-state">{JSON.stringify(form)}</output>
    </>
  );
}

function formState(): AccountInput {
  return JSON.parse(screen.getByTestId("form-state").textContent ?? "{}");
}

test("switching to AWS seeds profile-based fields and category presets", async () => {
  const user = userEvent.setup();
  render(<Harness initial={{ ...emptyForm }} />);

  await user.selectOptions(screen.getByLabelText("Provider"), "aws");

  expect(screen.getByLabelText("AWS profile")).toHaveValue("");
  expect(screen.getByLabelText("Region")).toHaveValue("us-east-1");
  expect(screen.getByDisplayValue("Amazon Bedrock")).toBeInTheDocument();
  expect(formState().awsCategories).toHaveLength(4);
});

test("AWS fields edit budget and category filters", async () => {
  const user = userEvent.setup();
  render(
    <Harness
      initial={{
        ...emptyForm,
        provider: "aws",
        label: "AWS",
        endpointOverride: "",
        secret: "",
        awsProfile: null,
        awsRegion: "us-east-1",
        awsMonthlyBudgetUsd: null,
        awsCategories: cloneDefaultAwsCategories(),
      }}
    />,
  );

  await user.type(screen.getByLabelText("AWS profile"), "work");
  await user.clear(screen.getByLabelText("Region"));
  await user.type(screen.getByLabelText("Region"), "us-west-2");
  await user.type(
    screen.getByLabelText("Monthly budget (USD, optional)"),
    "125",
  );

  const bedrockRow = screen
    .getByDisplayValue("Bedrock")
    .closest(".aws-category-row");
  expect(bedrockRow).toBeTruthy();
  await user.click(within(bedrockRow as HTMLElement).getByLabelText("Enabled"));
  await user.clear(within(bedrockRow as HTMLElement).getByLabelText("Label"));
  await user.type(
    within(bedrockRow as HTMLElement).getByLabelText("Label"),
    "Bedrock tokens",
  );
  await user.selectOptions(
    within(bedrockRow as HTMLElement).getByLabelText("Filter type"),
    "tag",
  );
  await user.clear(
    within(bedrockRow as HTMLElement).getByLabelText("Filter key"),
  );
  await user.type(
    within(bedrockRow as HTMLElement).getByLabelText("Filter key"),
    "Team",
  );
  await user.clear(
    within(bedrockRow as HTMLElement).getByLabelText("Filter value"),
  );
  await user.type(
    within(bedrockRow as HTMLElement).getByLabelText("Filter value"),
    "ai",
  );

  expect(formState()).toMatchObject({
    awsProfile: "work",
    awsRegion: "us-west-2",
    awsMonthlyBudgetUsd: 125,
  });
  expect(formState().awsCategories?.[1]).toMatchObject({
    label: "Bedrock tokens",
    enabled: false,
    filter: { kind: "tag", key: "Team", values: ["ai"] },
  });

  await user.clear(screen.getByLabelText("Monthly budget (USD, optional)"));
  expect(formState().awsMonthlyBudgetUsd).toBeNull();
});

test("custom AWS categories start disabled until configured", async () => {
  const user = userEvent.setup();
  render(
    <Harness
      initial={{
        ...emptyForm,
        provider: "aws",
        label: "AWS",
        awsCategories: cloneDefaultAwsCategories(),
      }}
    />,
  );

  await user.click(screen.getByRole("button", { name: "Add custom service" }));

  const customRow = screen
    .getByDisplayValue("Custom service")
    .closest(".aws-category-row") as HTMLElement;
  expect(within(customRow).getByLabelText("Enabled")).not.toBeChecked();

  await user.selectOptions(
    within(customRow).getByLabelText("Filter type"),
    "costCategory",
  );
  await user.clear(within(customRow).getByLabelText("Filter key"));
  await user.type(
    within(customRow).getByLabelText("Filter key"),
    "BusinessUnit",
  );
  await user.type(within(customRow).getByLabelText("Filter value"), "Platform");

  const categories = formState().awsCategories ?? [];
  const custom = categories[categories.length - 1];
  expect(custom).toMatchObject({
    enabled: false,
    filter: { kind: "costCategory", key: "BusinessUnit", values: ["Platform"] },
  });
});

test("hides Sign in again when the backend would reject a re-auth", () => {
  const props = {
    form: { ...emptyForm, provider: "claude-code" as const },
    setForm: vi.fn(),
    activeId: "claude-code-manual",
    busy: false,
    onSubmit: (event: FormEvent) => event.preventDefault(),
    onReset: vi.fn(),
    onStartLogin: vi.fn(),
    onLogout: vi.fn(),
  };
  const { rerender } = render(<AccountForm {...props} />);
  expect(
    screen.getByRole("button", { name: /sign in again/i }),
  ).toBeInTheDocument();

  // A manual-token CLI account (no isolated dir, not auto-detected) fails the
  // backend guard, so the affordance is withheld; Sign out stays available.
  rerender(<AccountForm {...props} canReauth={false} />);
  expect(
    screen.queryByRole("button", { name: /sign in again/i }),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /sign out/i })).toBeInTheDocument();
});
