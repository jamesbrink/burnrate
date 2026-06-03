import { useCallback, useEffect, useRef, useState } from "react";
import {
  checkForUpdates,
  installUpdate,
  onCheckUpdateRequested,
  onUpdateProgress,
  updaterAvailable,
} from "./api";
import type { UpdateChannel } from "./types";

/** Background auto-update poll interval (30 min) — well under GitHub's
 *  anonymous API rate limits even with several Burnrate windows alive. */
const CHECK_INTERVAL_MS = 30 * 60 * 1000;

export interface UpdaterState {
  available: boolean;
  version: string | null;
  body: string | null;
  downloading: boolean;
  progress: number;
  error: string | null;
  dismissed: boolean;
  /** A check is in flight (drives the Settings spinner). */
  checking: boolean;
  /** At least one check has completed this session (drives "Up to date"). */
  hasChecked: boolean;
}

const INITIAL_STATE: UpdaterState = {
  available: false,
  version: null,
  body: null,
  downloading: false,
  progress: 0,
  error: null,
  dismissed: false,
  checking: false,
  hasChecked: false,
};

export interface UseUpdater {
  state: UpdaterState;
  /** Manual check; surfaces the banner if an update is found. */
  checkNow: () => Promise<void>;
  /** Download + install the pending update, then the backend restarts. */
  install: () => Promise<void>;
  /** Hide the banner until a newer version appears. */
  dismiss: () => void;
}

export interface UseUpdaterOptions {
  /** Test-only: force the background poll to run under jsdom/dev. */
  forcePoll?: boolean;
}

/**
 * Channel-aware updater state machine. Polls every 30 minutes (off in dev so
 * iteration doesn't ping GitHub), exposes a manual check wired to the tray's
 * "Check for Updates" entry, and tracks download progress for the banner.
 *
 * `updaterAvailable()` is false on dev / unsigned builds (no pubkey), so the
 * banner stays hidden there instead of erroring on every poll.
 */
export function useUpdater(
  channel: UpdateChannel,
  options: UseUpdaterOptions = {},
): UseUpdater {
  const [state, setState] = useState<UpdaterState>(() => ({ ...INITIAL_STATE }));
  const stateRef = useRef(state);
  stateRef.current = state;
  const lastDismissedVersion = useRef<string | null>(null);
  const checkInFlight = useRef<Promise<void> | null>(null);
  const channelRef = useRef(channel);
  channelRef.current = channel;

  const update = useCallback((patch: Partial<UpdaterState>) => {
    setState((prev) => ({ ...prev, ...patch }));
  }, []);

  const runCheck = useCallback(async () => {
    if (checkInFlight.current) return checkInFlight.current;
    const run = (async () => {
      if (!(await updaterAvailable().catch(() => false))) return;
      update({ checking: true });
      try {
        const info = await checkForUpdates(channelRef.current);
        if (info) {
          // Stay dismissed only if it's the same version the user dismissed.
          const dismissed =
            stateRef.current.dismissed &&
            lastDismissedVersion.current === info.version;
          update({
            available: true,
            version: info.version,
            body: info.body,
            error: null,
            dismissed,
            checking: false,
            hasChecked: true,
          });
        } else {
          update({
            available: false,
            version: null,
            body: null,
            error: null,
            checking: false,
            hasChecked: true,
          });
        }
      } catch (err) {
        update({ error: String(err), checking: false, hasChecked: true });
      }
    })();
    checkInFlight.current = run;
    try {
      await run;
    } finally {
      if (checkInFlight.current === run) checkInFlight.current = null;
    }
  }, [update]);

  const install = useCallback(async () => {
    if (stateRef.current.downloading) return;
    const version = stateRef.current.version;
    if (!version) return;
    update({ downloading: true, progress: 0, error: null });
    try {
      // Pass the version we're showing so the backend refuses to install a
      // different pending update if a later check swapped it.
      await installUpdate(version);
      // The backend restarts on success; reaching here means a silent failure.
    } catch (err) {
      update({ downloading: false, progress: 0, error: String(err) });
    }
  }, [update]);

  const dismiss = useCallback(() => {
    lastDismissedVersion.current = stateRef.current.version;
    update({ dismissed: true });
  }, [update]);

  // Track download progress.
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    let disposed = false;
    void onUpdateProgress((percent) => update({ progress: percent })).then(
      (unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      },
    );
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [update]);

  // Manual "Check for Updates" from the tray menu.
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    let disposed = false;
    void onCheckUpdateRequested(() => void runCheck()).then((unlisten) => {
      if (disposed) unlisten();
      else cleanup = unlisten;
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [runCheck]);

  // Background poll. Off in dev unless a test opts in; re-checks when the
  // channel changes so switching channels surfaces the right feed immediately.
  useEffect(() => {
    if (import.meta.env.DEV && !options.forcePoll) return;
    void runCheck();
    const id = window.setInterval(() => void runCheck(), CHECK_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [runCheck, channel, options.forcePoll]);

  return { state, checkNow: runCheck, install, dismiss };
}
