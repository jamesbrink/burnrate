import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { AddAccountMenu } from "./AddAccountMenu";

afterEach(() => cleanup());

test("AWS skips straight to the profile-based manual form", async () => {
  const user = userEvent.setup();
  const onManual = vi.fn();
  const onStartLogin = vi.fn();
  const onClose = vi.fn();
  render(
    <AddAccountMenu
      onManual={onManual}
      onStartLogin={onStartLogin}
      onClose={onClose}
    />,
  );

  await user.click(screen.getByRole("menuitem", { name: /AWS/ }));

  expect(onManual).toHaveBeenCalledWith("aws");
  expect(onStartLogin).not.toHaveBeenCalled();
  expect(onClose).toHaveBeenCalledOnce();
});

test("OpenRouter skips straight to the manual form", async () => {
  const user = userEvent.setup();
  const onManual = vi.fn();
  const onStartLogin = vi.fn();
  const onClose = vi.fn();
  render(
    <AddAccountMenu
      onManual={onManual}
      onStartLogin={onStartLogin}
      onClose={onClose}
    />,
  );

  await user.click(screen.getByRole("menuitem", { name: /OpenRouter/ }));

  expect(onManual).toHaveBeenCalledWith("openrouter");
  expect(onStartLogin).not.toHaveBeenCalled();
  expect(onClose).toHaveBeenCalledOnce();
});

test("Claude Code offers browser sign-in then triggers login", async () => {
  const user = userEvent.setup();
  const onManual = vi.fn();
  const onStartLogin = vi.fn();
  const onClose = vi.fn();
  render(
    <AddAccountMenu
      onManual={onManual}
      onStartLogin={onStartLogin}
      onClose={onClose}
    />,
  );

  await user.click(screen.getByRole("menuitem", { name: /Claude Code/ }));
  await user.click(
    screen.getByRole("menuitem", { name: /Sign in with browser/ }),
  );

  expect(onStartLogin).toHaveBeenCalledWith("claude-code");
  expect(onClose).toHaveBeenCalledOnce();
});
