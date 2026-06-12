# ADR-0001 — Architecture hub / workers isolés par processus

- **Statut** : accepté
- **Date** : 2026-06-13
- **Décisions SPEC liées** : D-2.1, D-2.6, D-3.3 (specs §2.A, §2.C)

> Recopie des décisions SPEC dans le dépôt (PLAN tâche 0.6) pour que le code
> soit auto-porteur : un contributeur comprend l'architecture sans lire toute
> la SPEC. En cas de divergence, la SPEC fait foi.

## Contexte

Fluence est l'outil de communication de personnes qui n'en ont pas d'autre.
Le principe cardinal (SPEC §2.C) : **composer et vocaliser ne dépendent
JAMAIS de la santé des composants IA**. Or les bibliothèques d'inférence
natives (GGML, ONNX Runtime) crashent — c'est une réalité statistique, pas
une hypothèse. Un crash de lib native dans le processus du clavier rendrait
l'utilisateur muet.

## Décision

- `fluence-hub` (un seul processus vital) porte : superviseur, API HTTP/WS,
  moteur d'entrée/sélection, store. Les modèles IA tournent dans des
  **workers en processus enfants** (`worker-llm`, `worker-asr`, `worker-tts`,
  `worker-embed`), supervisés avec redémarrage à backoff exponentiel et
  événements `system.degraded` diffusés aux UI.
- IPC : UDS sur Linux, **named pipes sur Windows** (AF_UNIX n'est pas
  supporté par tokio sous Windows), abstraits par une couche commune ;
  messages JSON préfixés longueur (débogables) ; audio par ring buffer en
  mémoire partagée.
- Chaîne de dégradation explicite : LLM down → n-gram FR embarqué dans le
  hub ; TTS down → voix système OS ; hub down → watchdog (< 2 s) et draft
  restauré (write-ahead, perte ≤ 1 s).
- L'UI parle au hub **uniquement via l'API réseau locale**, même embarquée
  (D-2.1) : un seul chemin de code, le mode déporté est gratuit.
- Ordonnancement par priorités strictes (D-3.3) : P0 TTS > P1 suggestions
  (annulables par slot) > P2 ASR > P3 fond.

## Conséquences

- Toute PR qui touche le hub doit préserver les **kill-tests** (Phase 2) :
  tuer un worker ne doit jamais interrompre la frappe.
- Le hub reste petit et auditable ; la complexité IA vit dans des processus
  jetables.
- Coût assumé : une couche IPC à maintenir et tester (fuzzée en CI, D-9.3).
