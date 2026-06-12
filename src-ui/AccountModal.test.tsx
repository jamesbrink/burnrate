import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { AccountModal } from "./AccountModal";
import type { AccountInput, AccountView } from "./types";

afterEach(() => cleanup());

function account(overrides: Partial<AccountView> = {}): AccountView {
  return {
    id: "acct-1",
    provider: "claude-code",
    label: "Work Claude",
    enabled: true,
    autoDetected: true,
    credentialPath: null,
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: false,
    email: "work@example.com",
    configDir: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    ...overrides,
  };
}

function renderModal(
  overrides: Partial<Parameters<typeof AccountModal>[0]> = {},
) {
  const props = {
    mode: { kind: "add" } as const,
    busy: false,
    canReauth: true,
    onSave: vi.fn().mockResolvedValue(undefined),
    onClose: vi.fn(),
    onStartLogin: vi.fn(),
    onLogout: vi.fn(),
    onRemove: vi.fn(),
    ...overrides,
  };
  render(<AccountModal {...props} />);
  return props;
}

test("browser sign-in path: provider grid, method step, hand-off to login", async () => {
  const user = userEvent.setup();
  const props = renderModal();

  expect(
    screen.getByRole("dialog", { name: "Add Account" }),
  ).toBeInTheDocument();
  await user.click(screen.getByRole("menuitem", { name: "Claude Code" }));
  await user.click(
    screen.getByRole("menuitem", { name: /Sign in with browser/ }),
  );

  expect(props.onStartLogin).toHaveBeenCalledWith("claude-code");
  expect(props.onClose).toHaveBeenCalledOnce();
});

test("manual CLI path keeps the optional key field and Back re-picks cleanly", async () => {
  const user = userEvent.setup();
  const props = renderModal();

  await user.click(screen.getByRole("menuitem", { name: "Claude Code" }));
  await user.click(
    screen.getByRole("menuitem", { name: /Enter a token manually/ }),
  );

  await user.type(screen.getByLabelText("API Key (optional)"), "sk-ant-test");

  // Back to the method step, back to the grid, pick another provider: the
  // typed secret must not survive the provider switch.
  await user.click(screen.getByTitle("Back"));
  await user.click(screen.getByTitle("Back"));
  await user.click(screen.getByRole("menuitem", { name: "OpenRouter" }));

  expect(screen.getByLabelText("API Key")).toHaveValue("");
  expect(props.onSave).not.toHaveBeenCalled();
});

test("OpenRouter skips the method step and saves the typed input", async () => {
  const user = userEvent.setup();
  const props = renderModal();

  await user.click(screen.getByRole("menuitem", { name: "OpenRouter" }));
  expect(screen.getByLabelText("Endpoint")).toHaveValue(
    "https://openrouter.ai/api/v1/credits",
  );

  await user.clear(screen.getByLabelText("Label"));
  await user.type(screen.getByLabelText("Label"), "OpenRouter Team");
  await user.type(screen.getByLabelText("API Key"), "sk-or-test");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(props.onSave).toHaveBeenCalledWith(
    expect.objectContaining({
      provider: "openrouter",
      label: "OpenRouter Team",
      secret: "sk-or-test",
    }) as AccountInput,
  );
  expect(props.onClose).toHaveBeenCalledOnce();
});

test("a failed save stays open with the error inline", async () => {
  const user = userEvent.setup();
  const props = renderModal({
    onSave: vi.fn().mockRejectedValue(new Error("keychain locked")),
  });

  await user.click(screen.getByRole("menuitem", { name: "OpenRouter" }));
  await user.type(screen.getByLabelText("API Key"), "sk-or-test");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText(/keychain locked/)).toBeInTheDocument();
  expect(props.onClose).not.toHaveBeenCalled();
});

test("AWS tile seeds profile fields and category presets", async () => {
  const user = userEvent.setup();
  renderModal();

  await user.click(screen.getByRole("menuitem", { name: "AWS" }));

  expect(screen.getByLabelText("AWS profile")).toHaveValue("");
  expect(screen.getByLabelText("Region")).toHaveValue("us-east-1");
  expect(screen.getByDisplayValue("Amazon Bedrock")).toBeInTheDocument();
  expect(screen.queryByLabelText("Endpoint")).not.toBeInTheDocument();
});

test("AWS category rows edit filters in place", async () => {
  const user = userEvent.setup();
  const onSave = vi.fn().mockResolvedValue(undefined);
  renderModal({ onSave });

  await user.click(screen.getByRole("menuitem", { name: "AWS" }));
  const bedrockRow = screen
    .getByDisplayValue("Bedrock")
    .closest(".aws-category-row") as HTMLElement;
  await user.selectOptions(
    within(bedrockRow).getByLabelText("Filter type"),
    "tag",
  );
  await user.clear(within(bedrockRow).getByLabelText("Filter key"));
  await user.type(within(bedrockRow).getByLabelText("Filter key"), "Team");
  await user.click(screen.getByRole("button", { name: "Add custom service" }));
  expect(screen.getByDisplayValue("Custom service")).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Add" }));
  const saved = onSave.mock.calls[0][0] as AccountInput;
  const categories = saved.awsCategories ?? [];
  expect(categories[1]?.filter).toMatchObject({
    kind: "tag",
    key: "Team",
  });
  expect(categories[categories.length - 1]).toMatchObject({
    label: "Custom service",
    enabled: false,
  });
});

test("Copilot tile shows the plan selector; custom plan reveals the limit", async () => {
  const user = userEvent.setup();
  const onSave = vi.fn().mockResolvedValue(undefined);
  renderModal({ onSave });

  await user.click(screen.getByRole("menuitem", { name: "GitHub Copilot" }));
  expect(screen.getByLabelText("Plan")).toHaveValue("");
  expect(screen.queryByLabelText("Endpoint")).not.toBeInTheDocument();

  await user.selectOptions(screen.getByLabelText("Plan"), "custom");
  await user.type(screen.getByLabelText("Monthly premium requests"), "2500");
  // Leaving custom clears the stale limit so the plan's own allowance applies.
  await user.selectOptions(screen.getByLabelText("Plan"), "enterprise");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(onSave.mock.calls[0][0]).toMatchObject({
    provider: "copilot",
    copilotPlan: "enterprise",
    copilotCustomLimit: null,
  });
});

test("edit mode opens on prefilled fields with the provider fixed", async () => {
  const user = userEvent.setup();
  const props = renderModal({
    mode: { kind: "edit", account: account() },
  });

  const dialog = screen.getByRole("dialog", { name: "Edit Work Claude" });
  expect(within(dialog).getByText("work@example.com")).toBeInTheDocument();
  expect(screen.getByLabelText("Label")).toHaveValue("Work Claude");
  expect(screen.getByLabelText("API Key (optional)")).toHaveAttribute(
    "placeholder",
    "Leave blank to keep existing",
  );
  // No provider picker in edit mode — switching provider is never valid.
  expect(
    screen.queryByRole("menuitem", { name: "OpenRouter" }),
  ).not.toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(props.onSave).toHaveBeenCalledWith(
    expect.objectContaining({ id: "acct-1", secret: "" }) as AccountInput,
  );
});

test("edit mode offers Sign in again only when a re-auth is legitimate", async () => {
  const user = userEvent.setup();
  const props = renderModal({
    mode: { kind: "edit", account: account() },
  });

  await user.click(screen.getByRole("button", { name: /Sign in again/ }));
  expect(props.onStartLogin).toHaveBeenCalledWith("claude-code", "acct-1");
  expect(props.onClose).toHaveBeenCalledOnce();

  cleanup();
  const denied = renderModal({
    mode: { kind: "edit", account: account() },
    canReauth: false,
  });
  expect(
    screen.queryByRole("button", { name: /Sign in again/ }),
  ).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Sign out/ }));
  expect(denied.onLogout).toHaveBeenCalledWith("acct-1");
});

test("removal requires an explicit confirmation", async () => {
  const user = userEvent.setup();
  const props = renderModal({
    mode: { kind: "edit", account: account() },
  });

  await user.click(screen.getByRole("button", { name: /Remove account/ }));
  expect(props.onRemove).not.toHaveBeenCalled();
  expect(screen.getByText("Remove this account?")).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Keep" }));
  expect(screen.queryByText("Remove this account?")).not.toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: /Remove account/ }));
  await user.click(screen.getByRole("button", { name: "Remove" }));
  expect(props.onRemove).toHaveBeenCalledWith("acct-1");
  expect(props.onClose).toHaveBeenCalled();
});

test("Escape and the overlay click both close the modal", async () => {
  const user = userEvent.setup();
  const props = renderModal();

  await user.keyboard("{Escape}");
  expect(props.onClose).toHaveBeenCalledTimes(1);

  await user.click(document.querySelector(".login-overlay") as HTMLElement);
  expect(props.onClose).toHaveBeenCalledTimes(2);
});
