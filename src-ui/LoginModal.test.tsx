import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { LoginModal } from "./LoginModal";
import type { LoginSession } from "./useLogin";

afterEach(() => cleanup());

function session(overrides: Partial<LoginSession> = {}): LoginSession {
  return {
    id: "codex-1",
    provider: "codex",
    label: "Codex",
    reauthId: null,
    status: "waiting",
    url: null,
    lines: [],
    needsCode: false,
    error: null,
    ...overrides,
  };
}

test("surfaces the auth URL and a cancel control during sign-in", async () => {
  const user = userEvent.setup();
  const onCancel = vi.fn();
  render(
    <LoginModal
      session={session({ url: "https://auth.example/x", lines: ["Waiting…"] })}
      onCancel={onCancel}
      onRetry={vi.fn()}
      onSubmitCode={vi.fn()}
    />,
  );

  expect(
    screen.getByRole("link", { name: /open the sign-in page/i }),
  ).toHaveAttribute("href", "https://auth.example/x");
  expect(screen.getByText("Waiting…")).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onCancel).toHaveBeenCalledOnce();
});

test("shows the error and a retry control on failure", async () => {
  const user = userEvent.setup();
  const onRetry = vi.fn();
  render(
    <LoginModal
      session={session({ status: "failed", error: "Sign-in timed out." })}
      onCancel={vi.fn()}
      onRetry={onRetry}
      onSubmitCode={vi.fn()}
    />,
  );

  expect(screen.getByText("Sign-in timed out.")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Cancel" }),
  ).not.toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(onRetry).toHaveBeenCalledOnce();
});

test("submits a pasted Claude authentication code", async () => {
  const user = userEvent.setup();
  const onSubmitCode = vi.fn().mockResolvedValue(undefined);
  render(
    <LoginModal
      session={session({
        provider: "claude-code",
        url: "https://auth.example/manual",
        needsCode: true,
        lines: [
          "Copy the authentication code from the browser, then paste it here.",
        ],
      })}
      onCancel={vi.fn()}
      onRetry={vi.fn()}
      onSubmitCode={onSubmitCode}
    />,
  );

  expect(
    screen.getByRole("link", { name: "Open the code page" }),
  ).toHaveAttribute("href", "https://auth.example/manual");

  await user.type(
    screen.getByLabelText("Paste the full authentication code"),
    "auth-code#state",
  );
  await user.click(screen.getByRole("button", { name: "Submit code" }));

  expect(onSubmitCode).toHaveBeenCalledWith("auth-code#state");
});

test("validates and reports authentication code submission errors", async () => {
  const user = userEvent.setup();
  const onSubmitCode = vi.fn().mockRejectedValue(new Error("Code expired."));
  render(
    <LoginModal
      session={session({
        provider: "claude-code",
        needsCode: true,
        lines: ["Paste the code"],
      })}
      onCancel={vi.fn()}
      onRetry={vi.fn()}
      onSubmitCode={onSubmitCode}
    />,
  );

  await user.click(screen.getByRole("button", { name: "Submit code" }));
  expect(
    screen.getByText("Paste the authentication code first."),
  ).toBeInTheDocument();
  expect(onSubmitCode).not.toHaveBeenCalled();

  await user.type(
    screen.getByLabelText("Paste the full authentication code"),
    " expired-code#state ",
  );
  await user.click(screen.getByRole("button", { name: "Submit code" }));

  expect(onSubmitCode).toHaveBeenCalledWith("expired-code#state");
  expect(screen.getByText("Error: Code expired.")).toBeInTheDocument();
});
