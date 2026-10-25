package dev.phonesync.app

import android.view.LayoutInflater
import android.view.ViewGroup
import androidx.recyclerview.widget.RecyclerView
import dev.phonesync.app.databinding.ItemFolderBinding

class FolderAdapter(
    private val onToggle: (String, Boolean) -> Unit,
) : RecyclerView.Adapter<FolderAdapter.Holder>() {

    private var entries: List<FolderEntry> = emptyList()
    private var checked: Set<String> = emptySet()
    private var editable = true

    fun submit(entries: List<FolderEntry>, checked: Set<String>, editable: Boolean) {
        this.entries = entries
        this.checked = checked
        this.editable = editable
        notifyDataSetChanged()
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): Holder {
        val binding = ItemFolderBinding.inflate(
            LayoutInflater.from(parent.context), parent, false,
        )
        return Holder(binding)
    }

    override fun getItemCount() = entries.size

    override fun onBindViewHolder(holder: Holder, position: Int) {
        val entry = entries[position]
        val b = holder.binding
        b.folderName.text = entry.path
        b.folderCount.text = b.root.context.getString(R.string.files_count, entry.count)

        b.folderCheck.setOnCheckedChangeListener(null)
        b.folderCheck.isChecked = checked.contains(entry.path)
        b.folderCheck.isEnabled = editable
        b.folderCheck.setOnCheckedChangeListener { _, isChecked ->
            onToggle(entry.path, isChecked)
        }
        b.root.setOnClickListener {
            if (editable) b.folderCheck.toggle()
        }
        b.root.alpha = if (editable) 1f else 0.75f
    }

    class Holder(val binding: ItemFolderBinding) : RecyclerView.ViewHolder(binding.root)
}
