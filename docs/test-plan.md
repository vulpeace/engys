# Release test plan

Run the subscription review and credential checks first. Use a disposable Linux account or VM, a Windows VM snapshot, and a spare Android device. Use synthetic profiles, credentials, and traffic throughout. Record the client build, core version, OS, hardware, expected and actual results, and timestamps. Save redacted logs and configuration diffs for failures.

## 1. Subscription review and refresh

On desktop, serve a fixture from a temporary directory with `python -m http.server 8765 --bind 127.0.0.1`. Save the following as `profile.yaml`, import `http://127.0.0.1:8765/profile.yaml` as a subscription, activate it, and start the core with TUN off. Use an unused proxy port if 17890 is occupied.

```yaml
mixed-port: 17890
allow-lan: false
mode: rule
tun: {enable: false}
rules:
  - DOMAIN,old.example.invalid,DIRECT
  - MATCH,DIRECT
```

1. In **Rules**, add `DOMAIN,local.example.invalid,REJECT` before **MATCH** and click **Save & apply**. Change `old.example.invalid` to `new.example.invalid` in the served file. Run each manual refresh choice on a fresh copy of the original fixture and overlay. **Merge with subscription** should keep the local rule before **MATCH** and adopt the new source rule. **Override subscription** should keep the previous effective list until **Use subscription rules** is selected. **Use subscription rules** should adopt the new source list and clear edits to the rule list. Check that custom providers and Proxied Apps rules survive all three choices.
2. Repeat with a one-minute interval and **Close to tray** enabled. Hide the window before changing the served file. Use the server log to confirm a download within roughly 75 seconds of the last successful fetch. Check that the active runtime stays on the previous version. Reopen from the tray, choose **Review later**, and quit and relaunch. The same review should return. Leave it pending for two scan intervals past its next due time and confirm there is no additional fetch.
3. Repeat the background case with two subscriptions pending at once. Apply one and discard the other. Each choice should affect only its named profile. Discard should preserve the previous profile and resume scheduled downloads after the interval. A successful refresh of an inactive profile should leave the live runtime alone.
4. Serve invalid YAML, then valid YAML with a rule rejected by the core. Try both automatic and manual refresh. The previous saved profile and live routing should remain usable, and a failed attempt or failed review should be visible. Repeat with the core stopped, then start it and confirm that it uses the last successfully applied configuration.
5. With a fixture server that can delay responses, disable or change the interval, save a local YAML edit, or delete the profile while downloading. Confirm the outdated job cannot commit or recreate a deleted profile. Try saving an open Rules draft after a refresh and confirm the UI requires a reload. Duplicate a profile with custom groups, providers, and per-profile app rules. Edit the copy and verify the original routing stays intact.

## 2. Desktop lifecycle, routing, and credential checks

| Test | Actions and pass criteria |
| --- | --- |
| Start at login and tray | Follow the README sequence on both OSes. Include two quick launches, UAC denial on Windows, hidden startup with the core running and stopped, reopening from the tray, and one test-install upgrade and uninstall. Verify one GUI and at most one core, correct login registration, and no remaining child process after Quit. |
| Live routing and TUN | On Linux and Windows, overlap two app rules and test all four process match types with new connections. Change rule order and hot-reload. Verify the observed route in both ordering modes and switch between Global and This profile, including an empty profile list. Repeat TUN permission denial, grant and stop. Compare routes and DNS before and after Stop, Quit, and a core crash. |
| Controller boundary | On a private test network, confirm the controller listens on loopback only and rejects missing or incorrect credentials for REST and WebSocket. Verify a separate LAN host cannot reach the controller. Test both Allow LAN settings so the proxy listener behavior can be verified independently. After a reload that changes controller settings, verify the current endpoint stays usable until restart. |
| Local credential storage | With a fresh Linux account using umask `022`, add synthetic credentials, save twice, and inspect file and directory modes for settings, profile index/YAML, runtime YAML, and a pending download. Repeat after setting a fixture file to `0600`. Pass when the second local account cannot read the fixture secrets before or after a save. Check user-only access on Windows with a second standard account. |
| Subscription link consent | Open a synthetic import link from another application on a cold and warm launch, including when the window is hidden. Cancel and verify no request reached the fixture server. Confirm an import and verify the nested URL, name, and explicit zero interval. Check logs for URL tokens and controller secrets. |
| Update recovery | Use the README recovery sequence on test installations. Verify **Restore previous version** and **Keep update** with a previously running core, a stopped core and active profile, and an external binary path. Confirm the previous Linux binary retains capabilities after restoring it. Capture which binary and core state persist after relaunch. |

## 3. Android device checks

1. On a physical arm64 device, deny and grant VPN consent, deny notification and battery permissions, and revoke VPN access while connected. Test proxy-only mode separately. Check that the intended VPN state, notification, and error are visible after reopening the activity.
2. Verify TCP, UDP, DNS, and IPv6 against controlled endpoints. Apply include/exclude rules, including missing packages and a shared-UID case if available. Check actual routing from an included and excluded app. Exercise Wi-Fi/mobile handover, screen-off/Doze, and rapid connect/disconnect. Repeat under always-on VPN and Android's block-without-VPN setting if supported by the test device.
3. Dismiss the activity, reopen it, and check that the VPN session is still usable. Exercise system process termination, always-on restoration, and an explicit disconnect. Verify disconnect and consent revocation clear the previously requested running state. Record service and UI process IDs plus before/after connectivity.
4. On a test device or emulator with another fixture app, occupy the preferred controller port, then start the VPN. Check that the active port advances, that reload reuses the selected port, and that the UI connects to the expected core. Repeat the subscription consent and review flow on Android with a fixture endpoint reachable from that device.

## 4. Performance and soak

Use release builds on the same hardware for both the previous release and the candidate. Repeat cold launch and core startup ten times. Record p50 and p95 time to a usable Dashboard, time to controller readiness, and latency of a successful reload. Record app and core memory separately. On Android, record both UI and VPN processes and use PSS when available.

| Workload | What to measure |
| --- | --- |
| Rules | Paste 1,000, 10,000, and 50,000 equivalent rules as plain text and YAML. Time paste to preview, search response, and **Save & apply**. Look for UI tasks over 50 ms where profiling is available. |
| Connection churn | Use a controlled traffic source for 100 and 1,000 concurrent connections, then try 10,000 if the host supports it. Keep approximately 10% churn per one-second interval. Check scroll/search responsiveness, CPU, memory, and the default history limit of 2,000. Active connections should stay visible above the limit, with old closed entries evicted as load falls. |
| Soak and data path | Run 60 minutes with traffic and periodic refreshes, including 30 minutes hidden or screen-off. Measure memory and idle CPU in five-minute windows, and compare throughput, latency, and packet loss with the prior release using the same endpoints. On desktop, also measure the same core and profile running directly to isolate GUI overhead. On Android, record battery usage and reconnect time. |

Treat crashes, persistent memory growth under steady load, incorrect routing, leaked credentials, and failed rollback as failures. Investigate a reproducible p95 regression greater than 20% on the same hardware. The timing threshold is a proposed triage trigger.

## Coverage to add in CI

- Run the Node and Rust suites in CI alongside the existing frontend checks and Go tests. Require the mihomo parser fixtures to run against a pinned binary in at least one job. Add platform package smoke tests and scan the pnpm, Rust, and Go dependencies.
- Drive refresh with a local HTTP fixture and a controllable core adapter. Assert the four-download cap, per-profile exclusion, stale revision handling, pending-review persistence, and transactional rollback when validation succeeds but live application fails. Include a failed rollback and confirm both failures are visible.
- Add regression coverage for credential file modes across rewrites, bounded subscription response sizes, and updater artifact integrity checks before execution. Exercise approved local subscription and controller endpoints in packaged-app tests.
- Keep fixed synthetic rule and connection workloads for repeatable measurements. Measure the webview and IPC separately from parser and store functions.
