package dev.phonesync.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.View
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.fragment.app.Fragment
import dev.phonesync.app.databinding.ActivityMainBinding

class MainActivity : AppCompatActivity() {

    private lateinit var binding: ActivityMainBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)

        if (savedInstanceState == null) {
            supportFragmentManager.beginTransaction()
                .replace(R.id.navHost, SyncFragment())
                .commit()
        }

        binding.bottomNav.setOnItemSelectedListener { item ->
            val target: Fragment = when (item.itemId) {
                R.id.nav_folders -> FoldersFragment()
                R.id.nav_settings -> SettingsFragment()
                else -> SyncFragment()
            }
            supportFragmentManager.beginTransaction()
                .replace(R.id.navHost, target)
                .commit()
            true
        }

        requestPermissionsIfNeeded()
    }

    override fun onResume() {
        super.onResume()
        refreshChip()
    }

    fun refreshChip() {
        val paired = App.prefs.isPaired()
        binding.statusChip.text =
            getString(if (paired) R.string.chip_paired else R.string.chip_unpaired)
        binding.statusChip.setTextColor(
            ContextCompat.getColor(this, if (paired) R.color.lime else R.color.muted),
        )
    }

    fun openTab(itemId: Int) {
        binding.bottomNav.selectedItemId = itemId
    }

    private fun requestPermissionsIfNeeded() {
        val wanted = mutableListOf<String>()
        if (Build.VERSION.SDK_INT >= 33) {
            wanted += Manifest.permission.READ_MEDIA_IMAGES
            wanted += Manifest.permission.READ_MEDIA_VIDEO
            wanted += Manifest.permission.POST_NOTIFICATIONS
        } else {
            wanted += Manifest.permission.READ_EXTERNAL_STORAGE
        }
        val missing = wanted.filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isNotEmpty()) {
            requestPermissions(missing.toTypedArray(), REQUEST_PERMISSIONS)
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQUEST_PERMISSIONS) {
            val current = supportFragmentManager.findFragmentById(R.id.navHost)
            if (current is FoldersFragment) current.load()
        }
    }

    private companion object {
        const val REQUEST_PERMISSIONS = 7
    }
}
