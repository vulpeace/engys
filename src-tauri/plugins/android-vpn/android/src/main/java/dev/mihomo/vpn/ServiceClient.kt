package dev.mihomo.vpn

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import androidx.core.content.ContextCompat
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.Executors

/** UI-side control connection. The service owns all Go state in its own process. */
internal class ServiceClient(context: Context) {
    private val context = context.applicationContext
    private val handler = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor()
    private data class Pending(val invoke: Invoke, var file: File?, var sent: Boolean = false)
    private val pending = mutableMapOf<Int, Pending>()
    private var sequence = 0
    private var remote: Messenger? = null
    private var bound = false
    private var closed = false
    private val replies = Messenger(Handler(Looper.getMainLooper()) { message ->
        val id = message.arg1
        val path = message.data.getString("response")
        io.execute {
            val result = runCatching {
                val file = privateChild(context.cacheDir, File(requireNotNull(path)))
                require(file.parentFile == context.cacheDir.canonicalFile && file.name.startsWith("vpn-reply-"))
                try { JSONObject(file.readText()) } finally { file.delete() }
            }
            handler.post {
                val request = pending.remove(id) ?: return@post
                result.fold(onSuccess = { response ->
                    val error = response.optString("error")
                    if (error.isNotEmpty()) request.invoke.reject(error)
                    else when (val value = response.opt("value")) {
                        null, JSONObject.NULL -> request.invoke.resolve()
                        is JSONObject -> request.invoke.resolve(JSObject(value.toString()))
                        is JSONArray -> request.invoke.resolveObject((0 until value.length()).map { value.getString(it) })
                        else -> request.invoke.resolveObject(value)
                    }
                }, onFailure = { request.invoke.reject(it.message ?: "Cannot read service response") })
            }
        }
        true
    })
    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName, binder: IBinder) {
            remote = Messenger(binder)
            pending.keys.toList().forEach(::send)
        }
        override fun onServiceDisconnected(name: ComponentName) {
            remote = null
            failAll("VPN service connection was lost; reconnecting")
        }
        override fun onBindingDied(name: ComponentName) { releaseBinding(); failAll("VPN service binding was lost") }
        override fun onNullBinding(name: ComponentName) { releaseBinding(); failAll("VPN service rejected the control connection") }
    }
    fun call(method: String, arguments: JSONObject, invoke: Invoke) {
        io.execute {
            val result = runCatching {
                val file = File.createTempFile("vpn-command-", ".json", context.cacheDir)
                try {
                    if (method == "memory") arguments.put("uiRss", residentMemory() ?: 0)
                    file.writeText(arguments.toString()); file
                }
                catch (e: Exception) { file.delete(); throw e }
            }
            handler.post {
                val file = result.getOrElse { invoke.reject(it.message); return@post }
                if (closed) { file.delete(); invoke.reject("Application activity was closed"); return@post }
                val id = ++sequence
                pending[id] = Pending(invoke, file)
                try {
                    // Only an explicit connection request starts the foreground service.
                    // Status/version queries merely bind and never post a VPN notification.
                    if (method == "start") ContextCompat.startForegroundService(context,
                        Intent(context, MihomoVpnService::class.java).setAction(MihomoVpnService.START))
                    argumentsById[id] = method
                    if (!bound) {
                        bound = context.bindService(Intent(context, MihomoVpnService::class.java)
                            .setAction(MihomoVpnService.CONTROL), connection, Context.BIND_AUTO_CREATE)
                        check(bound) { "Cannot bind to VPN service" }
                    }
                    send(id)
                    handler.postDelayed({ fail(id, "VPN service command timed out") }, 120_000)
                } catch (e: Exception) { fail(id, e.message ?: "Cannot contact VPN service") }
            }
        }
    }
    private val argumentsById = mutableMapOf<Int, String>()
    private fun send(id: Int) {
        val target = remote ?: return
        val request = pending[id] ?: return
        if (request.sent) return
        try {
            target.send(Message.obtain().apply {
                arg1 = id; replyTo = replies
                data = Bundle().apply { putString("method", argumentsById.remove(id)); putString("request", request.file!!.absolutePath) }
            })
            request.sent = true
            request.file = null // The service owns the file after a successful send.
        } catch (e: Exception) { fail(id, e.message ?: "Cannot send VPN command") }
    }
    private fun fail(id: Int, reason: String) {
        argumentsById.remove(id)
        pending.remove(id)?.let { it.file?.delete(); it.invoke.reject(reason) }
    }
    private fun failAll(reason: String) { pending.keys.toList().forEach { fail(it, reason) } }
    private fun releaseBinding() {
        remote = null
        if (bound) { runCatching { context.unbindService(connection) }; bound = false }
    }
    fun close() {
        handler.post {
            closed = true
            failAll("Application activity was closed")
            releaseBinding()
            // Do not stop the foreground service when the UI is destroyed.
        }
    }
}
