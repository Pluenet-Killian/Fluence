# ADR-0009 — Audio de `/voice/speak` : WAV en v0, Opus différé au mode foyer

- **Statut** : accepté
- **Date** : 2026-06-14
- **Décisions SPEC liées** : voix (SPEC §6, D-6.1), API hub (§5.A), « une voix toujours » (§2.C), ADR-0007 (sous-processus plutôt que FFI sur windows-gnu) ; PLAN Phase 5.2.

## Contexte

Le contrat gelé déclarait la réponse de `POST /voice/speak` en **`audio/ogg; codecs=opus`** (streaming Opus, SPEC §5.A). La réalisation v0 du worker-tts est **Piper en sous-processus** (calque d'ADR-0007 : pas de C++/CMake dans le build, isolé par la frontière de processus, portable Win/Linux). Piper émet du **PCM brut 16 bits mono** ; produire de l'Opus/Ogg exige un encodeur libopus en **FFI** — exactement la classe de dépendance qui ne build pas de façon fiable sur `x86_64-pc-windows-gnu` (cause d'ADR-0007). 

Or, en v0, le hub est en mode **embarqué/loopback** (UI servie par le hub, même origine) : la compression Opus n'apporte rien (bande passante locale illimitée). Son intérêt — économiser la bande passante — est un besoin du **mode foyer/LAN** (Phase 7).

## Options considérées

1. **Encoder en Opus dès v0** (libopus FFI + muxer Ogg) — risque de build windows-gnu (ADR-0007), complexité audio (resampling 22050→48000, framing 20 ms, pages Ogg) pour zéro bénéfice en loopback.
2. **Streamer du WAV (PCM) en v0**, amender le contenu de `AudioStream` en `audio/wav` ; Opus différé au mode foyer (Phase 7). WAV est directement jouable par tout navigateur (`<audio>`/blob), sans dépendance ni FFI.
3. **Renvoyer 503 tant qu'Opus n'est pas câblé** — pas de voix, viole « une voix toujours » (§2.C).

## Décision

Nous choisissons l'**option 2**. La réponse de `/voice/speak` est **`audio/wav`** (RIFF/WAVE, 16 bits mono PCM) en v0 ; l'Opus/Ogg streaming est **différé à la Phase 7** (mode foyer/LAN), où la compression a un sens et où l'encodeur peut être évalué sur les machines de référence.

C'est une **correction méthodologique explicite** (PLAN §0.5 : un conflit contrat↔réalité se résout par amendement documenté, jamais en silence) et non un contournement : le `ResponseSpec::AudioStream` du registre passe à `audio/wav`, les goldens et l'`openapi.json` sont régénérés en conséquence.

## Conséquences

- **Plus fiable** : aucune dépendance FFI/codec, aucune compilation native ; la voix fonctionne sur Win/Linux dès le build.
- **Plus simple côté client** : le composeur joue le blob WAV directement.
- **Fidèle à « une voix toujours »** : le fallback OS (`SystemVoiceBackend` : SAPI/espeak-ng) émet aussi du WAV ; un seul format à jouer.
- **Dette / Phase 7** : streaming Opus/Ogg pour le mode foyer (bande passante LAN) ; vrai streaming par chunks (« premier échantillon < 200 ms » en contractuel) — en v0 Piper synthétise puis le hub streame le WAV complet (RTF ~0,07, donc rapide en pratique).
- **SPEC** : pas d'amendement SPEC nécessaire — §5.A décrit l'intention (audio streamé < 200 ms), réalisée ici en WAV ; le codec exact est un détail d'implémentation aligné par cet ADR.
