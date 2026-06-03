import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelAccountLogin,
  onLoginComplete,
  onLoginFailed,
  onLoginProgress,
  startAccountLogin,
  submitLoginCode,
} from "./api";
import type {
  AccountView,
  LoginComplete,
  LoginFailed,
  LoginProgress,
  ProviderKind,
} from "./types";

export type LoginStatus = "starting" | "waiting" | "failed";

export interface LoginSession {
  id: string;
  provider: ProviderKind;
  label: string;
  /** Existing account being re-authenticated in place, if any. */
  reauthId: string | null;
  status: LoginStatus;
  url: string | null;
  lines: string[];
  needsCode: boolean;
  error: string | null;
}

const MAX_LOG_LINES = 50;

type Terminal =
  | { kind: "complete"; payload: LoginComplete }
  | { kind: "failed"; payload: LoginFailed };

interface Buffered {
  progress: LoginProgress[];
  terminal?: Terminal;
}

/**
 * Owns interactive sign-in state and the three login event subscriptions, keeping
 * App.tsx lean. `onCompleted` fires with the resulting account so the caller can
 * refresh and select it.
 *
 * Events are matched by id; because the backend may emit before `start()` has set
 * the session (a fast failure can beat the IPC round-trip), unmatched events are
 * buffered per id and replayed once the session id is known.
 */
export function useLogin({
  onCompleted,
}: {
  onCompleted: (account: AccountView) => void | Promise<void>;
}) {
  const [session, setSession] = useState<LoginSession | null>(null);
  const sessionRef = useRef<LoginSession | null>(null);
  sessionRef.current = session;
  const onCompletedRef = useRef(onCompleted);
  onCompletedRef.current = onCompleted;
  const bufferRef = useRef<Map<string, Buffered>>(new Map());

  const applyProgress = useCallback((progress: LoginProgress) => {
    setSession((current) => {
      if (!current || current.id !== progress.id) {
        return current;
      }
      return {
        ...current,
        status: "waiting",
        url: progress.url ?? current.url,
        lines: [...current.lines, progress.line].slice(-MAX_LOG_LINES),
        needsCode: current.needsCode || Boolean(progress.needsCode),
      };
    });
  }, []);

  const applyComplete = useCallback((complete: LoginComplete) => {
    setSession(null);
    void onCompletedRef.current(complete.account);
  }, []);

  const applyFailed = useCallback((failed: LoginFailed) => {
    setSession((current) => {
      if (!current || current.id !== failed.id) {
        return current;
      }
      return { ...current, status: "failed", error: failed.error };
    });
  }, []);

  const bufferFor = useCallback((id: string): Buffered => {
    let entry = bufferRef.current.get(id);
    if (!entry) {
      entry = { progress: [] };
      bufferRef.current.set(id, entry);
    }
    return entry;
  }, []);

  const drainBuffer = useCallback(
    (id: string) => {
      const entry = bufferRef.current.get(id);
      if (!entry) {
        return;
      }
      bufferRef.current.delete(id);
      for (const progress of entry.progress) {
        applyProgress(progress);
      }
      if (entry.terminal?.kind === "complete") {
        applyComplete(entry.terminal.payload);
      } else if (entry.terminal?.kind === "failed") {
        applyFailed(entry.terminal.payload);
      }
    },
    [applyProgress, applyComplete, applyFailed],
  );

  useEffect(() => {
    let disposed = false;
    const cleanups: Array<() => void> = [];
    const track = (pending: Promise<() => void>) => {
      void pending.then((unlisten) => {
        if (disposed) {
          unlisten();
        } else {
          cleanups.push(unlisten);
        }
      });
    };

    track(
      onLoginProgress((progress) => {
        if (sessionRef.current?.id === progress.id) {
          applyProgress(progress);
        } else {
          bufferFor(progress.id).progress.push(progress);
        }
      }),
    );
    track(
      onLoginComplete((complete) => {
        if (sessionRef.current?.id === complete.id) {
          applyComplete(complete);
        } else {
          bufferFor(complete.id).terminal = {
            kind: "complete",
            payload: complete,
          };
        }
      }),
    );
    track(
      onLoginFailed((failed) => {
        if (sessionRef.current?.id === failed.id) {
          applyFailed(failed);
        } else {
          bufferFor(failed.id).terminal = { kind: "failed", payload: failed };
        }
      }),
    );

    return () => {
      disposed = true;
      for (const cleanup of cleanups) {
        cleanup();
      }
    };
  }, [applyProgress, applyComplete, applyFailed, bufferFor]);

  const start = useCallback(
    async (
      provider: ProviderKind,
      label: string,
      accountId?: string | null,
    ) => {
      try {
        const pending = await startAccountLogin(provider, label, accountId);
        setSession({
          id: pending.id,
          provider,
          label,
          reauthId: accountId ?? null,
          status: "starting",
          url: null,
          lines: [],
          needsCode: false,
          error: null,
        });
        // Replay any events that arrived before the session id was known.
        drainBuffer(pending.id);
      } catch (error) {
        setSession({
          id: `pending-${provider}`,
          provider,
          label,
          reauthId: accountId ?? null,
          status: "failed",
          url: null,
          lines: [],
          needsCode: false,
          error: String(error),
        });
      }
    },
    [drainBuffer],
  );

  const cancel = useCallback(async () => {
    const current = sessionRef.current;
    setSession(null);
    if (current && current.status !== "failed") {
      try {
        await cancelAccountLogin(current.id);
      } catch {
        // Best-effort: the modal is already closed.
      }
    }
  }, []);

  const retry = useCallback(() => {
    const current = sessionRef.current;
    if (current) {
      void start(current.provider, current.label, current.reauthId);
    }
  }, [start]);

  const submitCode = useCallback(async (code: string) => {
    const current = sessionRef.current;
    if (!current) {
      return;
    }
    await submitLoginCode(current.id, code);
  }, []);

  return { session, start, cancel, retry, submitCode };
}
