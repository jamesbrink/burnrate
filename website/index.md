---
layout: home

hero:
  name: Burnrate
  text: Know your burn before it knows you.
  tagline: >-
    A menu-bar monitor for Claude Code, Codex, OpenRouter, Runpod, and AWS —
    quotas, credits, spend, and subscription limits, all in one glance.
  image:
    src: /screenshots/tray.png
    alt: Burnrate menu-bar popover with live quota meters for each account
  actions:
    - theme: brand
      text: Get Started
      link: /guide/what-is-burnrate
    - theme: alt
      text: Download
      link: https://github.com/jamesbrink/burnrate/releases/latest

features:
  - icon: 🔥
    title: Five providers, one glance
    details:
      Claude Code, Codex, OpenRouter, Runpod, and AWS Cost Explorer side by
      side — remaining quota, credits, spend, and reset timers.
  - icon: 👥
    title: True multi-account
    details:
      Sign in extra Claude Code and Codex accounts from the app via browser
      OAuth. Each account is isolated in its own CLI config dir and labeled
      with its email.
  - icon: 📊
    title: Real subscription buckets
    details: The same 5-hour, weekly, and model-specific windows the providers
      enforce — plus Codex plan rate limits read straight from the app server.
  - icon: 🔐
    title: Keyring-first secrets
    details:
      API keys live in the OS keyring by default. Plaintext storage only when
      you explicitly opt in, per account.
  - icon: ⚡
    title: Native and lightweight
    details:
      Tauri 2 app with a translucent macOS popover that follows the system
      appearance, hides from the Dock, and refreshes quietly every five
      minutes.
  - icon: 🔄
    title: Automatic updates
    details:
      Signature-verified one-click updates on macOS, with selectable Stable
      and Nightly channels.
---

<span class="br-kicker">Every quota · One popover</span>

## Watch the burn, not the dashboards

No more tab-cycling through five provider consoles. Burnrate keeps every
account's meters in your menu bar and warns you before a window runs dry.

<div class="br-meters">
  <div class="br-meter">
    <span class="row"><span>claude · 5-hour</span><span class="val">70%</span></span>
    <span class="track"><span class="fill ok" style="--w: 70%; --d: 0.05s"></span></span>
  </div>
  <div class="br-meter">
    <span class="row"><span>codex · weekly</span><span class="val">100%</span></span>
    <span class="track"><span class="fill ok" style="--w: 100%; --d: 0.2s"></span></span>
  </div>
  <div class="br-meter">
    <span class="row"><span>openrouter · credits</span><span class="val">$58.60</span></span>
    <span class="track"><span class="fill warn" style="--w: 47%; --d: 0.35s"></span></span>
  </div>
  <div class="br-meter">
    <span class="row"><span>aws · month-to-date</span><span class="val">$86.20</span></span>
    <span class="track"><span class="fill hot" style="--w: 86%; --d: 0.5s"></span></span>
  </div>
</div>

<div class="br-shot">

![Burnrate Preferences window showing per-account usage across Claude Code, Codex, Runpod, and OpenRouter](/screenshots/preferences.png)

</div>

_The Preferences window: accounts on the left, live usage on the right._

<span class="br-kicker">Install</span>

## Up and burning in a minute

Grab a native bundle from
[GitHub Releases](https://github.com/jamesbrink/burnrate/releases/latest), or
build from the crate or flake:

::: code-group

```sh [cargo]
cargo install burnrate
```

```sh [nix]
nix run github:jamesbrink/burnrate
```

:::

See the [installation guide](/guide/installation) for signed macOS builds and
update channels.
