import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { App } from "./App";

afterEach(() => cleanup());

test("renders provider rows and snapshot states", async () => {
  render(<App />);

  expect(await screen.findByText("Claude Code")).toBeInTheDocument();
  expect(screen.getAllByText("Codex").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Warning").length).toBeGreaterThan(0);
  expect(screen.getAllByText("5-hour").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Weekly").length).toBeGreaterThan(0);
  expect(screen.getAllByText("OpenRouter").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Runpod").length).toBeGreaterThan(0);
  expect(screen.getByText("Current burn")).toBeInTheDocument();
  expect(screen.queryByText("Unknown plan")).not.toBeInTheDocument();
});

test("adds a manual account in browser fallback mode", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  expect(screen.getByLabelText("Endpoint")).toHaveValue(
    "https://openrouter.ai/api/v1/credits",
  );
  await user.clear(screen.getByLabelText("Label"));
  await user.type(screen.getByLabelText("Label"), "OpenRouter Team");
  await user.type(screen.getByLabelText("API Key"), "sk-test");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("OpenRouter Team")).toBeInTheDocument();
});

test("adds a Runpod account with the default REST endpoint", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.selectOptions(screen.getByLabelText("Provider"), "runpod");
  expect(screen.getByLabelText("Label")).toHaveValue("Runpod");
  expect(screen.getByLabelText("Endpoint")).toHaveValue(
    "https://rest.runpod.io/v1",
  );
  await user.clear(screen.getByLabelText("Label"));
  await user.type(screen.getByLabelText("Label"), "Runpod Team");
  await user.type(screen.getByLabelText("API Key"), "rp-test");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("Runpod Team")).toBeInTheDocument();
});

test("edits and resets an existing account", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  const accountButton = screen
    .getAllByRole("button")
    .find((button) => button.textContent?.includes("Claude Code"));
  expect(accountButton).toBeTruthy();

  await user.click(accountButton!);
  expect(
    screen.getByRole("heading", { name: "Edit Account" }),
  ).toBeInTheDocument();
  expect(screen.getByLabelText("Label")).toHaveValue("Claude Code");

  await user.click(screen.getByTitle("Reset form"));
  expect(
    screen.getByRole("heading", { name: "Add Account" }),
  ).toBeInTheDocument();
});

test("updates provider, storage, endpoint, enabled state, and removes an account", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.selectOptions(screen.getByLabelText("Provider"), "codex");
  expect(screen.getByLabelText("Label")).toHaveValue("Codex");

  await user.click(screen.getByRole("button", { name: "Plaintext" }));
  await user.type(screen.getByLabelText("Endpoint"), "http://localhost:8787");
  await user.click(screen.getAllByLabelText("Enabled")[0]);
  await user.clear(screen.getByLabelText("Label"));
  await user.type(screen.getByLabelText("Label"), "Remove Me");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("Remove Me")).toBeInTheDocument();

  const removeButtons = screen.getAllByTitle("Remove account");
  await user.click(removeButtons[removeButtons.length - 1]);

  expect(screen.queryByText("Remove Me")).not.toBeInTheDocument();
});

test("runs detect and refresh actions", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.click(screen.getByTitle("Detect accounts"));
  await user.click(screen.getByTitle("Refresh"));

  expect(await screen.findByText("Burnrate: 1 warning")).toBeInTheDocument();
});
