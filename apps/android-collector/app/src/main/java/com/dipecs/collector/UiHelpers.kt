package com.dipecs.collector

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

internal fun Context.sectionLabel(text: String): TextView =
    TextView(this).apply {
        this.text = text
        textSize = 13f
        setTextColor(Color.rgb(75, 85, 99))
        setPadding(0, 14, 0, 4)
    }

internal fun Context.rowButton(text: String, onClick: () -> Unit): Button =
    Button(this).apply {
        this.text = text
        setAllCaps(false)
        setOnClickListener { onClick() }
    }

internal fun Context.card(title: String, content: View): View =
    LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(24, 22, 24, 22)
        background = GradientDrawable().apply {
            setColor(Color.WHITE)
            cornerRadius = 16f
            setStroke(1, Color.rgb(226, 232, 240))
        }
        val params = LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT,
        )
        params.setMargins(0, 0, 0, 18)
        layoutParams = params

        addView(TextView(this@card).apply {
            this.text = title
            textSize = 17f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Color.rgb(17, 24, 39))
            setPadding(0, 0, 0, 10)
        })
        addView(content)
    }

internal fun Context.toast(message: String) {
    Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
}
