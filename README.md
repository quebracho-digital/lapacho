# Lapacho

Gestor de portapapeles **seguro** para escritorio. Clasifica, sanea y enmascara
lo que copiás antes de que llegue a la interfaz: las credenciales y los secretos
nunca se muestran en claro, y el contenido original solo sale cuando vos lo
pegás de vuelta.

Parte del ecosistema **Quebracho Digital**. Reemplaza a los prototipos
`quebracho-client` y `RustyBoard`.

> **Estado:** en desarrollo avanzado. Núcleo (`lapacho-core`) estable con 50+ tests.
> App de escritorio (Tauri 2 + Leptos/WASM) tiene backend completo, bandeja nativa,
> atajo global Ctrl+Shift+V, soporte de imágenes, render rico (MD/SVG/Mermaid) y
> modelo de seguridad raw-first. Verificado headless + smoke GUI.

## Características

- **Captura automática** del portapapeles mediante un monitor en segundo plano.
- **Clasificación de tipo:** texto, URL, JSON, SVG, Mermaid, Markdown.
- **Clasificación de sensibilidad:** `None` / `Personal` / `Credential` /
  `Secret`, usando regex + entropía de Shannon (claves privadas, tokens de API,
  tarjetas, emails, etc.).
- **Enmascarado:** credenciales y secretos se muestran redactados (`••••••••`),
  nunca en claro. El `raw_content` jamás se envía a la capa de UI.
- **Políticas de persistencia:**
  - `Paranoia` (default): solo guarda contenido no sensible.
  - `Balanceado`: guarda todo salvo secretos, con TTL para credenciales.
  - `Todo`: guarda todo.
- **Saneado de SVG:** elimina vectores XSS (scripts, handlers `on*`,
  `javascript:`).
- **Plugins de transformación:** comandos externos que reciben el contenido por
  `stdin` (sin inyección de comandos), con timeout, y cuya salida se sanea antes
  de mostrarse.
- **Historial en SQLite** (modo WAL) con límite de tamaño y limpieza por TTL.

## Arquitectura

Workspace Cargo (Rust, edición 2024):

```
lapacho/
├─ crates/lapacho-core/    # Lógica pura, sin UI (lib)
│  ├─ types        # ClipboardItem, UIClipboardItem (proyección segura), enums
│  ├─ detectors    # clasificación del tipo de contenido
│  ├─ security     # saneo + clasificación de sensibilidad
│  ├─ storage      # SQLite: historial, niveles de persistencia, TTL
│  ├─ plugins      # ejecución de plugins externos
│  └─ ingest       # pipeline que compone todo + enmascarado
└─ apps/desktop/
   ├─ src-tauri/   # backend Tauri 2 (monitor de portapapeles + comandos)
   ├─ ui/          # frontend Leptos/WASM (crate standalone, build con Trunk)
   └─ legacy-ui/   # UI vanilla original, conservada como referencia
```

`lapacho-core` no depende de Tauri ni de ningún framework de UI: es reutilizable
desde cualquier frontend.

## Modelo de seguridad

- El `raw_content` (contenido original) vive **solo en el backend**; la UI recibe
  un `UIClipboardItem` que nunca lo incluye.
- Credenciales y secretos se enmascaran con un placeholder de ancho fijo, que no
  filtra la longitud del original.
- Los plugins reciben su input por `stdin` (no por argumentos → sin inyección) y
  corren con timeout; su salida se sanea antes de llegar a la UI.

## Desarrollo

Requisitos: Rust ≥ 1.85. Para la app de escritorio, el toolchain de Tauri 2
(en Linux: `gtk3`, `webkit2gtk-4.1`, `libsoup-3.0`).

```bash
# Tests del núcleo
cargo test -p lapacho-core

# Compilar todo el workspace
cargo build --workspace

# Levantar la app de escritorio
cd apps/desktop/src-tauri
cargo tauri dev
```

## Roadmap

- [x] `lapacho-core`: clasificación, saneo, storage, plugins, ingest (50+ tests)
- [x] Backend de escritorio: monitor + comandos Tauri + keyring + hardening
- [x] Refresco reactivo de la UI ante captura en vivo
- [x] Cifrado en reposo (AES-256-GCM) + clave en keyring + endurecimiento en memoria
- [x] Escáner de amenazas modular + acciones por ítem (copiar/exportar/plugin)
- [x] Frontend Leptos/WASM + render rico (Markdown, SVG seguro, JSON, Mermaid)
- [x] Bandeja del sistema nativa + atajo global (Ctrl+Shift+V) + launch-to-tray
- [x] Soporte completo de imágenes (captura, thumbnails en tray, saneo metadata)

Detalle completo y pendientes menores: ver [`ROADMAP.md`](ROADMAP.md) y [`HANDOFF.md`](HANDOFF.md).

## License

The core (`lapacho-core`) and the app are released under **MIT OR Apache-2.0**.
Quebracho Digital's proprietary plugins and integrations are closed source.
See [`LICENSING.md`](LICENSING.md).
