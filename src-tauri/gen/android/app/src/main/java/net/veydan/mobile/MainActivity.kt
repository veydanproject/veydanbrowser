package net.veydan.mobile

import android.graphics.Color
import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature

class MainActivity : TauriActivity() {
  private var lastBars: Insets = Insets.NONE
  private var lastImeBottom: Int = 0
  private var insetsScriptHandler: ScriptHandler? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    // Light icons/scrims until JS theme bridge reports the actual theme.
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    webView.addJavascriptInterface(ChromeBridge(), "VeydanChrome")

    ViewCompat.setOnApplyWindowInsetsListener(webView) { v, windowInsets ->
      lastBars = windowInsets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
      )
      lastImeBottom = windowInsets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      injectInsets(v as WebView, lastBars, lastImeBottom)
      windowInsets
    }
    ViewCompat.requestApplyInsets(webView)
  }

  private fun injectInsets(webView: WebView, bars: Insets, imeBottom: Int) {
    val d = webView.resources.displayMetrics.density
    val top = bars.top / d
    val right = bars.right / d
    val bottom = bars.bottom / d
    val left = bars.left / d
    val kb = imeBottom / d
    val script =
      """
      (function(){
        var s=document.documentElement.style;
        s.setProperty('--sat','${top}px');
        s.setProperty('--sar','${right}px');
        s.setProperty('--sab','${bottom}px');
        s.setProperty('--sal','${left}px');
        s.setProperty('--kb','${kb}px');
        window.dispatchEvent(new Event('veydan-keyboard'));
      })();
      """.trimIndent()

    if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
      insetsScriptHandler?.remove()
      insetsScriptHandler = WebViewCompat.addDocumentStartJavaScript(webView, script, setOf("*"))
    }
    webView.evaluateJavascript(script, null)
  }

  private fun applyLightBars(light: Boolean) {
    val controller = WindowCompat.getInsetsController(window, window.decorView)
    controller.isAppearanceLightStatusBars = light
    controller.isAppearanceLightNavigationBars = light
  }

  inner class ChromeBridge {
    @JavascriptInterface
    fun setLightBars(light: Boolean) {
      runOnUiThread { applyLightBars(light) }
    }
  }
}
