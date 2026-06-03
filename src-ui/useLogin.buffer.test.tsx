import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import * as api from "./api";
import type { AccountView, LoginComplete } from "./types";
import { useLogin } from "./useLogin";

vi.mock("./api");

afterEach(() => vi.restoreAllMocks());

function stubLoginListeners() {
  vi.mocked(api.onLoginProgress).mockResolvedValue(() => {});
  vi.mocked(api.onLoginFailed).mockResolvedValue(() => {});
  vi.mocked(api.onLoginComplete).mockResolvedValue(() => {});
}

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

// Reproduces the race Codex flagged: the backend emits a login event before the
// IPC `start_account_login` response (and thus the session id) is known. The
// event must be buffered and replayed once the session is established.
test("buffers a completion that arrives before the session id, then replays it", async () => {
  let completeHandler: ((complete: LoginComplete) => void) | undefined;
  vi.mocked(api.onLoginProgress).mockResolvedValue(() => {});
  vi.mocked(api.onLoginFailed).mockResolvedValue(() => {});
  vi.mocked(api.onLoginComplete).mockImplementation(async (handler) => {
    completeHandler = handler as (complete: LoginComplete) => void;
    return () => {};
  });

  const completed = account("codex-race");
  vi.mocked(api.startAccountLogin).mockImplementation(async () => {
    // Fire the completion *before* resolving — i.e. before useLogin has set the
    // session — so it can only be handled via the buffer.
    completeHandler?.({ id: "codex-race", account: completed });
    return completed;
  });

  const onCompleted = vi.fn();
  const { result } = renderHook(() => useLogin({ onCompleted }));

  await act(async () => {
    await result.current.start("codex", "Codex");
  });

  expect(onCompleted).toHaveBeenCalledWith(completed);
  expect(result.current.session).toBeNull();
});

test("forwards submitted auth codes only when a login session is active", async () => {
  stubLoginListeners();
  vi.mocked(api.startAccountLogin).mockResolvedValue(account("codex-submit"));
  vi.mocked(api.submitLoginCode).mockResolvedValue(undefined);
  const { result } = renderHook(() => useLogin({ onCompleted: vi.fn() }));

  await act(async () => {
    await result.current.submitCode("ignored-code#state");
  });
  expect(api.submitLoginCode).not.toHaveBeenCalled();

  await act(async () => {
    await result.current.start("codex", "Codex");
  });
  await act(async () => {
    await result.current.submitCode("real-code#state");
  });

  expect(api.submitLoginCode).toHaveBeenCalledWith(
    "codex-submit",
    "real-code#state",
  );
});
