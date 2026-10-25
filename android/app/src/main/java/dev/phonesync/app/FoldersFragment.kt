package dev.phonesync.app

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import androidx.fragment.app.Fragment
import androidx.lifecycle.lifecycleScope
import androidx.recyclerview.widget.LinearLayoutManager
import dev.phonesync.app.databinding.FragmentFoldersBinding
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class FoldersFragment : Fragment() {

    private var _binding: FragmentFoldersBinding? = null
    private val binding get() = _binding!!
    private lateinit var adapter: FolderAdapter
    private var entries: List<FolderEntry> = emptyList()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View {
        _binding = FragmentFoldersBinding.inflate(inflater, container, false)
        return binding.root
    }

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        adapter = FolderAdapter { path, checked ->
            val chosen = App.prefs.folders.toMutableSet()
            if (checked) chosen.add(path) else chosen.remove(path)
            App.prefs.folders = chosen
        }
        binding.folderList.layoutManager = LinearLayoutManager(requireContext())
        binding.folderList.adapter = adapter

        binding.allFolders.isChecked = App.prefs.allFolders
        binding.allFolders.setOnCheckedChangeListener { _, checked ->
            App.prefs.allFolders = checked
            if (!checked && App.prefs.folders.isEmpty()) {
                App.prefs.folders = entries.map { it.path }.toSet()
            }
            applyState()
        }

        binding.rescan.setOnClickListener { load() }
        applyState()
        load()
    }

    fun load() {
        val b = _binding ?: return
        val context = requireContext().applicationContext
        viewLifecycleOwner.lifecycleScope.launch {
            b.rescan.isEnabled = false
            b.rescan.text = getString(R.string.scanning)
            val items = withContext(Dispatchers.IO) { MediaScanner.scan(context) }
            entries = MediaScanner.folders(items)
            b.rescan.text = getString(R.string.rescan)
            b.rescan.isEnabled = true
            b.folderEmpty.visibility =
                if (entries.isEmpty()) View.VISIBLE else View.GONE
            applyState()
        }
    }

    private fun applyState() {
        val b = _binding ?: return
        val prefs = App.prefs
        val everything = prefs.allFolders
        b.allFoldersHint.text = getString(
            if (everything) R.string.folder_hint_on else R.string.folder_hint_off,
        )
        if (everything) {
            b.allFoldersHint.text =
                getString(R.string.folder_hint_on)
        }
        val checked = if (everything) entries.map { it.path }.toSet() else prefs.folders
        adapter.submit(entries, checked, editable = !everything)
        b.folderList.alpha = if (everything) 0.55f else 1f
    }

    override fun onDestroyView() {
        super.onDestroyView()
        _binding = null
    }
}
