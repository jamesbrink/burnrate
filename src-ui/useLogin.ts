import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelAccountLogin,
  onLoginComplete,
  onLoginFailed,
  onLoginProgress,
  startAccountLogin,
} from "./api";
import type { AccountView, ProviderKind } from "./types";

export type LoginStatus = "starting" | "waiting" | "failed";

export interface LoginSession {
  id: string;
  provider: ProviderKind;
  label: string;
  status: LoginStatus;
  url: string | null;
  lines: string[];
  error: string | null;
}

const MAX_LOG_LINES = 50;

/**
 * Owns interactive sign-in state and the three login event subscriptions, keeping
 * App.tsx lean. `onCompleted` fires with the resulting account so the caller can
 * refresh and select it.
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
        setSession((current) => {
          if (!current || current.id !== progress.id) {
            return current;
          }
          return {
            ...current,
            status: "waiting",
            url: progress.url ?? current.url,
            lines: [...current.lines, progress.line].slice(-MAX_LOG_LINES),
          };
        });
      }),
    );
    track(
      onLoginComplete((complete) => {
        if (sessionRef.current?.id !== complete.id) {
          return;
        }
        setSession(null);
        void onCompletedRef.current(complete.account);
      }),
    );
    track(
      onLoginFailed((failed) => {
        setSession((current) => {
          if (!current || current.id !== failed.id) {
            return current;
          }
          return { ...current, status: "failed", error: failed.error };
        });
      }),
    );

    return () => {
      disposed = true;
      for (const cleanup of cleanups) {
        cleanup();
      }
    };
  }, []);

  const start = useCallback(async (provider: ProviderKind, label: string) => {
    try {
      const pending = await startAccountLogin(provider, label);
      setSession({
        id: pending.id,
        provider,
        label,
        status: "starting",
        url: null,
        lines: [],
        error: null,
      });
    } catch (error) {
      setSession({
        id: `pending-${provider}`,
        provider,
        label,
        status: "failed",
        url: null,
        lines: [],
        error: String(error),
      });
    }
  }, []);

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
      void start(current.provider, current.label);
    }
  }, [start]);

  return { session, start, cancel, retry };
}
