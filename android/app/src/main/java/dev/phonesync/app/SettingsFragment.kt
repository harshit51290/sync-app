package dev.phonesync.app

import android.content.Context
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Toast
import androidx.core.content.ContextCompat
import androidx.fragment.app.Fragment
import androidx.lifecycle.lifecycleScope
import dev.phonesync.app.databinding.FragmentSettingsBinding
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class SettingsFragment : Fragment() {

    private var _binding: FragmentSettingsBinding? = null
    private val binding get() = _binding!!

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View {
        _binding = FragmentSettingsBinding.inflate(inflater, container, false)
        return binding.root
    }

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val prefs = App.prefs
        binding.address.setText(prefs.baseUrl)
        binding.token.setText(prefs.token)
        binding.deviceName.setText(prefs.deviceName)
        binding.autoSync.isChecked = prefs.autoSync
        binding.wifiOnly.isChecked = prefs.wifiOnly
        binding.version.text = getString(R.string.version_line, versionName())

        binding.autoSync.setOnCheckedChangeListener { _, checked ->
            prefs.autoSync = checked
            SyncScheduler.ensure(requireContext())
        }
        binding.wifiOnly.setOnCheckedChangeListener { _, checked ->
            prefs.wifiOnly = checked
            SyncScheduler.ensure(requireContext())
        }

        binding.testConnection.setOnClickListener { testConnection() }
        binding.findLaptop.setOnClickListener { findLaptop() }
        binding.saveName.setOnClickListener {
            App.prefs.deviceName = binding.deviceName.text.toString()
            binding.deviceName.setText(App.prefs.deviceName)
            toast(getString(R.string.name_saved))
        }
        binding.resetHistory.setOnClickListener {
            App.prefs.resetHistory()
            SyncEngine.log.value = emptyList()
            toast(getString(R.string.reset_done))
        }
    }

    override fun onPause() {
        super.onPause()
        saveConnectionFields()
    }

    private fun saveConnectionFields() {
        val b = _binding ?: return
        val address = b.address.text.toString()
        // A pasted sync link carries the token; take it when the field is empty so
        // pairing is "paste the link" instead of "paste, then hunt for the key".
        val pasted = Regex("[?&]k=([A-Za-z0-9]+)").find(address)?.groupValues?.get(1)
        val token = b.token.text.toString().trim().ifBlank { pasted.orEmpty() }
        App.prefs.baseUrl = address
        App.prefs.token = token
        if (token.isNotBlank() && b.token.text.isBlank()) b.token.setText(token)
        (activity as? MainActivity)?.refreshChip()
        SyncScheduler.ensure(requireContext())
    }

    private fun testConnection() {
        saveConnectionFields()
        val api = Api(App.prefs.baseUrl, App.prefs.token)
        showState(getString(R.string.finding), R.color.muted)
        viewLifecycleOwner.lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { api.info() } }
            val info = result.getOrNull()
            if (info == null) {
                showState(getString(R.string.connection_bad), R.color.bad)
            } else {
                val name = info.optString("name").ifBlank { "laptop" }
                showState(getString(R.string.test_ok, name), R.color.good)
            }
        }
    }

    private fun findLaptop() {
        val context: Context = requireContext().applicationContext
        val token = binding.token.text.toString()
        showState(getString(R.string.finding), R.color.muted)
        binding.findLaptop.isEnabled = false
        viewLifecycleOwner.lifecycleScope.launch {
            val candidates = withContext(Dispatchers.IO) { Discovery(context).find() }
            val match = withContext(Dispatchers.IO) {
                candidates.firstOrNull { url ->
                    runCatching { Api(url, token).info() }.isSuccess
                }
            }
            binding.findLaptop.isEnabled = true
            if (match == null) {
                showState(getString(R.string.found_none), R.color.bad)
                return@launch
            }
            binding.address.setText(match)
            saveConnectionFields()
            showState(getString(R.string.test_ok, match), R.color.good)
        }
    }

    private fun showState(text: String, color: Int) {
        val b = _binding ?: return
        b.connectionState.visibility = View.VISIBLE
        b.connectionState.text = text
        b.connectionState.setTextColor(ContextCompat.getColor(requireContext(), color))
    }

    private fun versionName(): String = runCatching {
        requireContext().packageManager.getPackageInfo(requireContext().packageName, 0).versionName
            ?: "1.0"
    }.getOrDefault("1.0")

    private fun toast(text: String) {
        Toast.makeText(requireContext(), text, Toast.LENGTH_SHORT).show()
    }

    override fun onDestroyView() {
        super.onDestroyView()
        _binding = null
    }
}
