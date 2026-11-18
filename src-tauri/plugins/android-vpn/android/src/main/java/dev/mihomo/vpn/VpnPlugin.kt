package dev.mihomo.vpn

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.net.VpnService
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.result.ActivityResult
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject

@InvokeArg
class ConfigArgs { var yaml: String = ""; var home: String = ""; var profileId: String = "" }

@InvokeArg
class PackageArgs { var packageName: String = "" }

@TauriPlugin(permissions = [Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "notifications")])
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
    private val service = ServiceClient(activity)
    private fun work(invoke: Invoke, action: () -> Unit) {
        InstalledApps.executor.execute {
            try { action() } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
        }
    }
    private fun command(method: String, invoke: Invoke, hasArguments: Boolean = false) {
        val args = if (hasArguments) invoke.parseArgs(ConfigArgs::class.java) else ConfigArgs()
        service.call(method, JSONObject().put("yaml", args.yaml).put("home", args.home).put("profileId", args.profileId), invoke)
    }
    override fun onDestroy() { service.close() }
    @Command fun version(invoke: Invoke) = command("version", invoke)
    @Command fun memory(invoke: Invoke) = command("memory", invoke)
    @Command fun logs(invoke: Invoke) = command("logs", invoke)
    @Command fun status(invoke: Invoke) = command("status", invoke)
    @Command fun vpnActive(invoke: Invoke) = command("vpnActive", invoke)
    @Command fun readDocument(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(ConfigArgs::class.java)
        val uri = Uri.parse(args.yaml)
        require(uri.scheme == "content") { "Expected an Android document URI" }
        val text = requireNotNull(activity.contentResolver.openInputStream(uri)).bufferedReader().use { it.readText() }
        invoke.resolveObject(text)
    }
    @Command fun validate(invoke: Invoke) = command("validate", invoke, true)
    @Command fun start(invoke: Invoke) = command("start", invoke, true)
    @Command fun stop(invoke: Invoke) = command("stop", invoke)
    @Command fun configure(invoke: Invoke) = command("configure", invoke, true)
    @Command fun setProfile(invoke: Invoke) = command("setProfile", invoke, true)
    private fun permission(): JSObject = JSObject().apply {
        put("granted", VpnService.prepare(activity) == null)
        put("method", "vpn"); put("canGrant", true)
        put("detail", "Allow Android to establish the VPN connection.")
    }
    @Command fun vpnPermission(invoke: Invoke) { invoke.resolve(permission()) }
    @Command fun requestVpn(invoke: Invoke) {
        val intent = VpnService.prepare(activity)
        if (intent == null) invoke.resolve(permission()) else startActivityForResult(invoke, intent, "vpnResult")
    }
    @ActivityCallback fun vpnResult(invoke: Invoke, result: ActivityResult) { invoke.resolve(permission()) }
    @Command fun setupStatus(invoke: Invoke) {
        invoke.resolve(JSObject().apply {
            put("supported", true)
            put("batteryExempt", activity.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(activity.packageName))
            put("notifications", NotificationManagerCompat.from(activity).areNotificationsEnabled())
        })
    }
    @Command fun requestBattery(invoke: Invoke) {
        try {
            activity.startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${activity.packageName}")))
        } catch (_: Exception) {
            activity.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${activity.packageName}")))
        }
        invoke.resolve()
    }
    @Command fun requestNotifications(invoke: Invoke) {
        if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(activity, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissionForAlias("notifications", invoke, "notificationsResult")
            return
        } else if (!NotificationManagerCompat.from(activity).areNotificationsEnabled()) {
            activity.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName))
        }
        invoke.resolve()
    }
    @PermissionCallback fun notificationsResult(invoke: Invoke) { invoke.resolve() }
    @Command fun installedApps(invoke: Invoke) {
        InstalledApps.executor.execute {
            try { invoke.resolveObject(InstalledApps.list(activity.applicationContext)) }
            catch (e: Exception) { invoke.reject(e.message ?: "Cannot list installed applications") }
        }
    }
    @Command fun installedAppIcon(invoke: Invoke) {
        val args = invoke.parseArgs(PackageArgs::class.java)
        InstalledApps.executor.execute {
            val icon = InstalledApps.icon(activity.applicationContext, args.packageName)
            if (icon == null) invoke.resolve() else invoke.resolveObject(icon)
        }
    }
}
