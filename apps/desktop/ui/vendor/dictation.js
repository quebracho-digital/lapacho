// The dictation overlay's three states, called by the backend with
// webview eval (src-tauri/src/voice.rs): listen → level… → recognizing →
// done(text) / fail(message). The waveform is the last levels heard,
// newest on the right.
(() => {
  const es = navigator.language.startsWith("es");
  const say = {
    listening: es ? "Escuchando… soltá para terminar" : "Listening… let go to finish",
    recognizing: es ? "Reconociendo…" : "Recognizing…",
    copied: es ? "Copiado: " : "Copied: ",
    nothing: es ? "No se entendió nada" : "Nothing understood",
  };
  const canvas = document.getElementById("wave");
  const ctx = canvas.getContext("2d");
  const text = document.getElementById("text");
  const bars = 30;
  let levels = [];

  function draw() {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#e0a86a";
    const w = canvas.width / bars;
    levels.forEach((l, i) => {
      // Speech peaks sit far below 1.0: a square root makes them visible.
      const h = Math.max(2, Math.min(1, Math.sqrt(l) * 2) * canvas.height);
      ctx.fillRect((bars - levels.length + i) * w + 1, (canvas.height - h) / 2, w - 2, h);
    });
  }
  function state(cls, message) {
    document.body.className = cls;
    text.textContent = message;
  }

  window.dictation = {
    listen() { levels = []; draw(); state("", say.listening); },
    level(l) { levels.push(l); if (levels.length > bars) levels.shift(); draw(); },
    recognizing() { state("busy", say.recognizing); },
    done(t) { state("done", t ? say.copied + t : say.nothing); },
    fail(message) { state("busy", message); },
  };
})();
