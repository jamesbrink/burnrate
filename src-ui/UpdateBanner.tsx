import { AlertCircle, Download, RefreshCw, X } from "lucide-react";
import type { UpdateChannel } from "./types";
import type { UpdaterState } from "./useUpdater";

const channelLabels: Record<UpdateChannel, string> = {
  stable: "Burnrate",
  nightly: "Burnrate Nightly",
};

/**
 * Dismissible update banner shown atop Preferences. Renders the error,
 * downloading (progress), or available states. Hidden entirely when no update
 * is available or the user dismissed the current one.
 */
export function UpdateBanner({
  state,
  channel,
  onInstall,
  onDismiss,
  onRetry,
}: {
  state: UpdaterState;
  channel: UpdateChannel;
  onInstall: () => void;
  onDismiss: () => void;
  onRetry: () => void;
}) {
  if (!state.available || state.dismissed) {
    return null;
  }

  const product = channelLabels[channel];

  if (state.error) {
    return (
      <div className="update-banner error" role="alert">
        <AlertCircle size={16} />
        <div className="update-banner-body">
          <strong>Update failed</strong>
          <span>{state.error}</span>
        </div>
        <div className="update-banner-actions">
          <button className="secondary" onClick={onRetry}>
            <RefreshCw size={14} />
            Try again
          </button>
          <button
            className="icon-button subtle"
            onClick={onDismiss}
            title="Dismiss"
          >
            <X size={16} />
          </button>
        </div>
      </div>
    );
  }

  if (state.downloading) {
    return (
      <div className="update-banner downloading" role="status">
        <Download size={16} />
        <div className="update-banner-body">
          <strong>Downloading update…</strong>
          <div className="update-progress" aria-label="Download progress">
            <span style={{ width: `${state.progress}%` }} />
          </div>
        </div>
        <span className="update-progress-pct">{state.progress}%</span>
      </div>
    );
  }

  return (
    <div className="update-banner available" role="status">
      <Download size={16} />
      <div className="update-banner-body">
        <strong>
          {product} {state.version} is available
        </strong>
        {state.body ? <span>{state.body}</span> : null}
      </div>
      <div className="update-banner-actions">
        <button className="primary" onClick={onInstall}>
          Install &amp; Restart
        </button>
        <button
          className="icon-button subtle"
          onClick={onDismiss}
          title="Dismiss"
        >
          <X size={16} />
        </button>
      </div>
    </div>
  );
}
