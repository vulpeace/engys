# engys

Desktop GUI by vulpeace for the [mihomo](../mihomo) proxy core. Tauri v2 (Rust) + SvelteKit (Svelte 5, SPA) + Bits UI.

[GPL-3.0 license](LICENSE) · [Source repository](https://github.com/vulpeace/engys)

## Run

```sh
pnpm install
pnpm tauri dev        # dev app with hot reload
pnpm build            # release bundle (versioned AppImage on Linux)
pnpm build:frontend   # frontend assets only
pnpm check            # type-check frontend
cd src-tauri && cargo test
```

The app version comes from `package.json`. On Linux, `pnpm build` packages
`src-tauri/target/release/bundle/appimage/engys_<version>_amd64.AppImage`
(`amd64` for x86-64 builds); Windows keeps the configured installer targets.
Tauri runs `build:frontend` before compiling the desktop app. Its Linux packaging
hook embeds `X-AppImage-Version` from `package.json` in the desktop entry so
AppImage properties show the version too, including with `pnpm tauri build`.

Pushing a Git tag runs the Android and Linux build workflows. Download the debug APKs and the
x86-64 AppImage from the workflow runs' artifacts. The signed Android release workflow runs on demand.

The core binary is resolved in this order: **Settings → Core → mihomo binary** (explicit path) → the
**app-managed core** in `<data>/core-bin/` → a `mihomo` file next to the app executable (bundled sidecar) →
`mihomo` on `PATH`.

Settings → Core → Check / Download fetches the official release for this OS/CPU from
`github.com/MetaCubeX/mihomo` (through the running core's proxy if it's up). With **Download without proxy**
off, the desktop runtime config routes the engys process through Mihomo's **GLOBAL** selector in Rule mode.
Turning it on removes that generated rule and disables HTTP proxy use for the archive transfer.
The updater checks the archive's
size and GitHub-provided SHA-256 digest before extraction, then checks that the binary runs and reports the
expected version before swapping it in (stopping/restarting the core if needed). On Linux a
replaced binary loses its TUN capabilities. Starting or restarting it interactively with TUN
enabled prompts for authorization when those capabilities are missing.

> Linux + NVIDIA/Wayland: if the window is blank, run with `WEBKIT_DISABLE_DMABUF_RENDERER=1`.

## How it works

- The core runs as a **child process** (`mihomo -d <workdir> -f runtime.yaml`), never in-process.
- `runtime.yaml` = the active profile + explicit client overrides. Unedited network/mode/TUN fields
  inherit profile values; controller access stays on loopback with the resolved port and secret. It is validated with `mihomo -t` before every start/reload.
  Stored profiles are never modified.
- Switching profiles / saving settings while running rewrites `runtime.yaml` and hot-reloads via
  `PUT /configs?force=true` (no restart). Controller port/secret/binary changes need a restart.
- The UI talks to the core's REST + WebSocket API directly (`src/lib/api/mihomo.ts`). Rust handles only
  what a webview can't: process lifecycle, files, subscription downloads, process listing.
- mihomo only reports *open* connections; the Requests history is accumulated client-side
  (`src/lib/stores/live.svelte.ts`) up to the configured limit.

### Dashboard memory

The Memory card shows **Core** (mihomo's reported resident memory) and **Client** separately. Client
sums the application's OS process tree, including child WebView processes, while excluding the running
core and its descendants. It updates every two seconds while the Dashboard is visible, and remains
available when the core is stopped. Values are resident-memory estimates (RSS / Windows working set);
shared pages may be counted in multiple processes, so this is not a unique/private-memory total.

### Resetting subscription overrides

Profile-backed fields follow the active subscription/config until edited. An individual **Reset to
subscription** button (undo arrow) appears beside each overridden field. It removes that field's client
override rather than copying a default into it, so later subscription updates control the value too.
Fields absent from the profile use normal application/core defaults. Other overrides remain untouched.

This applies to mixed port, Allow LAN, IPv6, log level, mode, controller port/secret, and all exposed
TUN options (including fallback DNS servers). Settings-page changes and resets apply with **Save**.
The Dashboard's TUN toggle and Proxies mode selector also have reset buttons, applied immediately.
TUN permission is still checked when enabling or inheriting an enabled TUN. Controller changes need a
core restart; live reloads keep the current controller reachable. Local preferences (theme, startup,
tray behavior, binary path, latency-test preferences, history limit and download User-Agent) have no
subscription counterpart and do not show subscription-reset controls.

The subscription refresh-interval field also has a reset button after a user edit. Resetting it explicitly
follows the provider's `profile-update-interval` hint (converted from hours to minutes), including future
hint changes. A missing/zero hint disables automation. New subscriptions still start with automation
disabled unless an interval is chosen. A client-set **0** stays disabled until explicitly reset.
Proxied Apps' **Use profile default** button similarly releases its per-profile ordering override.

Override ownership is persisted independently of the current inherited values, so background subscription
updates cannot make an old form accidentally pin stale settings. Older settings did not record ownership:
identifiable nondefault values migrate as overrides; unchanged defaults now inherit. Existing generated
secrets are kept as the local fallback. Reset controls are shown for migrated overrides too.

### Automatic subscription refresh

Each subscription has an **Update interval (minutes)** field, both in Add subscription and on its
Profiles card. Click the card's **Save** button to change its interval. **0 disables automation** and
is the default for new and existing subscriptions. The server's `profile-update-interval` header is
kept as provider metadata and does not override client choices. Only an explicit reset to the
subscription interval lets later provider hints control the timer.

Refresh runs in Rust while the app is open, including hidden/tray launches and when the core is
stopped. Due subscriptions are checked every 15 seconds, with at most four downloads at once and
one refresh per subscription. Scheduling timestamps survive restarts: an overdue subscription gets
one refresh, rather than repeated catch-up requests. Success schedules the next refresh from the
last successful fetch; failure waits until the configured interval after the last attempt. Manual
refresh remains available when automation is disabled and resets the schedule after success.

Subscription downloads are limited to 8 MiB. They must be mapping YAML and are validated with the installed
mihomo parser when available.
Refreshing the active profile reloads a running core, retaining app-routing rules and their inferred
order; refreshing an inactive profile leaves the running core alone. Invalid downloads keep the old
profile. Live reload failures restore the previous profile/runtime config and report rollback failures.
Local edits made during a download are preserved instead of being silently overwritten. Disabling or
changing an interval during a download cancels its pending automatic commit. Later scheduled refreshes
still replace local subscription edits, as the editor indicates.

Profiles shows the last successful fetch and refresh errors without opening dialogs for background
failures. Proxied Apps refreshes its available targets and inferred order after updates. Automation
stops when the application exits; it does not install a separate service or OS refresh job.

Manual validation: set a test subscription to 1 minute, change its served YAML, and confirm a refresh
within the next interval plus the scheduler scan delay; repeat with the window hidden and the core
stopped. Set 0 and verify requests stop, restart the app to check persistence, and return invalid YAML
or an unavailable server to check that the old profile remains usable and failures are visible.

### Selecting a proxy before startup

While the core is stopped, open **Proxies** and choose a member under **Startup proxy selections**.
For ungrouped proxies, switch to **Global** mode and choose a member of **GLOBAL**. Choices are saved
per profile and applied when its controller becomes ready, then Mihomo remembers them as regular
group selections. Members resolved through providers or include-all filters become available after
startup. If a profile change makes a pending choice invalid, update it or use **Clear selections**
before trying again.

### Custom proxy groups

On **Proxies**, use **Add group** to create a Select, URL-test, Fallback, or Load-balance group
alongside the profile's existing groups. Choose ordered proxy/group members, named proxy providers,
or all profile proxies/providers, with an optional name regex filter. Automatic groups expose a test
interval in seconds; Load-balance also offers a strategy. Custom groups have Edit and Delete controls.

Groups are saved per profile in `<config>/custom-groups.json` and appended to the generated runtime
config, leaving the subscription file unchanged. They survive restarts and subscription re-fetches.
Adding a group does not change routing rules: choose it as a target in **Proxied Apps**, or use it through
a parent group. Existing explicit GLOBAL membership stays unchanged.

Saving validates the complete config with the installed mihomo core and reloads a running core;
with the core stopped, groups apply on its next start. Name collisions, missing members/providers,
dependency cycles, and invalid regexes are rejected. Referencing rules/groups must be updated before
deleting or renaming a group. Subscription updates that invalidate custom groups retain the previous
working configuration and report the error.

### Rules and rule-providers

The **Rules** tab lists the active profile's inherited rules in order, with search and pagination.
Use the insertion buttons beside any row, or **Add rule** and its position field, to insert custom
rules anywhere in the list. **Paste rules** accepts one rule per line or a copied YAML rules list,
preserving order and duplicates while skipping blank lines and full-line comments. Choose the insertion
position before adding the batch to the draft. Move rows with the up/down arrows or the editor's
position field; positions refer to the complete list even while searching or paging. Definitions
use mihomo syntax, including logical rules, `no-resolve`, and `RULE-SET,ProviderName,Target`.
Rules after a catch-all such as `MATCH` are usually unreachable.

The rule dialog offers a **Constructor** with rule-type and routing-target dropdowns, match-value
inputs, a provider picker for `RULE-SET`, and a `no-resolve` toggle for applicable types. Its preview
shows the resulting rule. Switch to **Inline rule** to paste or edit raw syntax. Supported existing
rules prefill the constructor; advanced expressions and parameters stay inline without being rewritten.

Inherited rules are locked by default. **Allow editing inherited rules** unlocks their definitions
and deletion, and allows reordering them. **Reset inherited rules** restores the current
subscription's rules and order while keeping custom insertions. Save inherited edits before locking again;
the tab starts locked when reopened. Existing saved overrides remain effective while locked.

Inherited rule-providers are shown read-only. **Add provider** creates HTTP, file, or inline custom
providers with domain, IP-CIDR, or classical behavior. HTTP/file providers support YAML/text and,
for domain/IP-CIDR, MRS format. HTTP update intervals use seconds, with 0 disabling updates.
File/cache paths follow mihomo's working-directory and safe-path rules. The custom-provider YAML
editor also exposes advanced options, editing, and deletion. Add a `RULE-SET` rule to route through
a provider; defining one alone does not change routing. Custom names cannot replace inherited providers.

**Save & apply** validates the combined config using the installed core, then saves and hot-reloads
it when running, or applies on the next start when stopped. Failed validation leaves saved settings
unchanged; failed application attempts restore the previous saved and runtime config. Changes live
per profile in `<config>/routing.json`, without modifying subscription files. On re-fetch, inherited
edits/deletions match the original rule text and duplicate occurrence. Explicitly reordered inherited
rules keep their saved order; new subscription rules attach before their next surviving source anchor
or at the end. Changed source rules take the subscription's new value. Custom rules stay before their next surviving inherited anchor, or at the
end if no anchor remains. Unsaved drafts require a reload if their subscription/profile changes.
Proxied Apps rules are added separately, after this merge, using that tab's ordering toggle.

Use **Duplicate profile** on the Profiles tab to copy a profile, its saved rule edits and providers,
its custom proxy groups, and its per-profile Proxied Apps rules. The copy has its own profile ID.
When you edit the source YAML or manually refresh a subscription and its rule list changes,
profiles with saved rule edits offer three choices. **Merge with subscription** inserts added and
edited rules at their previous positions in the new list. **Override subscription** keeps the
previous effective list until you choose **Use subscription rules**. **Use subscription rules**
clears local changes to the rule list. Custom rule-providers and Proxied Apps rules are kept.
If an automatic refresh changes the source rule list for a profile with saved rule edits, the app
saves the download for review and keeps using the current configuration. Review opens when the GUI
is visible. **Review later** retains the download across restarts. Further automatic refreshes for
that profile wait until you apply or discard the pending update.

### Proxied apps

Add applications from running processes, browse to an executable, or use **Add manually** for an exact
process name/path or a name/path regex. Choose a proxy group (or a named proxy) from the active profile,
**Proxy (GLOBAL)**, **Direct**, or **Block** for each entry. Rules can be edited, removed, and reordered;
the first matching app rule wins. **Select through proxy** initially follows the active profile: a Direct
fallback turns it on, a proxy fallback turns it off. On puts app rules after profile rules but before the
first catch-all (`MATCH`); off puts them first. With no catch-all, on appends them. The catch-all itself is
preserved. Mixed/dynamic destinations cannot be inferred reliably and default to off. Manual overrides
are saved per profile; **Use profile default** restores automatic inference after subscription updates.
Earlier subscription rules can therefore take precedence when this toggle is on. New app selections default
to Proxy when on and Direct when off; existing per-app route choices are preserved. **Save & apply** validates the candidate using the installed core,
saves it, and reloads a running core. When stopped, rules apply on the next start. Existing connections
keep their original routing; use new connections to check changes.

Use the header dropdown to choose **Global** or **This profile**. Global is the shared list used by
default. The first time you choose This profile for a profile with no saved list, the draft starts
with a copy of the Global rules. **Save & apply** saves and activates the selected scope for the current
profile. Its own list replaces the shared list, including when saved empty. Switching back to Global
keeps the saved profile list available for later use. Deleting a profile removes its app list
and ordering override.

Both lists live in `<config>/app-rules.json` and survive app restarts. Nonempty lists are inserted into
the generated runtime config with `find-process-mode: always`. An empty list leaves the profile's rules
and process mode in effect. Validation errors leave the previous configuration intact. Failed live
application attempts restore the previous saved and runtime configuration and report any rollback failure.
Named targets in the Global list must exist in each profile that uses it. GLOBAL, DIRECT, and REJECT
are available across profiles. An explicit import button can bring old preview-only localStorage
selections into the draft for the selected scope.

App rules require **Rule mode**. In Global/Direct modes the page offers **Use Rule mode**. They route only
traffic already reaching mihomo: enable TUN to capture applications that don't use the HTTP/SOCKS proxy.
Process lookup also needs sufficient OS permissions, particularly for other users' applications.

Regexes use mihomo's case-insensitive regexp2 syntax (not JavaScript's). For example,
`^(firefox|chrome)(\.exe)?$` matches those process names, and `/opt/app[0-9]{1,3}/.*` matches paths.
Exact names/paths cannot contain commas in mihomo's rule format; use an anchored regex with `\x2c`
for a literal comma. Regexes with a comma next to spaces must also use `\x2c` to preserve those spaces.
The installed core is needed to validate nonempty rules, including while it is stopped.

Parser integration tests can run against a real installed binary without starting a proxy listener:

```sh
MIHOMO_TEST_BINARY=/usr/local/bin/mihomo cargo test --offline --manifest-path src-tauri/Cargo.toml
```

Manual checks: save all four match types, restart the app/core, and inspect generated/runtime rules;
check new connections in Rule mode with TUN; change a target and reorder overlapping rules; remove all
entries; reject an invalid regex without changing the last saved/live config; switch profiles and confirm
missing targets are reported. Windows routing and live reload still need platform testing.

### Start at login

Enable **Settings → Startup & tray → Start at login**. The change applies immediately, independently of
Settings Save/Revert. Login launches create the window hidden and expose an **Open** / **Quit** tray
menu with core status. Enable **Close to tray** under **Settings → Startup & tray** and click Save
to keep the core running after closing the window. On Linux, this also releases the WebView.
Opening the app from the tray or launching it again recreates the window and starts a fresh UI
session, resetting the in-memory Requests history. On Windows, the window is hidden and restored.
This option defaults to off. Choose **Quit** in the tray to exit and stop the core. With the option
off, closing the window exits the app normally.

The app remembers whether the core was running, and restores that state on the next launch (including
manual launches). Explicit Stop records stopped; exit cleanup and temporary stops for updates or
Windows elevation preserve the remembered state. A core crash records stopped. The active profile,
mode, TUN preference, other saved settings, proxy-group selections (`profile.store-selected` in
mihomo's cache), and window geometry are retained. Connections and traffic counters start a new session.
Operational state lives in `session.json`, separately from editable settings. Startup failures appear
in tray status and on the Dashboard, without opening a dialog or window at login.

- **Windows:** registers `engys-<user SID>` in Task Scheduler with an at-login trigger for that
  user, interactive logon, and highest privileges. Enabling/disabling asks for UAC when necessary;
  subsequent login launches need no prompt. This requires an administrator account. Supplying a
  different administrator account in UAC is rejected to avoid registering startup for the wrong user.
  The whole GUI runs elevated, as it does with the existing TUN permission flow. Manual launches use
  the registered task when necessary. Tasks have no execution time limit and run on battery power.
  NSIS/MSI uninstall removes this installation's tasks; updater-driven upgrades preserve them.
- **Linux:** writes `$XDG_CONFIG_HOME/autostart/top.vulpeace.engys.desktop` (default `~/.config/autostart/`).
  AppImage registration uses the persistent `APPIMAGE` path, not the temporary mounted executable.
  TUN still needs capabilities granted on the current core binary.

Enable startup from an installed release build at its permanent location. If you move the executable,
open it at the new location and click **Repair** in Startup. Refresh reads the actual OS registration,
including externally disabled entries. For a manually removed Linux installation, disable startup first
or remove the desktop entry above.

Native registration and a per-user process lock are implemented without extra Tauri startup plugins.
A token-protected loopback message can only reveal the existing window, including across Windows
integrity levels. Registration helpers run before the GUI and never start another core.

#### Manual validation on Windows and Linux

1. Enable startup; on Windows approve UAC. Check cancellation leaves the switch at the OS-reported value.
2. Select a profile and proxies, choose a mode, enable TUN with permission, and start the core. Quit.
3. Log out/in: no window should appear, the tray should report running, and the same proxy/TUN settings
   should be active. Open through the tray and through the normal application shortcut.
4. Stop the core, quit, and repeat login: the app should stay in the background with the core stopped.
5. Open twice and verify one GUI/core. Test Windows elevation handoff with login startup disabled too.
6. Test a missing core/profile or lost Linux capabilities: the app stays hidden and reports the failure
   when opened. Repair the problem and start manually.
7. Disable startup and confirm the OS entry disappears. Re-enable and uninstall a Windows package:
   its task should disappear. Updater-driven upgrades should retain registration.

### TUN mode

- Toggle on the Dashboard; options (stack, strict route, DNS hijack, …) in Settings → TUN. The app's TUN
  only override the profile's `tun:` block after you edit them; reset returns each field to the profile
  (extras like `route-exclude-address` are kept).
- TUN hijacks DNS into mihomo's DNS module, so if the profile has no enabled `dns:` section the app injects
  one (fake-ip, nameservers from Settings → TUN → Fallback DNS servers). Otherwise all lookups would fail.
- Linux defaults to `tun.dns-mode: disabled`. With `native`/`hijack` the core runs `resolvectl` as the
  user to point systemd-resolved at the TUN, and polkit asks for a password three times on every TUN start.
  It isn't needed: sing-tun's auto-route rules send *all* port-53 traffic (even to LAN resolvers) into the
  TUN, where `dns-hijack` answers it. Changeable in Settings → TUN → System DNS integration.
- Whether TUN is actually up is read back from the core (`GET /configs` → `tun.enable`); if it didn't come up
  the setting is rolled back and the core's error is shown.
- Permissions (`src-tauri/src/privilege.rs`):
  - **Linux**: "Grant permission" runs `pkexec setcap cap_net_admin,cap_net_raw,cap_net_bind_service+ep <core>`
    (one password prompt). The GUI and core keep running as your user. Capabilities vanish when the binary is
    replaced (e.g. updated) and must be granted again. Note: with caps set, *any* local user who can execute
    that binary gets those network capabilities through it.
  - **Windows**: "Restart as administrator" stops the core and relaunches the app elevated (UAC).
    Untested on real Windows so far (compile-checked only).
  - **macOS**: not supported yet.
- On stop and on app exit the core gets SIGTERM and up to 3 s to remove its routes before being killed.

Data locations (Linux): settings in `~/.config/top.vulpeace.engys/`, profiles and the core working dir
(GEO databases, cache, `runtime.yaml`) in `~/.local/share/top.vulpeace.engys/`.
Installations using the previous application identifier keep their data in their original directories.
Android treats `top.vulpeace.engys` as a separate app from the previous package.

## Layout

```
src-tauri/src/
  core.rs          start/stop/reload, runtime config, log capture, crash detection
  profiles.rs      profile store (index.json + <id>.yaml)
  subscription.rs  download, subscription-userinfo parsing, validation
  subscription_refresh.rs  background refresh scheduling and transactional live updates
  settings.rs      persisted settings
  login.rs         Linux desktop autostart / elevated Windows scheduled task
  session.rs       remembered core state and window geometry
  instance.rs      per-user instance lock and reveal-window signal
  desktop.rs       tray menu and window handling
  app_rules.rs     saved app routing, rule generation, validation and live application
  custom_groups.rs per-profile custom proxy groups, validation and live application
  routing.rs       inherited rule overlays, custom rule-providers and live application
  processes.rs     running-process list (Proxied apps)
  commands.rs      Tauri commands exposed to the UI
src/lib/
  ui/              DESIGN LAYER — tokens.css + components (Bits UI wrappers). Restyle here.
  components/      app-specific widgets: bandwidth chart (uPlot), virtual list, YAML editor, sidebar
  api/             backend.ts (Tauri commands), mihomo.ts (core REST/WS client)
  stores/          core.svelte.ts (process + settings), live.svelte.ts (traffic, memory, connections)
src/routes/        one folder per tab: / (dashboard), proxies, apps, rules, profiles, requests, settings
```

## Designing the UI

All colors, spacing, radii, fonts and the light/dark themes are CSS variables in `src/lib/ui/tokens.css`.
Components in `src/lib/ui/` use only those tokens; Bits UI parts are styled through `ui-*` classes and
Bits' `data-state` / `data-highlighted` attributes. Pages contain layout CSS (also token-based) only.

## Status

- Not yet: system-proxy toggle, macOS TUN, a Windows service for TUN without elevating the GUI,
  bundling the mihomo binary as a sidecar (`bundle.externalBin`).

### Core update recovery

Core updates keep the previous managed binary as a local backup. If the new core
fails configuration validation or startup, a native dialog offers **Restore previous
version** or **Keep update**, with the original error. Restoring also returns to the
previous external core path when applicable, and restarts the core only if it was
running before the update. A stopped core's active configuration is validated without
starting it; with no active profile, validation waits until a profile is configured.
On Linux, cancelling the authorization prompt or failing to grant TUN permissions during
restart also offers recovery. Renaming the previous managed binary preserves its file capabilities.

Manual checks on Windows and Linux: update a running core with a configuration the
new version rejects, accept restoration, and verify the previous version starts.
Repeat choosing Keep update; verify the failed version remains installed and stopped.
Repeat with the core stopped and an active profile, and with an external core path.

## Android

Android builds share this project and bundle the mihomo core. See [Android build and runtime documentation](docs/android.md) for setup, build commands and device validation.

### Subscription links

Open a link of the form `engys://import-sub?url=encodedUrl&refresh=int#name`
to review and download a subscription in engys. Use `encodeURIComponent`
on the full HTTP or HTTPS subscription URL and on the optional profile name.

- `url` is required.
- `refresh` is optional and sets the update interval in whole minutes. Use `0`
  to disable automatic refresh. Omit it to use the default interval.
- `#name` is optional. If omitted, the app uses the server's suggested name or
  the subscription host.

Example: `engys://import-sub?url=https%3A%2F%2Fexample.com%2Fconfig.yaml%3Fid%3D42&refresh=60#My%20Profile`

On Linux, launch the AppImage once to register the scheme if your desktop has
not already registered it.

## Credits

The Android Go bridge adapts VPN socket protection and app identification hooks
from [FlClash by chen08209 and contributors](https://github.com/chen08209/FlClash/blob/43a7af80bb0d3ce25c99efeca9868ee6abfed7c7/core/lib.go#L30-L153)
and listener shutdown code from [FlClashX by pluralplay and contributors](https://github.com/pluralplay/FlClashX/blob/99ae586c4c10396007878cd8e2b846c8f8f88286/core/lifecycle.go).
engys modified these sections for its Android host interface and core
lifecycle on 2026-11-19. The adapted code is distributed under the
[GNU General Public License, version 3](LICENSE).

Android also embeds the [Mihomo core by MetaCubeX and contributors](https://github.com/MetaCubeX/mihomo).
See [Android distribution](docs/android.md#distribution) for redistribution details.
