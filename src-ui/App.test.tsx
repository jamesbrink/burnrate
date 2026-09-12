import {
  cleanup,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { App } from "./App";

afterEach(() => cleanup());

test("renders provider rows and snapshot states", async () => {
  render(<App />);

  expect((await screen.findAllByText("Claude Code")).length).toBeGreaterThan(0);
  // The insights panel can hydrate before the dashboard snapshots — wait for
  // the usage rows' status badge instead of assuming render order.
  expect((await screen.findAllByText("Warning")).length).toBeGreaterThan(0);
  expect(screen.getAllByText("Codex").length).toBeGreaterThan(0);
  expect(screen.getAllByText("5-hour").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Weekly").length).toBeGreaterThan(0);
  expect(screen.getAllByText("OpenRouter").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Runpod").length).toBeGreaterThan(0);
  expect(screen.getAllByText("AWS").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Nous Portal").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Total available").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Subscription credits").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Purchased credits").length).toBeGreaterThan(0);
  expect(screen.getByText(/^AWS cost data · just now$/)).toBeInTheDocument();
  expect(screen.getByText("Bedrock")).toBeInTheDocument();
  expect(screen.getByText("EC2 compute")).toBeInTheDocument();
  expect(screen.getByText("Current burn")).toBeInTheDocument();
  expect(screen.queryByText("Unknown plan")).not.toBeInTheDocument();
});

test("adds a manual account through the modal wizard", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.click(screen.getByTitle("Add account"));
  await user.click(screen.getByRole("menuitem", { name: "OpenRouter" }));
  expect(screen.getByLabelText("Endpoint")).toHaveValue(
    "https://openrouter.ai/api/v1/credits",
  );
  await user.clear(screen.getByLabelText("Label"));
  await user.type(screen.getByLabelText("Label"), "OpenRouter Team");
  await user.type(screen.getByLabelText("API Key"), "sk-test");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("OpenRouter Team")).toBeInTheDocument();
  // The modal closes after a successful save.
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("adds an AWS account with profile and category presets", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.click(screen.getByTitle("Add account"));
  await user.click(screen.getByRole("menuitem", { name: "AWS" }));
  // The AWS category rows each have a Label field too; index 0 is the
  // account label.
  expect(screen.getAllByLabelText("Label")[0]).toHaveValue("AWS");
  expect(screen.getByLabelText("AWS profile")).toHaveValue("");
  expect(screen.getByLabelText("Region")).toHaveValue("us-east-1");
  expect(screen.getByDisplayValue("Amazon Bedrock")).toBeInTheDocument();
  expect(
    screen.getByDisplayValue("Amazon Elastic Compute Cloud - Compute"),
  ).toBeInTheDocument();
  expect(
    screen.getByText(
      /Estimated minimum: 1 billable Cost Explorer request per daily refresh/,
    ),
  ).toBeInTheDocument();

  await user.clear(screen.getAllByLabelText("Label")[0]);
  await user.type(screen.getAllByLabelText("Label")[0], "AWS Team");
  await user.type(screen.getByLabelText("AWS profile"), "work");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("AWS Team")).toBeInTheDocument();
});

test("adds a Codex account manually with plaintext storage and disabled state", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.click(screen.getByTitle("Add account"));
  await user.click(screen.getByRole("menuitem", { name: "Codex" }));
  await user.click(
    screen.getByRole("menuitem", { name: /Enter a token manually/ }),
  );
  expect(screen.getByLabelText("Label")).toHaveValue("Codex");

  const dialog = screen.getByRole("dialog");
  await user.click(screen.getByRole("button", { name: "Plaintext" }));
  await user.type(screen.getByLabelText("Endpoint"), "http://localhost:8787");
  await user.click(within(dialog).getByLabelText("Enabled"));
  await user.clear(screen.getByLabelText("Label"));
  await user.type(screen.getByLabelText("Label"), "Codex Spare");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("Codex Spare")).toBeInTheDocument();
});

test("opens the edit modal from an account row and cancels", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  const accountButton = screen
    .getAllByRole("button")
    .find((button) => button.textContent?.includes("Claude Code"));
  expect(accountButton).toBeTruthy();

  await user.click(accountButton!);
  expect(
    screen.getByRole("dialog", { name: /Edit Claude Code/ }),
  ).toBeInTheDocument();
  expect(screen.getByLabelText("Label")).toHaveValue("Claude Code");

  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("removing an account asks for confirmation first", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.click(screen.getByTitle("Add account"));
  await user.click(screen.getByRole("menuitem", { name: "Runpod" }));
  await user.clear(screen.getByLabelText("Label"));
  await user.type(screen.getByLabelText("Label"), "Remove Me");
  await user.type(screen.getByLabelText("API Key"), "rp-test");
  await user.click(screen.getByRole("button", { name: "Add" }));
  expect(await screen.findByText("Remove Me")).toBeInTheDocument();

  const removeButtons = screen.getAllByTitle("Remove account");
  await user.click(removeButtons[removeButtons.length - 1]);
  // Still present: the trash click only arms the confirmation.
  expect(screen.getByText("Remove Me")).toBeInTheDocument();

  await user.click(screen.getByTitle("Confirm removing Remove Me"));
  expect(screen.queryByText("Remove Me")).not.toBeInTheDocument();
});

test("toggles an account from the sidebar switch", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  const offCount = () =>
    screen
      .getAllByRole("switch")
      .filter((node) => node.getAttribute("aria-checked") === "false").length;
  const before = offCount();
  const target = screen
    .getAllByRole("switch")
    .find((node) => node.getAttribute("aria-checked") === "true");
  expect(target).toBeTruthy();

  await user.click(target!);
  await waitFor(() => expect(offCount()).toBe(before + 1));
});

test("runs detect and refresh actions", async () => {
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "Accounts" });
  await user.click(screen.getByTitle("Detect accounts"));
  await user.click(screen.getByTitle("Refresh"));

  expect(await screen.findByText("Burnrate: 2 warning")).toBeInTheDocument();
});

for (const view of ["preferences", "tray"]) {
  test(`Nous shows only the subscription meter in ${view}`, async () => {
    window.history.replaceState({}, "", view === "tray" ? "/?view=tray" : "/");
    try {
      render(<App />);
      await screen.findAllByText("Purchased credits");
      if (view === "tray") {
        expect(screen.queryByText("Total available")).not.toBeInTheDocument();
        const card = screen.getByText("Purchased credits").closest("article")!;
        await userEvent.click(within(card).getByTitle("Show details"));
        expect(within(card).getByText("Total available")).toBeVisible();
        expect(
          within(card).getByText("Subscription rollover (reported)"),
        ).toBeVisible();
      } else {
        const details = screen.getByText("Credit details").closest("details")!;
        expect(details).not.toHaveAttribute("open");
        await userEvent.click(screen.getByText("Credit details"));
        expect(details).toHaveAttribute("open");
        expect(
          within(details).getByText("Subscription rollover (reported)"),
        ).toBeVisible();
      }
      expect(
        screen.queryByLabelText("Total available remaining"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByLabelText("Purchased credits remaining"),
      ).not.toBeInTheDocument();
      expect(
        screen.getByLabelText("Subscription credits remaining"),
      ).toBeInTheDocument();
      expect(screen.getByText("$0 / $110 USD")).toBeInTheDocument();
    } finally {
      cleanup();
      window.history.replaceState({}, "", "/");
    }
  });
}
