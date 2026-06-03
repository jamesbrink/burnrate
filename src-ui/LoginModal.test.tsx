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
    />,
  );

  expect(screen.getByText("Sign-in timed out.")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Cancel" }),
  ).not.toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(onRetry).toHaveBeenCalledOnce();
});
