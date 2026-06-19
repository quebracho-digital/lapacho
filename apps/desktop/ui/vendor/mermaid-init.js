// Mermaid initialization and rendering bridge.
// Loaded as an external script so the page has no inline scripts, which
// allows a strict script-src CSP (no 'unsafe-inline' needed).
if (window.mermaid) {
  window.mermaid.initialize({
    startOnLoad: false,
    theme: "dark",
    securityLevel: "strict",
  });
}

// Called from Rust via request_animation_frame once the container is in the DOM.
// No-op when the bundle failed to load, so the diagram source stays visible.
window.renderMermaid = function (elementId, code) {
  var el = document.getElementById(elementId);
  if (!el || !window.mermaid || !code) return;
  try {
    window.mermaid
      .render(elementId + "-svg", code)
      .then(function (r) {
        var t = document.getElementById(elementId);
        if (t) t.innerHTML = r.svg;
      })
      .catch(function (err) {
        console.error(err);
        var t = document.getElementById(elementId);
        if (t)
          t.innerHTML =
            "<div class='mermaid-error'>Mermaid: no se pudo renderizar (revisá la sintaxis).</div>";
      });
  } catch (e) {
    console.error(e);
  }
};
