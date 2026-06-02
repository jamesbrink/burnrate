import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { App } from "./App";

afterEach(() => cleanup());

test("renders provider rows and snapshot states", async () => {
  render(<App />);

  expect(await screen.findByText("Claude Code")).toBeInTheDocument();
  expect(screen.getAllByText("Codex").length).toBeGreaterThan(0);
  expect(screen.getByText("Warning")).toBeInTheDocument();
  expect(screen.getAllByText("OpenRouter").length).toBeGreaterThan(0);
});

test("adds a manual account in browser fallback mode", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.clear(screen.getByLabelText("Label"));
  await user.type(screen.getByLabelText("Label"), "OpenRouter Team");
  await user.type(screen.getByLabelText("API Key"), "sk-test");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("OpenRouter Team")).toBeInTheDocument();
});
