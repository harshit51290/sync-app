package dev.phonesync.app

import android.view.LayoutInflater
import android.view.ViewGroup
import androidx.core.content.ContextCompat
import androidx.recyclerview.widget.RecyclerView
import dev.phonesync.app.databinding.ItemLogBinding

class LogAdapter : RecyclerView.Adapter<LogAdapter.Holder>() {

    private var lines: List<SyncEngine.LogLine> = emptyList()

    fun submit(newLines: List<SyncEngine.LogLine>) {
        lines = newLines
        notifyDataSetChanged()
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): Holder {
        val binding = ItemLogBinding.inflate(
            LayoutInflater.from(parent.context), parent, false,
        )
        return Holder(binding)
    }

    override fun getItemCount() = lines.size

    override fun onBindViewHolder(holder: Holder, position: Int) {
        val line = lines[position]
        val b = holder.binding
        b.logText.text = line.text
        b.logMeta.text = line.meta

        val (icon, tint) = when (line.kind) {
            SyncEngine.LogKind.OK -> R.drawable.ic_check to R.color.good
            SyncEngine.LogKind.SKIP -> R.drawable.ic_check to R.color.muted
            SyncEngine.LogKind.BAD -> R.drawable.ic_alert to R.color.bad
        }
        b.logIcon.setImageResource(icon)
        b.logIcon.setColorFilter(ContextCompat.getColor(b.root.context, tint))
    }

    class Holder(val binding: ItemLogBinding) : RecyclerView.ViewHolder(binding.root)
}
