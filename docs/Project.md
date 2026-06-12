# Logiciels de CAA et de contrôle par le regard pour la SLA et le handicap moteur lourd : cartographie, état des briques techniques, et LE meilleur projet open source à lancer

## TL;DR
- **Recommandation principale : construire un module open source de prédiction/accélération de la communication piloté par un LLM local — un « SpeakFaster libre, multilingue et offline » — interopérable, en commençant par AsTeRICS Grid (web, AGPL, activement maintenu).** C'est là que se situe le goulot d'étranglement réel (la *vitesse* de communication : ~8–10 mots/min en frappe oculaire contre ~150 en parole) et où aucune solution libre, française et locale n'existe aujourd'hui.
- L'écosystème libre est riche mais fragmenté : OptiKey (le plus connu, mais Windows-only et dormant depuis sept. 2024), AsTeRICS Grid (web, très actif), Dasher (en réécriture v6 web), des moteurs de regard webcam (WebGazer, EyeGestures, MediaPipe) nettement moins précis que l'infrarouge, et des TTS neuronaux libres (Piper, XTTS, F5-TTS) déjà capables de cloner une voix.
- Les deux autres idées à fort impact : (2) un **moteur de suivi du regard par webcam packagé comme service réutilisable** (calibration robuste + compensation de pose de tête + sortie standardisée), et (3) un **pipeline open source de banque/clonage de voix française on-device** (Piper/F5-TTS), aujourd'hui réservé à l'anglais et aux services propriétaires.

## Key Findings

**Le vrai goulot d'étranglement n'est pas l'entrée, c'est la vitesse.** La frappe oculaire produit « an extremely low text-entry speed of 8-10 words per minute » (Cai et al., *Context-Aware Abbreviation Expansion*, arXiv:2205.03767), contre ~150 mots/min en parole naturelle. Les travaux récents montrent que des LLM « context-aware » y remédient : **SpeakFaster** (collaboration Google Research–Team Gleason, Cai et al., *Nature Communications* 2024, s41467-024-53873-3) économise « 57% more motor actions than traditional predictive keyboards in offline simulation » et atteint des « text-entry rates 29–60% above baselines » chez « two eye-gaze AAC users with amyotrophic lateral sclerosis ». C'est l'avancée la plus directement actionnable — mais elle n'existe pas sous forme de brique libre, locale et multilingue.

**OptiKey est la référence libre mais montre des signes d'essoufflement.** Licence GPLv3, ~4,3k étoiles, C#/.NET, mais Windows-only (.NET 4.6, Windows 8/8.1/10). Dernière version **v4.1.1 de septembre 2024** (la note de release indique « re-releasing as 4.1.1 to prompt upgrades… Main change was fixing the bug when installing on a PC with default language set to Chinese »), apparemment **dormant depuis** (aucune activité 2025–2026 décelée), avec **121 issues ouvertes** et un mainteneur quasi-unique (**Julius Sweetland**). La localisation française existe (communautaire, via Transifex — l'app a été « localisée en plus de 25 langues » dont le français) mais reste incomplète selon les apps (« Optikey Symbol does not currently support all languages »). Forks notables : **EyeMine** (jouer à Minecraft au regard, SpecialEffect) et **OKGO**.

**AsTeRICS Grid est le meilleur socle libre vivant.** Application web (PWA) offline, **licence AGPL-3.0** (code) / CC BY-NC-SA 4.0 (grilles), multiplateforme (Windows/Linux/Android/iOS via navigateur), développée à la **UAS Technikum Wien** (mainteneur principal Benjamin Klaus / « klues », financement Ville de Vienne, projet InDiKo). **Releases très fréquentes** (la plus récente est `release-2026-06-03`, soit le 3 juin 2026 ; le dépôt a été renommé `asterics/Asterics-AAC`). Elle gère déjà clic, survol, scanning, contacteurs, prédiction de mots, symboles ARASAAC/OpenSymbols, domotique, YouTube/radio — **mais l'eye/head-tracking et l'EMG passent par le AsTeRICS Framework externe, pas en natif dans le web**.

**Le webcam-only reste nettement moins précis que l'infrarouge — un point à assumer honnêtement.** Les trackers IR Tobii atteignent **0,2–0,6° d'angle visuel** (Tobii Pro Fusion < 0,3° ; Tobii EyeX consumer < 0,6° accuracy / < 0,25° precision, Gibaldi et al. 2017), soit **≈ 3–6 mm d'erreur à 65 cm** (un degré ≈ 11 mm à cette distance). Les solutions webcam sont ~10× moins précises : WebGazer ≈ **4,17° / 4,06 cm** (IJCAI 2016 ; Frontiers 2024), avec dégradation « from approximately 5 to 10 cm during a 20-minute test » sans correction de pose de tête. Les nouveaux modèles CNN réduisent l'écart sans le combler : **WebEyeTrack** (Davalos et al., arXiv:2508.19544, 27 août 2025) atteint « an error margin of 2.32 cm on GazeCapture and real-time inference speeds of 2.4 milliseconds on an iPhone 14 » avec calibration few-shot (« as few as nine calibration samples »). **Conséquence : le webcam-only convient aux interfaces à grandes cibles et tolérantes au bruit (Dasher, scanning, grilles à grandes cases), pas à un clavier QWERTY dense.**

**Le clonage de voix est devenu trivial techniquement, mais la voix française personnelle reste sous-desservie en libre.** **XTTS-v2** (Coqui) « clone voices into different languages by using just a quick 6-second audio clip », supporte 17 langues dont le français, avec « streaming inference with < 200ms latency » — *mais* sous **licence non commerciale (Coqui Public Model License)** et la société Coqui a fermé le 4 janvier 2024 (projet poursuivi par la communauté sous `coqui-tts`). **Piper** (OHF-Voice/piper1-gpl) offre un TTS local rapide (temps réel sur Raspberry Pi) avec voix françaises (siwis, tom, upmc), ~60 Mo, sans clonage zero-shot natif. **F5-TTS** (licence MIT) fait du clonage zero-shot à partir de ~3 s ; **StyleTTS 2** (MIT) atteint une qualité quasi-humaine en anglais. **Apple Personal Voice** clone on-device, chiffré, mais lié à l'écosystème Apple. Services de référence (propriétaires/payants) : Acapela My-Own-Voice, ModelTalker (100 $), The Voice Keeper, SpeakUnique, CereProc — environ 80–95 % de leurs voix banquées concernent des personnes SLA.

**La BCI progresse spectaculairement mais n'est pas un terrain pour un projet logiciel libre solo.** Card et al. (*NEJM*, 14 août 2024, NEJMoa2314132 ; participant ALS Casey Harrell) ont obtenu un décodage soutenu « 97.5% accuracy over a period of 8.4 months after surgical implantation… at a rate of approximately 32 words per minute » sur un vocabulaire de 125 000 mots ; une mise à jour UC Davis (juin 2025) a démontré la synthèse vocale en quasi temps réel. Mais c'est de la **recherche clinique invasive**. Les BCI non invasifs (EEG) souffrent encore d'« illettrisme BCI » (15–30 % des utilisateurs) et de fatigue. À situer comme horizon, pas comme cible — d'autant qu'Apple a annoncé en 2025 un protocole BCI pour Switch Control, qui standardisera l'entrée.

## Details

### 1) Cartographie des logiciels existants

**Open source — claviers/contrôle par le regard :**
- **OptiKey** (github.com/OptiKey/OptiKey) — GPLv3, C#/.NET, **Windows uniquement**. Suite de 4 apps (Pro, Mouse, Chat, Symbol). Compatible Tobii et trackers émulant la souris ; webcam possible mais marginale. v4.1.1 (sept. 2024), dormant depuis ; 121 issues ouvertes ; mainteneur Julius Sweetland ; >25 langues (français inclus, qualité variable). Limites : Windows-only, dépendance à un tracker, occupation d'écran, maintenance fragile.
- **AsTeRICS Grid** (asterics/AsTeRICS-Grid → Asterics-AAC) — AGPL-3.0, web/PWA, offline, multiplateforme. CAA par grilles + symboles, prédiction de mots, domotique, médias, messagerie Matrix. Input : clic/survol/scanning/contacteurs/clavier ; eye/head/EMG via AsTeRICS Framework. Très actif (release juin 2026). **Le socle le plus pérenne.**
- **Dasher** (dasher-project/dasher, ACE Centre) — GPL, entrée de texte par zoom + modèle de langage, pilotable au regard/tête/contacteur, 60+ langues, ~25 mots/min au regard pour un utilisateur expérimenté. v5 (C++) mature ; **v6 en réécriture (JavaScript, web)**, en cours. Très tolérant à l'imprécision → compatible webcam. En cours de relicenciation MIT.
- **GazeSpeaker** — gratuit (statut open source non clair), Windows, 28 langues dont français, grilles ARASAAC, trackers Tobii/EyeTribe/ITU. Activité récente faible.
- **Cboard**, **FreeSpeech AAC**, **Pasco** (ACE Centre), **Leeloo AAC**, **PiCom** — projets CAA libres surtout symboles/web (cf. OpenAAC.org).
- **Moteurs de regard libres** : **WebGazer.js** (Brown, JS navigateur, ~4° de précision), **EyeGestures** (NativeSensors, Python + port JS EyeGesturesLite, basé MediaPipe), **OpenGazer** (ancien), **GazePointer/GazeFlowAPI** (freemium), **Opentrack** (head-tracking, très actif, 3,6k★), **Miranda** (eyes-on-disabilities — calibration regard/tête→écran→OptiKey via UDP), **EyeWriter/openEyes** (hardware DIY historique). Listes de référence : **eyes-on-disabilities/awesome-eye-tracking** (Codeberg) et **openassistive/awesome-assistivetech**.
- **Briques de prédiction libres** : **Presage** (C++), **Asterics-Predictionary** (JS), API d'Imagineville/Vertanen.

**Commerciales (état de l'art, pour situer) :**
- **Tobii Dynavox** — leader. Matériel (TD I-13/I-16, TD Pilot, PCEye 5) + logiciels TD Snap (symboles), Communicator 5 (texte), TD Control (Windows au regard), TD Talk. Eye-tracking IR sub-degré, usage extérieur, voix personnelle synthétique. Dispositifs dédiés coûteux (devis sur demande ; généralement plusieurs milliers d'euros, souvent financés).
- **Smartbox Grid 3** — suite Windows complète, multimodale (regard/contacteur/tactile), très répandue.
- **Control Bionics NeuroNode** — accès EMG + eye-tracking combinés.
- Repères français : **JIB EYES**, **Cenomy** (revendeurs Tobii), **HappyCAA** — accompagnement par ergothérapeutes, matériel reconditionné pour réduire le coût.

### 2) État des briques techniques (2025–2026)

**Suivi du regard webcam :** MediaPipe Face Landmarker/Iris fournit les landmarks iris/pupille en temps réel sans matériel dédié (erreur de distance < 10 %). Mais l'estimation du point de regard à l'écran reste imprécise (~2–4 cm) et sensible à la lumière, aux lunettes, à la pose de tête et à la calibration. État de l'art : ETH-XGaze, L2CS-Net, GazeML, et CNN récents type WebEyeTrack (few-shot, on-device, navigateur, 2,32 cm). **Verdict : utilisable pour interfaces tolérantes (Dasher, scanning, grosses cases), pas pour clavier dense.**

**Head-tracking & modalités résiduelles :** Opentrack + AITrack/webcam, capteurs de tête, clignement (MediaPipe), EMG (NeuroNode, AsTeRICS), contacteur unique. Essentiel pour l'adaptation à la progression (regard → tête → contacteur → BCI).

**Scanning à un contacteur :** indispensable aux stades très avancés ; bien géré par AsTeRICS Grid et Grid 3. Nomon offre une alternative au scanning, plus rapide (un utilisateur expérimenté ~9,3 mots/min, 1,2 clic/caractère).

**TTS neuronal & banque/clonage de voix :**
- **Piper** : local, rapide, voix FR (siwis/tom/upmc), pas de clonage zero-shot natif.
- **XTTS-v2** : clonage 6 s, 17 langues dont FR, < 200 ms streaming ; **licence CPML non commerciale** ; Coqui fermé 04/01/2024.
- **F5-TTS** (MIT) : clonage zero-shot ~3 s ; **StyleTTS 2** (MIT) : quasi-humain en anglais.
- **Apple Personal Voice** : clonage on-device chiffré, écosystème Apple, usage via API AAC.
- Services propriétaires : Acapela My-Own-Voice, ModelTalker, The Voice Keeper, SpeakUnique, CereProc, VocaliD.

**Prédiction & LLM :** **SpeakFaster** (dépôt GitHub TeamGleason/SpeakFaster, peu actif) : expansion d'abréviation context-aware, +29–60 % de vitesse en conditions réelles SLA (Nature Comms 2024). **Yusufali et al.** (Univ. Sheffield, HCI International 2023 / ICASSP 2024) : « used LLMs to generate word predictions for AAC devices and showed entry rates up to 30.4 WPM » (rapporté par Shoaib et al., arXiv:2501.10582). Petits modèles locaux (Llama, Mistral 7B, Phi, Qwen) désormais exécutables sur machine grand public (llama.cpp, transformers.js).

**BCI :** UC Davis/BrainGate — 97,5 % de décodage soutenu, ~32 mots/min, vocabulaire 125 000 mots (NEJM 2024) ; synthèse vocale quasi temps réel (2025). Invasif, recherche clinique. EEG non invasif encore limité. **Pas une cible pour un projet libre solo**, mais protocole d'entrée à anticiper (Apple BCI/Switch Control 2025).

### 3) Besoins réels non couverts (le plus important)

D'après les retours patients/aidants/cliniciens (ALS Association, Team Gleason, MND Association, blogs d'aidants, publications EyeO/arXiv 2307.15039, Frontiers Neurology 2018) :
- **Vitesse de communication** = frustration n°1 ; la frappe oculaire est lente et fatigante.
- **Calibration & installation** : « human debugging » constant par les aidants ; sensibilité lumière/lunettes/posture ; recalibration fréquente (« There are so many situations where you need someone to figure out what you see from the calibration »).
- **Français et langues non-anglaises** sous-desservis dans le libre (prédiction, voix, grilles).
- **Multiplateforme / web** : la plupart des solutions libres performantes sont Windows-only (OptiKey, GazeSpeaker, Grid 3).
- **Coût/accessibilité matériel** : trackers IR et dispositifs dédiés chers ; le webcam-only séduisant mais pas assez fiable seul.
- **Adaptation à la progression** : peu d'outils gèrent en continu le passage regard → tête → contacteur → BCI dans une même interface.
- **Voix personnelle** : voice/message banking à faire tôt ; solutions libres françaises quasi inexistantes.
- **Abandon/maintenance** : risque récurrent (OptiKey dormant, Coqui fermé, AraBoard disparu). Un projet « freely accessible » pérenne (single-page, offline, faible coût serveur) est explicitement réclamé (étude AsTeRICS Grid, Springer 2024).

### 4) Synthèse et recommandation

**Construire de A à Z vs contribuer vs brique manquante ?**
- *De A à Z* : énorme effort, risque de réinventer AsTeRICS Grid/OptiKey et d'abandon. Déconseillé.
- *Moderniser OptiKey* : utile mais l'architecture Windows/.NET et la dormance limitent l'impact ; mieux vaut porter ses idées vers le web.
- *Brique manquante interopérable* : **meilleur rapport impact/effort**, surtout greffée sur l'écosystème vivant (AsTeRICS Grid, Dasher v6).

**IDÉE N°1 (RECOMMANDÉE) — « OpenSpeakFaster » : moteur libre de prédiction/accélération de communication par LLM local, multilingue (FR inclus), interopérable.**
- *Pourquoi le plus d'impact* : attaque directement le goulot (vitesse), gain prouvé de 29–60 %, et aucune brique libre, locale et française n'existe. Bénéficie à TOUTES les interfaces (regard, tête, contacteur, scanning).
- *Périmètre MVP* : bibliothèque + service local exposant une API standard : (a) expansion d'abréviations context-aware, (b) prédiction de phrases/mots, (c) complétion tenant compte du tour de conversation. Premier branchement : **plugin/clavier prédictif dans AsTeRICS Grid** (web, AGPL) + démonstrateur Dasher v6.
- *Choix techniques* : TypeScript/JS côté Grid ; petit LLM local (llama.cpp / transformers.js / ONNX, modèle Phi/Qwen/Mistral quantifié) avec fallback API optionnel ; corpus FR ; confidentialité by design (tout on-device). Interop : OpenBoardFormat pour les grilles, API de prédiction documentée.
- *Défis* : latence sur machine modeste, qualité FR, intégration UI (afficher les bonnes prédictions au bon moment sans ralentir), évaluation avec utilisateurs réels.
- *Jalon 1* : expansion d'abréviation FR context-aware dans le navigateur, offline, intégrée comme source de prédiction dans AsTeRICS Grid.

**IDÉE N°2 — « OpenGaze service » : moteur de suivi du regard par webcam packagé comme brique réutilisable.**
- Calibration robuste + compensation de pose de tête + sortie standardisée (UDP/WebSocket, compatible Opentrack/Miranda/OptiKey), en navigateur (MediaPipe + modèle type WebEyeTrack) et en natif.
- *Pourquoi* : démocratise l'accès sans matériel IR coûteux ; comble le manque d'un tracker webcam fiable et open source packagé pour la CAA (pas seulement une lib de recherche).
- *Réalité* : ne remplacera pas l'IR pour un clavier dense (≈ 2–4 cm vs 3–6 mm) ; à coupler avec des interfaces tolérantes (Dasher, grosses cases, scanning hybride). Honnêteté sur la précision indispensable.

**IDÉE N°3 — « VoixLibre » : pipeline open source de banque/clonage de voix française on-device.**
- Enregistrement guidé (message + voice banking) → clonage/fine-tuning avec Piper (FR) ou F5-TTS (MIT) → voix utilisable dans AsTeRICS Grid/OptiKey, 100 % local, RGPD-friendly.
- *Pourquoi* : la voix personnelle française libre n'existe quasiment pas ; les services sont propriétaires/payants.
- *Défis* : qualité FR, effort d'enregistrement minimal, intégration AAC.

**Conclusion : l'idée n°1 (OpenSpeakFaster) est la plus différenciante et la plus utile.** Elle s'attaque au problème le plus douloureux (vitesse), capitalise sur la recherche récente (gains prouvés), reste faisable en local et multilingue, et profite à tout l'écosystème via l'interopérabilité. Les idées n°2 et n°3 sont d'excellents projets complémentaires si l'auteur préfère le traitement d'image ou l'audio.

## Recommendations
1. **Commencer par OpenSpeakFaster, greffé sur AsTeRICS Grid** (socle web, AGPL, vivant). Livrer un MVP : expansion d'abréviation FR context-aware, offline, intégrée comme source de prédiction de la grille.
2. **Valider tôt avec des utilisateurs réels** (ARSLA en France, MND Association, ergothérapeutes/orthophonistes). Critère de succès : gain mesurable de mots/minute ou d'actions économisées (benchmark : viser un gain ≥ 30 % comme SpeakFaster).
3. **Choisir des licences permissives pour les modèles** : éviter XTTS/CPML pour la prod ; préférer Piper, F5-TTS (MIT), ou LLM sous licences exploitables (Qwen, Llama selon conditions).
4. **Publier en interopérable** (OpenBoardFormat, API de prédiction documentée) pour bénéficier aussi à Dasher v6 et, idéalement, à un futur OptiKey-web.
5. **Seuils de bascule** : si la précision webcam franchit ~1° de façon robuste en conditions réelles → l'idée n°2 devient prioritaire. Si une voix FR clonée de qualité émerge en libre → l'idée n°3 perd en urgence. Si OptiKey reprend une maintenance active et migre vers le web → envisager d'y contribuer plutôt que de dupliquer.

## Caveats
- Les gains SpeakFaster (Nature Comms 2024) reposent sur l'hypothèse d'un contexte conversationnel disponible aux LLM, avec des enjeux de confidentialité et de praticité encore en investigation ; résultats validés sur **2 utilisateurs SLA seulement** en conditions réelles.
- Les chiffres de précision webcam (WebEyeTrack ~2,32 cm) viennent d'un **preprint 2025 sur datasets (GazeCapture)**, pas de la frappe AAC réelle ; en conditions réelles (lumière, mouvements de tête) c'est moins bon. Les chiffres IR sont mesurés en labo (mentonnière).
- XTTS-v2 est sous licence **non commerciale (CPML)** et Coqui a fermé début 2024 ; vérifier les licences avant toute mise en production (préférer F5-TTS/Piper).
- OptiKey paraît **dormant depuis septembre 2024** ; à reconfirmer avant d'investir dessus.
- La BCI invasive relève de la **recherche clinique** et n'est pas un terrain réaliste pour un projet logiciel libre individuel.
- Les prix exacts du matériel Tobii Dynavox ne sont pas publiés (devis sur demande) ; « plusieurs milliers d'euros » est une estimation issue de revendeurs.
- AsTeRICS Grid : l'eye/head-tracking n'est pas natif dans l'app web (il passe par le AsTeRICS Framework), ce qui ouvre justement une opportunité d'intégration plus fluide.