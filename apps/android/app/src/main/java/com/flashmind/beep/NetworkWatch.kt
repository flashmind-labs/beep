package com.flashmind.beep

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Tells the endpoint when the network changes, since Android gives iroh no way to watch it
 * itself, and catches up on a new network so the server sends what it queued meanwhile.
 */
object NetworkWatch {
    private var job: Job? = null
    /** The network the phone last caught up on. */
    @Volatile private var caughtUpOn: Network? = null

    /**
     * Capability updates arrive often (signal strength among them), so only a change of
     * transport or validation counts, and a burst of callbacks becomes one call.
     */
    fun start(context: Context, scope: CoroutineScope) {
        val cm = context.getSystemService(ConnectivityManager::class.java)
        cm.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
            private var last: NetworkCapabilities? = null
            override fun onAvailable(network: Network) = changed(cm, scope)
            override fun onLost(network: Network) = changed(cm, scope)
            override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) = changed(cm, scope)
            override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
                val before = last
                last = capabilities
                if (before == null || differs(before, capabilities)) changed(cm, scope)
            }
        })
    }

    private fun differs(a: NetworkCapabilities, b: NetworkCapabilities): Boolean =
        listOf(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.TRANSPORT_CELLULAR,
            NetworkCapabilities.TRANSPORT_ETHERNET, NetworkCapabilities.TRANSPORT_VPN)
            .any { a.hasTransport(it) != b.hasTransport(it) } ||
            a.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) !=
            b.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)

    /** Callbacks all run on one thread, so the job needs no lock. */
    private fun changed(cm: ConnectivityManager, scope: CoroutineScope) {
        job?.cancel()
        job = scope.launch {
            delay(400)
            val repository = BeepApplication.instance?.repository
            val network = cm.activeNetwork
            val online = network != null && cm.getNetworkCapabilities(network)
                ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true
            if (online) repository?.networkBack() else { caughtUpOn = null; repository?.networkLost() }
            val client = BeepClients.current() ?: return@launch
            client.networkChanged()
            if (online && network != caughtUpOn) {
                caughtUpOn = network
                // Connecting is enough: the server sends what it queued. The connection closes
                // itself when idle.
                try { client.health() } catch (error: Exception) { if (error is CancellationException) throw error }
            }
        }
    }
}
