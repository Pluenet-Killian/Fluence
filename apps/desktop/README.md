# apps/desktop — application de bureau Tauri v2

**À construire en Phase 7** (PLAN §2, tâche 7.1) : application double-clic qui embarque et
supervise le hub (watchdog < 2 s, autostart), installeurs signés MSI/NSIS (Windows) et
AppImage + deb (Linux).

Conformément à D-2.1, l'UI ne parlera au hub **que via l'API réseau locale** même embarquée
— un seul chemin de code, le mode déporté est gratuit.

Licence : AGPL-3.0-only (application complète, D-10.1).
