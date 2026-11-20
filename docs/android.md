# Android

Android is a target of the existing Tauri project. The Svelte frontend and Rust configuration/profile logic are shared with desktop. Android 10 (API 29) or newer is required. Supported ABIs are arm64-v8a and x86_64.

## Build

Install Bash, Python 3, unzip, Node 22+, pnpm 10, Rust, JDK 17, Go 1.26.11 or newer, the Android SDK command-line tools, SDK platform 36, build-tools 35.0.0, and NDK 28.2.13676358. Set `ANDROID_HOME` to the SDK and `NDK_HOME` to its `ndk/28.2.13676358` directory. Go, Java and pnpm must be on PATH.

```sh
rustup target add aarch64-linux-android x86_64-linux-android
pnpm install --frozen-lockfile
pnpm android:build --debug --apk --ci
# One physical-device ABI:
pnpm tauri android build --debug --target aarch64 --apk --ci
# Development on a connected device or emulator:
pnpm android:dev
```

The Android Gradle project is already initialized and tracked in `src-tauri/gen/android`; do not reinitialize it during normal builds. It includes the SDK/toolchain configuration and network-security policy. Native service code lives in `src-tauri/plugins/android-vpn/android`.

If this checkout built the previous Android package, remove the stale generated Kotlin directory at `src-tauri/gen/android/app/src/main/java/dev/mihomo/client/generated` before building engys. Tauri now generates those sources under `top/vulpeace/engys/generated`.

Gradle builds `core-mobile` using pinned gomobile tools and mihomo dependencies, then packages the Java bindings and native libraries together. It follows Tauri's selected ABI list; `-Pmihomo.core.targets=android/arm64` can override Go targets when invoking Gradle directly. Go sources, checksums, and ABI selection are build inputs.

APKs are written under `src-tauri/gen/android/app/build/outputs/apk`. Debug APKs use Android's development signing key.

## Distribution

The workflow run identifies the GitHub commit used to build its APK or App Bundle.
When sharing a binary outside the workflow run, link to the exact build commit
and make the matching Corresponding Source available at no charge. The Go
dependency versions, including the embedded Mihomo core, are pinned in
`core-mobile/go.mod`. See [GPLv3 section 6](https://www.gnu.org/licenses/gpl-3.0.html#section6).

## Signed release builds

Initialize the release identity once:

```sh
python3 scripts/android-signing.py
```

The script creates `release.p12` and `release.properties` in `$XDG_DATA_HOME/engys/signing` (default `~/.local/share/engys/signing`). Files are private to the current user, and the randomly generated password is never printed or passed in command arguments. Back up both files securely; future updates must use the same keystore. The script refuses to replace an existing identity.

Normal builds discover these credentials automatically:

```sh
# ARM64 APK for a phone
pnpm tauri android build --target aarch64 --apk --ci
# Both supported ABIs, APK and Android App Bundle
pnpm android:build --apk --aab --ci
```

Release output:

- APK: `src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk`
- Bundle: `src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab`
- Obfuscation mapping: `src-tauri/gen/android/app/build/outputs/mapping/universalRelease/mapping.txt`

Release builds optimize Rust, shrink/minify Android code and resources, disable debugging, and sign the package. Release packaging fails if signing credentials are missing. Debug builds do not require release credentials. Increase the application version before distributing subsequent updates.

To use a different properties file, set `MIHOMO_ANDROID_SIGNING_PROPERTIES`. Alternatively supply all four environment variables: `ANDROID_KEYSTORE_PATH`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. Environment variables override the properties file. Keep credentials out of the checkout and shell history.

The currently installed debug app has a different signing certificate. Android cannot update it in place with the release APK. Export profiles before uninstalling debug and installing release; uninstalling deletes app data. Once on release, subsequent releases signed by this key update normally.

### GitHub Actions

The **Android release** workflow is manually triggered and uploads signed APK/AAB artifacts without publishing them. Configure these repository secrets using the same local release identity:

- `ANDROID_RELEASE_KEYSTORE_BASE64`: single-line base64 encoding of `release.p12`.
- `ANDROID_KEYSTORE_PASSWORD`: `storePassword` from the private properties file.
- `ANDROID_KEY_ALIAS`: `keyAlias` from the private properties file.
- `ANDROID_KEY_PASSWORD`: `keyPassword` from the private properties file.

The existing Android workflow continues to build debug APKs for pushes and pull requests without signing secrets. The release workflow has not been run remotely as part of local setup.

## Runtime

The foreground `MihomoVpnService` owns the embedded Go core in a dedicated `:vpn` process. Tauri exits its UI process when the last activity is destroyed; that exit no longer kills the VPN. A private Messenger connection transfers commands and status between processes, using private cache files for large configurations and log responses. Activity closure only releases the UI connection. Reopening the app reconnects to the same running service. The service persists the last successfully applied YAML and profile ID in private storage, restores it on a sticky/system VPN start, and clears requested-running state after Disconnect or VPN revocation. Android can still terminate or force-stop the process; a foreground notification is not a survival guarantee.

Starting with VPN enabled requests Android VPN consent automatically when needed. Android navigation contains Dashboard, Proxies, Profiles and Settings; application routing, rules and requests are available under Settings → Tools. An empty dashboard offers subscription, file import and inline profile actions. Settings contains battery exemption and notification setup, plus installed-app include/exclude pickers. App metadata is cached briefly, and icons load only for visible rows so the picker can open without rendering every installed app icon first. App routing uses package names through mihomo's PROCESS-NAME rules, with Android connection-owner UID lookup. Shared UIDs may resolve to the first associated package; attribution can fail for some connections.

Effective `tun.include-package` and `tun.exclude-package` lists become a single Android allowlist or denylist. Exclusions narrow includes. The client itself stays captured, while core sockets are individually protected from VPN routing. An allowlist whose applications are absent captures only the client, never every installed app. Saved package names survive app removal. CIDR route exclusions are subtracted before establishing the interface, including an explicitly empty result.

Android owns routes, interface creation, addresses and DNS advertisement. Desktop auto-route/interface management, redirection ports, iptables and system-time writes are disabled in the native configuration. UID/user/interface capture filters and route-set capture filters are rejected rather than silently ignored. System DNS is supplied from underlying non-VPN networks. Changing effective configuration reconnects the tunnel and can close existing connections.

Android treats the configured controller port as a preference. If it is occupied, the service increments it until an available loopback port is found, up to 65535. It skips standard proxy listener ports declared by the same configuration, reuses its own controller on reload, and checks a per-core identity before declaring readiness. The selected endpoint is returned to the UI and saved in the service runtime snapshot; source profiles and the preferred setting are unchanged. Settings shows the active controller endpoint. Local mixed/HTTP/SOCKS listener ports are separate settings and must be distinct if multiple clients need those listeners simultaneously.

The core is bundled and pinned in `core-mobile/go.mod`; Android's desktop-style core download/install commands are disabled. Update the application to update the core. Profiles, providers and databases can still update independently.

## Verification

```sh
pnpm check
node --test tests/*.test.mjs
(cd src-tauri && cargo test --lib)
(cd core-mobile && go test ./mobile)
```

Before release, test on physical devices: VPN consent denial/revocation, notification and battery denial, TCP/UDP/DNS/IPv6, per-app allow/deny and routing, missing/shared-UID apps, Wi-Fi/mobile handover, screen-off/Doze, activity recreation, process death, always-on system starts, and rapid start/stop. System VPN startup needs a previously saved, VPN-enabled working profile. Subscription scheduling uses the shared Rust backend; after service-only process restoration it resumes when Tauri is initialized again.
