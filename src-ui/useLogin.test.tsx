import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { __resetMockLogins } from "./api";
import type { AccountView } from "./types";
import { useLogin } from "./useLogin";

beforeEach(() => {
  __resetMockLogins();
});

afterEach(() => {
  __resetMockLogins();
  vi.restoreAllMocks();
});

function account(id: string): AccountView {
  return {
    id,
    provider: "codex",
    label: "Codex",
    enabled: true,
    autoDetected: false,
    credentialPath: null,
    endpointOverride: null,
    secretStorage: "keyring",
    hasSecret: false,
    email: "codex@example.com",
    configDir: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
}

function emit<T>(name: string, detail: T) {
  act(() => {
    window.dispatchEvent(new CustomEvent<T>(name, { detail }));
  });
}

test("tracks progress, ignores other ids, and completes", async () => {
  const onCompleted = vi.fn();
  const { result } = renderHook(() => useLogin({ onCompleted }));

  await act(async () => {
    await result.current.start("codex", "Codex");
  });
  // Stop the mock driver so we control the event sequence ourselves.
  __resetMockLogins();
  const id = result.current.session?.id;
  expect(id).toBeTruthy();

  // A progress event for a different login is ignored.
  emit("burnrate-login-progress", {
    id: "other",
    line: "nope",
    url: "https://no",
  });
  expect(result.current.session?.status).toBe("starting");

  // The matching progress event advances to "waiting" and surfaces the URL.
  emit("burnrate-login-progress", {
    id,
    line: "Authorize in your browser",
    url: "https://auth.example/x",
  });
  expect(result.current.session?.status).toBe("waiting");
  expect(result.current.session?.url).toBe("https://auth.example/x");
  expect(result.current.session?.needsCode).toBe(false);

  emit("burnrate-login-progress", {
    id,
    line: "Paste the code",
    needsCode: true,
  });
  expect(result.current.session?.needsCode).toBe(true);

  // A completion for another id does nothing.
  emit("burnrate-login-complete", { id: "other", account: account("other") });
  expect(onCompleted).not.toHaveBeenCalled();

  // The matching completion clears the session and reports the account.
  const completed = account(id as string);
  emit("burnrate-login-complete", { id, account: completed });
  await waitFor(() => expect(result.current.session).toBeNull());
  expect(onCompleted).toHaveBeenCalledWith(completed);
});

test("surfaces failures, ignores mismatched ids, and retries", async () => {
  const onCompleted = vi.fn();
  const { result } = renderHook(() => useLogin({ onCompleted }));

  await act(async () => {
    await result.current.start("codex", "Codex");
  });
  __resetMockLogins();
  const id = result.current.session?.id;

  emit("burnrate-login-failed", { id: "other", error: "ignored" });
  expect(result.current.session?.status).not.toBe("failed");

  emit("burnrate-login-failed", { id, error: "Sign-in timed out." });
  expect(result.current.session?.status).toBe("failed");
  expect(result.current.session?.error).toBe("Sign-in timed out.");

  // Retry starts a fresh session for the same provider/label.
  await act(async () => {
    result.current.retry();
  });
  __resetMockLogins();
  expect(result.current.session?.status).toBe("starting");
  expect(result.current.session?.id).not.toBe(id);
});

test("cancel clears the session", async () => {
  const { result } = renderHook(() => useLogin({ onCompleted: vi.fn() }));

  await act(async () => {
    await result.current.start("claude-code", "Claude Code");
  });
  __resetMockLogins();
  expect(result.current.session).not.toBeNull();

  await act(async () => {
    await result.current.cancel();
  });
  expect(result.current.session).toBeNull();

  // Cancelling again with no active session is a no-op.
  await act(async () => {
    await result.current.cancel();
  });
  expect(result.current.session).toBeNull();
});
