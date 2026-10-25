package dev.phonesync.app

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/// Finds the laptop over mDNS so the user only has to confirm a token.
class Discovery(private val context: Context) {

    private val nsd = context.getSystemService(Context.NSD_SERVICE) as NsdManager

    fun find(timeoutMs: Long = 5_000): List<String> {
        val found = mutableListOf<NsdServiceInfo>()
        val started = CountDownLatch(1)
        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) {
                started.countDown()
            }

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                synchronized(found) { found.add(serviceInfo) }
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) = Unit
            override fun onDiscoveryStopped(serviceType: String) = Unit
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                started.countDown()
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) = Unit
        }

        if (!runCatching { nsd.discoverServices(SERVICE, NsdManager.PROTOCOL_DNS_SD, listener) }
                .isSuccess) {
            return emptyList()
        }
        started.await(2, TimeUnit.SECONDS)
        Thread.sleep(timeoutMs)

        runCatching { nsd.stopServiceDiscovery(listener) }

        val services = synchronized(found) { found.toList() }
        return services.mapNotNull { resolve(it) }.distinct()
    }

    private fun resolve(info: NsdServiceInfo): String? {
        val latch = CountDownLatch(1)
        var host: String? = null
        var port = 0
        // The two-argument overload is the only one that exists back to API 29;
        // the Executor variant added in API 34 would NoSuchMethodError on older
        // phones even though it compiles against SDK 35.
        @Suppress("DEPRECATION")
        runCatching {
            nsd.resolveService(info, object : NsdManager.ResolveListener {
                override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                    latch.countDown()
                }

                override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                    val address = serviceInfo.host
                    if (address != null && !address.isLoopbackAddress) {
                        host = address.hostAddress
                        port = serviceInfo.port
                    }
                    latch.countDown()
                }
            })
        }
        latch.await(4, TimeUnit.SECONDS)
        val ip = host ?: return null
        if (port <= 0) return null
        val pretty = if (ip.contains(':')) "[$ip]" else ip
        return "http://$pretty:$port"
    }

    private companion object {
        const val SERVICE = "_phonesync._tcp"
    }
}
