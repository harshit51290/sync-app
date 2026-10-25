package dev.phonesync.app

import android.content.ContentUris
import android.content.Context
import android.net.Uri
import android.provider.MediaStore

enum class MediaKind { IMAGE, VIDEO, AUDIO, DOCUMENT }

data class MediaItem(
    val uri: Uri,
    val name: String,
    val relPath: String,
    val folder: String,
    val size: Long,
    val mtimeMs: Long,
    val kind: MediaKind,
) {
    /// Identifies one file version; the laptop still de-duplicates by hash.
    val key: String get() = "$relPath|$size|$mtimeMs"
}

data class FolderEntry(val path: String, val count: Int)

/// Reads what the phone actually has, through MediaStore — no folder walking,
/// no special storage permission beyond the media ones.
object MediaScanner {

    fun scan(context: Context): List<MediaItem> {
        val items = ArrayList<MediaItem>(512)
        items += query(context, MediaStore.Images.Media.EXTERNAL_CONTENT_URI, MediaKind.IMAGE)
        items += query(context, MediaStore.Video.Media.EXTERNAL_CONTENT_URI, MediaKind.VIDEO)
        items += query(context, MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, MediaKind.AUDIO)
        items += query(context, MediaStore.Downloads.EXTERNAL_CONTENT_URI, MediaKind.DOCUMENT)
        return items.sortedBy { it.mtimeMs }
    }

    fun folders(items: List<MediaItem>): List<FolderEntry> = items
        .groupingBy { it.folder }
        .eachCount()
        .map { (path, count) -> FolderEntry(path, count) }
        .sortedWith(compareByDescending<FolderEntry> { it.count }.thenBy { it.path })

    private fun query(context: Context, collection: Uri, kind: MediaKind): List<MediaItem> {
        val projection = arrayOf(
            MediaStore.MediaColumns._ID,
            MediaStore.MediaColumns.DISPLAY_NAME,
            MediaStore.MediaColumns.SIZE,
            MediaStore.MediaColumns.DATE_MODIFIED,
            MediaStore.MediaColumns.RELATIVE_PATH,
        )
        val result = ArrayList<MediaItem>()
        runCatching {
            context.contentResolver.query(
                collection,
                projection,
                "${MediaStore.MediaColumns.SIZE} > 0",
                null,
                "${MediaStore.MediaColumns.DATE_MODIFIED} ASC",
            )?.use { cursor ->
                val idColumn = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
                val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.DISPLAY_NAME)
                val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.SIZE)
                val dateColumn = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.DATE_MODIFIED)
                val relColumn = cursor.getColumnIndex(MediaStore.MediaColumns.RELATIVE_PATH)

                while (cursor.moveToNext() && result.size < MAX_FILES) {
                    val id = cursor.getLong(idColumn)
                    val name = cursor.getString(nameColumn) ?: continue
                    val size = cursor.getLong(sizeColumn)
                    if (size <= 0) continue
                    val rel = if (relColumn >= 0) cursor.getString(relColumn).orEmpty() else ""
                    val folder = rel.trimEnd('/').ifBlank { "Other" }
                    result.add(
                        MediaItem(
                            uri = ContentUris.withAppendedId(collection, id),
                            name = name,
                            relPath = rel,
                            folder = folder,
                            size = size,
                            mtimeMs = cursor.getLong(dateColumn) * 1000L,
                            kind = kind,
                        ),
                    )
                }
            }
        }
        return result
    }

    private const val MAX_FILES = 20_000
}
