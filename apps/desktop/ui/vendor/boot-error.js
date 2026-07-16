// Visible diagnostics when the WASM UI fails to mount (black window).
// External script so CSP does not need 'unsafe-inline'.
(function () {
  function show(msg) {
    try {
      var el = document.getElementById("lapacho-boot-error");
      if (!el) {
        el = document.createElement("pre");
        el.id = "lapacho-boot-error";
        el.style.cssText =
          "margin:0;padding:16px;white-space:pre-wrap;color:#e89090;background:#3a2020;font:12px/1.4 monospace;z-index:99999;position:relative";
        document.body.insertBefore(el, document.body.firstChild);
      }
      el.textContent = (el.textContent ? el.textContent + "\n" : "") + msg;
    } catch (_) {}
  }

  window.addEventListener("error", function (e) {
    show("error: " + (e.message || String(e.error || e)));
  });
  window.addEventListener("unhandledrejection", function (e) {
    show("rejection: " + String(e.reason));
  });

  // If Leptos never mounts, surface a clear message (header is only in WASM).
  window.setTimeout(function () {
    if (!document.querySelector("header") && !document.getElementById("controls")) {
      show(
        "Lapacho UI did not mount after 3s (WASM/JS).\n" +
          "If you only see a black window, try WEBKIT_DISABLE_DMABUF_RENDERER=1."
      );
    }
  }, 3000);
})();
