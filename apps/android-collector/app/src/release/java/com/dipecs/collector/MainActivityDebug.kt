package com.dipecs.collector

import android.widget.LinearLayout

/**
 * Release no-op for the debug-only manual AuthorizedAction panel. The release
 * build does not expose UI controls that can dispatch an AuthorizedAction
 * outside of the core lifecycle; only the authenticated localhost socket bridge
 * (used by [com.dipecs.collector.actions.AuthorizedActionSocketServer]) is
 * available in release.
 */
fun MainActivity.addAuthorizedActionCard(root: LinearLayout) {
    // No-op in release source set.
}
