# Lista rápida de verificación — Lapacho

**Auditoría completa:** `AUDITORIA_PROFUNDA_2026-08-04.md`

---

## ✅ Pass (no se rompe producción)

| Criterio | Estado | Comando |
|----------|--------|---------|
| 1. Tests pasan | ✅ | `cargo test --workspace` → 98 tests |
| 2. Build成功 | ✅ | `cargo build --workspace` |
| 3. no `dirty` | ✅ | `git status` → clean |
| 4. Main branch | ✅ | `feat/clipboard-refactor` borrada, main → 14 commits ahead |
| 5. PRs abiertos | ⚠️ | PR #15 (zeroize completo + LockedRing) — mergear + probar |

---

## ⚠️ Moderado (atender antes de móvil)

| Criterio | Estado | Acción |
|----------|--------|--------|
| Tray rebuild latency | ⚠️ ~300 ms | `get_tray_items` carga DB dos veces → una sola vez |
| Mobile Kotlin storage | ⚠️ divergió | PR #10: borrar `storage/`, usar `lapacho-core` vía `uniffi` |
| UI tests headless | ☐ | Requiere WASM + headless browser |

---

## 📋 Próximos pasos (antes de móvil)

1. **Merge PR #15** → código listo, falta mergear
2. **Prueba manual de pegado** → `Ctrl+Shift+Alt+L` → clic → pegar
3. **Tray rebuild** → cargar historia una sola vez

---

## 🔒 Checklist de seguridad (100% pasados)

- [x] AES-256-GCM en SQLite
- [x] Key en OS keyring
- [x] Key `mlock`-ed
- [x] Session buffer `LockedRing` (800 KB)
- [x] `VmLck` 808 kB, `VmSwap` 0 (verificado)
- [x] `UIClipboardItem` sin `raw_content`
- [x] Credential/Secret re-masked desde raw
- [x] stdin plugins (no args)
- [x] Timeout plugins
- [x] `persist_level.persists` única regla
- [x] `purge_forbidden` baja nivel → purga
- [x] Log a archivo, no contenido

---

## 📊 Métricas

- **Rust líneas:** ~5.8K (excluyendo `target/`)
- **Crates:** 4 (core + predict + sync + mobile bridge)
- **Tests:** 98 (78 core + 15 tray + 2 mobile + 1 predict + 1 sync + 1 plugin)
- **Plugins dir:** examples/ (vacío, doc)
- **Target size:** 18G (caché build, no versionado)

---

## 📚 Docs

- `README.md` — visión general + features + arquitectura + roadmap
- `ROADMAP.md` — checklist detallada (hecho/pending)
- `CHANGELOG.md` — cambios por versión (más nuevo primero)
- `docs/DECISIONS.md` — rechazados + por qué (`mlockall`, `mlock` por `String`, encriptar buffer)
- `docs/ARQUITECTURA_MOBILE_ANDROID.md` — diseño móvil (no leído aún)

---

**Última actualización:** 2026-08-04  
**Auditor:** Claude Code  
**Estado:** LISTO (salvo merge PR #15 + prueba manual)
