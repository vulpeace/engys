package dev.mihomo.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.util.AtomicFile
import androidx.core.app.NotificationCompat
import dev.mihomo.vpn.mobile.Host
import dev.mihomo.vpn.mobile.Mobile
import org.json.JSONObject
import java.io.File
import java.net.InetSocketAddress
import java.net.URI
import java.util.concurrent.Executors

internal object RuntimeState {
    val executor = Executors.newSingleThreadExecutor()
    val generation = java.util.concurrent.atomic.AtomicLong()
    val wantRunning = java.util.concurrent.atomic.AtomicBoolean()
    @Volatile var service: MihomoVpnService? = null
    @Volatile var vpnActive = false
    @Volatile var status = JSONObject().put("running", false).put("controller", "").put("secret", "")
    fun setDesired(context: Context, value: Boolean) {
        check(context.getSharedPreferences("vpn", Context.MODE_PRIVATE).edit().putBoolean("desired", value).commit()) { "Cannot persist VPN state" }
    }
}

class MihomoVpnService : VpnService() {
    private var request: JSONObject? = null
    private var startedAt = 0L
    private val connectivity by lazy { getSystemService(ConnectivityManager::class.java) }
    private val networks = mutableMapOf<Network, LinkProperties>()
    private var previousNetwork: Network? = null
    private var observing = false
    private val saved by lazy { AtomicFile(File(filesDir, "vpn-runtime.json")) }
    private val host = object : Host {
        override fun protect(fd: Int): Boolean = this@MihomoVpnService.protect(fd)
        override fun resolveUid(protocol: Int, source: String, destination: String): Int = runCatching {
            fun endpoint(text: String): InetSocketAddress {
                val uri = URI("tcp://$text")
                return InetSocketAddress(uri.host, uri.port)
            }
            connectivity.getConnectionOwnerUid(protocol, endpoint(source), endpoint(destination))
        }.getOrDefault(-1)
        override fun resolvePackage(uid: Int): String = packageManager.getPackagesForUid(uid)?.firstOrNull().orEmpty()
    }
    override fun onCreate() {
        super.onCreate()
        RuntimeState.service = this
    }
    private fun promote() {
        // Promote before parsing profiles or loading the Go runtime.
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel(CHANNEL, "VPN connection", NotificationManager.IMPORTANCE_LOW).apply {
            setSound(null, null); enableVibration(false); setShowBadge(false)
        })
        val launch = packageManager.getLaunchIntentForPackage(packageName)
        val stop = PendingIntent.getService(this, 2, Intent(this, MihomoVpnService::class.java).setAction(STOP), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val notification = NotificationCompat.Builder(this, CHANNEL).setSmallIcon(R.drawable.ic_vpn)
            .setContentTitle("engys").setContentText("Proxy service running")
            .setOngoing(true).setSilent(true).setOnlyAlertOnce(true).setPriority(NotificationCompat.PRIORITY_LOW)
            .setCategory(NotificationCompat.CATEGORY_SERVICE).addAction(0, "Disconnect", stop)
            .apply { if (launch != null) setContentIntent(PendingIntent.getActivity(this@MihomoVpnService, 1, launch, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)) }.build()
        if (Build.VERSION.SDK_INT >= 34) startForeground(1, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        else startForeground(1, notification)
    }
    private val control = Messenger(Handler(Looper.getMainLooper()) { message ->
        val reply = message.replyTo
        val id = message.arg1
        val method = message.data.getString("method").orEmpty()
        val path = message.data.getString("request")
        if (method == "start") RuntimeState.wantRunning.set(true)
        if (method == "stop") RuntimeState.wantRunning.set(false)
        val generation = if (method == "start" || method == "stop") RuntimeState.generation.incrementAndGet() else RuntimeState.generation.get()
        RuntimeState.executor.execute {
            val response = JSONObject()
            try {
                val file = privateChild(cacheDir, File(requireNotNull(path)))
                require(file.parentFile == cacheDir.canonicalFile && file.name.startsWith("vpn-command-")) { "Invalid VPN command file" }
                val arguments = try { JSONObject(file.readText()) } finally { file.delete() }
                val value: Any? = when (method) {
                    "version" -> Mobile.version()
                    "logs" -> org.json.JSONArray(Mobile.logs())
                    "status" -> RuntimeState.status
                    "vpnActive" -> RuntimeState.vpnActive
                    "memory" -> JSONObject().put("processRss", residentMemory()?.plus(arguments.optLong("uiRss", 0)) ?: JSONObject.NULL)
                        .put("coreHeap", Mobile.heapAllocated())
                    "validate" -> {
                        val home = arguments.getString("home")
                        privateChild(dataDir, File(home))
                        Mobile.validate(arguments.getString("yaml"), home)
                        null
                    }
                    "start" -> {
                        check(generation == RuntimeState.generation.get()) { "VPN start was cancelled" }
                        RuntimeState.service = this
                        startRuntime(arguments)
                        check(generation == RuntimeState.generation.get()) { "VPN start was cancelled" }
                        persist(arguments)
                        RuntimeState.setDesired(this, true)
                        null
                    }
                    "stop" -> { disconnect(true); null }
                    "configure" -> { check(RuntimeState.status.optBoolean("running")) { "VPN service is not running" }; reconfigure(arguments.getString("yaml")); null }
                    "setProfile" -> { if (RuntimeState.status.optBoolean("running")) setProfile(arguments.getString("profileId")); null }
                    else -> error("Unknown VPN service command")
                }
                response.put("value", value ?: JSONObject.NULL)
            } catch (error: Exception) {
                android.util.Log.e("MihomoVpn", "Service command failed: $method", error)
                if (method == "start" && (generation == RuntimeState.generation.get() || !RuntimeState.wantRunning.get())) {
                    disconnect(!RuntimeState.wantRunning.get(), error.message)
                }
                response.put("error", error.message ?: "VPN service command failed")
            }
            sendResponse(reply, id, response)
        }
        true
    })
    override fun onBind(intent: Intent): IBinder? = if (intent.action == CONTROL) control.binder else super.onBind(intent)
    private fun sendResponse(reply: Messenger?, id: Int, response: JSONObject) {
        if (reply == null) return
        val file = File.createTempFile("vpn-reply-", ".json", cacheDir)
        try {
            file.writeText(response.toString())
            reply.send(Message.obtain().apply { arg1 = id; data = Bundle().apply { putString("response", file.absolutePath) } })
        } catch (_: Exception) { file.delete() } // The activity may have exited; the core keeps running.
    }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == STOP) {
            RuntimeState.wantRunning.set(false)
            RuntimeState.generation.incrementAndGet()
            RuntimeState.executor.execute { disconnect(true) }
            return START_NOT_STICKY
        }
        promote()
        if (intent?.action == START) return START_STICKY // The UI sends configuration over the private control connection.
        RuntimeState.wantRunning.set(true)
        val generation = RuntimeState.generation.incrementAndGet()
        RuntimeState.executor.execute {
            try {
                if (RuntimeState.status.optBoolean("running")) return@execute
                val desired = getSharedPreferences("vpn", MODE_PRIVATE).getBoolean("desired", false)
                check(desired || intent?.action == SERVICE_INTERFACE) { "VPN was stopped by the user" }
                val next = JSONObject(String(saved.readFully()))
                if (intent?.action == SERVICE_INTERFACE) {
                    check(JSONObject(Mobile.options(next.getString("yaml"))).getBoolean("enable")) {
                        "Enable VPN in the application before configuring Always-on VPN"
                    }
                }
                check(generation == RuntimeState.generation.get()) { "VPN start was cancelled" }
                RuntimeState.service = this
                startRuntime(next)
                persist(next)
                RuntimeState.setDesired(this, true)
            } catch (error: Exception) {
                android.util.Log.e("MihomoVpn", "Service restoration failed", error)
                if (generation == RuntimeState.generation.get()) disconnect(false, error.message)
            }
        }
        return START_STICKY
    }
    override fun onTaskRemoved(rootIntent: Intent?) {
        // The Tauri activity process can exit. This independently started VPN
        // process must retain its tunnel, callbacks and core without restarting.
        android.util.Log.i("MihomoVpn", "UI task removed; VPN service remains independent")
        super.onTaskRemoved(rootIntent)
    }
    private class ControllerCollision : Exception("Controller port belongs to another core")
    private fun startRuntime(next: JSONObject) {
        val original = next.getString("yaml")
        val home = next.getString("home")
        privateChild(dataDir, File(home))
        Mobile.validate(original, home)
        var minimum = 0
        while (true) {
            val yaml = Mobile.controllerConfig(original, minimum)
            val options = JSONObject(Mobile.options(yaml))
            if (options.getBoolean("enable")) check(prepare(this) == null) { "Android VPN permission is required" }
            if (options.getBoolean("enable")) Mobile.prepareProtection(host)
            val fd = if (options.getBoolean("enable")) establish(options) else -1
            // Start owns the detached descriptor, including on failure.
            Mobile.start(yaml, home, fd)
            observeNetworks()
            try { awaitController(options) }
            catch (_: ControllerCollision) {
                val selected = URI(options.getString("controller")).port
                check(selected < 65535) { "No available controller port remains" }
                minimum = selected + 1
                continue
            }
            next.put("yaml", yaml)
            request = next
            RuntimeState.vpnActive = options.getBoolean("enable")
            startedAt = System.currentTimeMillis() / 1000
            publish(options)
            return
        }
    }
    private fun establish(options: JSONObject): Int {
        val builder = Builder().setSession("engys").setMtu(options.getInt("mtu"))
            .addAddress("198.18.0.1", 30).addDnsServer("198.18.0.2").setBlocking(false).setMetered(false)
        if (options.getBoolean("ipv6")) builder.addAddress("fdfe:dcba:9876::1", 126).addDnsServer("fdfe:dcba:9876::2")
        val routes = options.getJSONArray("routes")
        for (i in 0 until routes.length()) {
            val parts = routes.getString(i).split("/")
            builder.addRoute(parts[0], parts[1].toInt())
        }
        if (options.getBoolean("allowMode")) {
            // Keep the client captured and protect only core outbound sockets. Also makes
            // an empty/missing user allowlist capture no other apps instead of all apps.
            builder.addAllowedApplication(packageName)
            val allowed = options.getJSONArray("include")
            for (i in 0 until allowed.length()) try { builder.addAllowedApplication(allowed.getString(i)) } catch (_: PackageManager.NameNotFoundException) { }
        } else {
            val denied = options.optJSONArray("exclude")
            if (denied != null) for (i in 0 until denied.length()) {
                val name = denied.getString(i)
                if (name != packageName) try { builder.addDisallowedApplication(name) } catch (_: PackageManager.NameNotFoundException) { }
            }
        }
        return requireNotNull(builder.establish()) { "Android rejected VPN establishment" }.detachFd()
    }
    private fun awaitController(options: JSONObject) {
        var last: Exception? = null
        val identity = Mobile.controllerIdentity()
        var foreignResponses = 0
        repeat(120) {
            try {
                val connection = java.net.URL(options.getString("controller") + "/engys/instance").openConnection() as java.net.HttpURLConnection
                try {
                    connection.connectTimeout = 500; connection.readTimeout = 500
                    connection.setRequestProperty("Authorization", "Bearer " + options.getString("secret"))
                    val code = connection.responseCode
                    if (code == 200) {
                        val found = connection.inputStream.bufferedReader().use { it.readText() }
                        if (found == identity) return
                        throw ControllerCollision()
                    }
                    if (code == 401 || code == 404) {
                        // A reconfiguration may briefly reach our previous server.
                        if (++foreignResponses >= 3) throw ControllerCollision()
                    }
                } finally { connection.disconnect() }
            } catch (error: ControllerCollision) { throw error }
            catch (error: Exception) { last = error }
            Thread.sleep(250)
        }
        throw IllegalStateException("Core controller did not become ready", last)
    }
    private fun publish(options: JSONObject) {
        RuntimeState.status = JSONObject().put("running", true).put("pid", android.os.Process.myPid())
            .put("controller", options.getString("controller")).put("secret", options.getString("secret"))
            .put("profileId", request?.optString("profileId")).put("startedAt", startedAt)
            .put("binary", "bundled-mihomo").put("lastError", JSONObject.NULL)
    }
    private fun persist(next: JSONObject) {
        val output = saved.startWrite()
        try { output.write(next.toString().toByteArray()); saved.finishWrite(output) }
        catch (e: Exception) { saved.failWrite(output); throw e }
    }
    internal fun setProfile(id: String) {
        val next = request ?: return
        next.put("profileId", id); persist(next)
        publish(JSONObject(Mobile.options(next.getString("yaml"))))
    }
    internal fun reconfigure(yaml: String) {
        val previous = requireNotNull(request)
        val next = JSONObject(previous.toString()).put("yaml", yaml)
        try { startRuntime(next); persist(next) }
        catch (e: Exception) {
            try { startRuntime(previous) } catch (restore: Exception) { disconnect(false, "${e.message}; restore failed: ${restore.message}") }
            throw e
        }
    }
    internal fun disconnect(forget: Boolean, error: String? = null) {
        try {
            if (forget) RuntimeState.setDesired(this, false)
            Mobile.stop()
        } finally {
            if (observing) { connectivity.unregisterNetworkCallback(callback); observing = false }
            RuntimeState.vpnActive = false
            RuntimeState.status = JSONObject().put("running", false).put("controller", "").put("secret", "").put("lastError", error ?: JSONObject.NULL)
            if (RuntimeState.service === this) RuntimeState.service = null
            stopForeground(STOP_FOREGROUND_REMOVE); stopSelf()
        }
    }
    override fun onRevoke() { RuntimeState.wantRunning.set(false); RuntimeState.generation.incrementAndGet(); RuntimeState.executor.execute { disconnect(true) } }
    override fun onDestroy() {
        RuntimeState.executor.execute { if (RuntimeState.service === this) disconnect(false) }
        super.onDestroy()
    }
    private fun observeNetworks() {
        if (observing) return
        connectivity.registerNetworkCallback(NetworkRequest.Builder().addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN).addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET).build(), callback)
        observing = true
    }
    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) { RuntimeState.executor.execute { networks[network] = properties; updateNetwork() } }
        override fun onLost(network: Network) { RuntimeState.executor.execute { networks.remove(network); updateNetwork() } }
        override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) { RuntimeState.executor.execute { updateNetwork() } }
    }
    private fun updateNetwork() {
        if (RuntimeState.service !== this || !observing) return
        val preferred = networks.keys.minByOrNull { network ->
            val caps = connectivity.getNetworkCapabilities(network)
            when {
                caps?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true -> 0
                caps?.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) == true -> 1
                else -> 2
            }
        }
        val servers = networks[preferred]?.dnsServers.orEmpty().joinToString(" ") { address ->
            val host = address.hostAddress.orEmpty()
            if (host.contains(':')) "[$host]:53" else "$host:53"
        }
        Mobile.updateDNS(servers)
        if (previousNetwork != null && previousNetwork != preferred) Mobile.resetNetwork()
        previousNetwork = preferred
        setUnderlyingNetworks(preferred?.let { arrayOf(it) })
    }
    companion object {
        private const val CHANNEL = "mihomo-vpn"
        private const val STOP = "dev.mihomo.vpn.STOP"
        const val START = "dev.mihomo.vpn.START"
        const val CONTROL = "dev.mihomo.vpn.CONTROL"
    }
}
