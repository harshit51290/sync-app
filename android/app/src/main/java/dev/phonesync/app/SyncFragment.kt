package dev.phonesync.app

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import androidx.core.content.ContextCompat
import androidx.fragment.app.Fragment
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.recyclerview.widget.LinearLayoutManager
import dev.phonesync.app.databinding.FragmentSyncBinding
import kotlinx.coroutines.launch

class SyncFragment : Fragment() {

    private var _binding: FragmentSyncBinding? = null
    private val binding get() = _binding!!
    private lateinit var logAdapter: LogAdapter

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View {
        _binding = FragmentSyncBinding.inflate(inflater, container, false)
        return binding.root
    }

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        logAdapter = LogAdapter()
        binding.logList.layoutManager = LinearLayoutManager(requireContext())
        binding.logList.adapter = logAdapter

        binding.syncNow.setOnClickListener {
            if (!App.prefs.isPaired()) {
                (activity as? MainActivity)?.openTab(R.id.nav_settings)
                return@setOnClickListener
            }
            SyncService.start(requireContext())
        }
        binding.stopSync.setOnClickListener { SyncEngine.cancel() }

        viewLifecycleOwner.lifecycleScope.launch {
            viewLifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) {
                launch { SyncEngine.state.collect { render(it) } }
                launch {
                    SyncEngine.log.collect { lines ->
                        logAdapter.submit(lines)
                        binding.logEmpty.visibility =
                            if (lines.isEmpty()) View.VISIBLE else View.GONE
                        binding.logList.visibility =
                            if (lines.isEmpty()) View.GONE else View.VISIBLE
                    }
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        render(SyncEngine.state.value)
    }

    private fun render(state: SyncEngine.State) {
        val b = _binding ?: return
        b.phaseTitle.text = state.title.ifBlank { getString(R.string.sync_idle_title) }
        b.phaseDetail.text = state.detail.ifBlank {
            if (App.prefs.isPaired()) {
                getString(R.string.sync_idle_detail)
            } else {
                getString(R.string.pair_hint)
            }
        }

        val (icon, tint) = when (state.phase) {
            SyncEngine.Phase.DONE -> R.drawable.ic_check to R.color.good
            SyncEngine.Phase.ERROR -> R.drawable.ic_alert to R.color.bad
            SyncEngine.Phase.OFFLINE -> R.drawable.ic_wifi to R.color.muted
            SyncEngine.Phase.PAUSED -> R.drawable.ic_alert to R.color.warn
            SyncEngine.Phase.UNPAIRED, SyncEngine.Phase.NO_ACCESS ->
                R.drawable.ic_laptop to R.color.muted
            else -> R.drawable.ic_bolt to R.color.lime
        }
        b.phaseIcon.setImageResource(icon)
        b.phaseIcon.setColorFilter(ContextCompat.getColor(requireContext(), tint))

        if (state.running) {
            b.progress.visibility = View.VISIBLE
            if (state.total > 0) {
                b.progress.isIndeterminate = false
                b.progress.max = state.total
                b.progress.setProgressCompat(state.done, true)
            } else {
                b.progress.isIndeterminate = true
            }
        } else {
            b.progress.visibility = View.GONE
        }

        b.syncNow.isEnabled = !state.running
        b.stopSync.visibility = if (state.running) View.VISIBLE else View.GONE
        b.statSent.text = state.sent.toString()
        b.statSkipped.text = state.skipped.toString()
        b.statFailed.text = state.failed.toString()
    }

    override fun onDestroyView() {
        super.onDestroyView()
        _binding = null
    }
}
