# ADR-0006 — Harnais d'évaluation (Phase 3 « La boussole »)

- **Statut** : accepté
- **Date** : 2026-06-13
- **Décisions SPEC liées** : D-8.1 (harnais §8.A), D-8.2 (sources données), D-8.3 (FluenceBench-FR), D-5.5 (pipeline corpus §5.D), D-2.6 (fallback n-gram)

## Contexte

La Phase 3 du PLAN construit la **boussole** : le harnais de simulation qui mesure
les frappes économisées (KS%) et le WPM simulé *avant* qu'on construise le moteur IA
(Phase 4). Sans elle, chaque gain revendiqué serait une opinion. La SPEC §8.A en fait
« une infrastructure centrale, pas un outil annexe », branchée en CI sur chaque commit.

Forces en tension :

1. **Langage** : le harnais vit en Python (`ml/eval`, écosystème éval/ML), mais le
   n-gram de secours (D-2.6) est embarqué dans le **hub Rust** et doit y tourner. La
   SPEC exige que le harnais mesure le **vrai** fallback, pas une réimplémentation
   Python qui dériverait.
2. **Public vs interne** : le corpus et le format de dialogue seront publiés
   (FluenceBench-FR, D-8.3, CC BY-SA) ; le format doit donc être versionné et stable,
   pas un détail interne.
3. **Déterminisme** : la porte de CI « régression KS% > 2 points = échec » (§8.A)
   n'a de sens que si le même seed produit les mêmes chiffres, **sur Windows comme sur
   Linux** (DoD). L'utilisateur simulé a pourtant du hasard (bruit moteur, dwell).
4. **Volumes** : §8.A vise « PR 50 dialogues / nightly 500+ » et « corpus FR v1 » ;
   le PLAN Phase 3 vise « PR 20 / corpus v0 ~100 ». Ce ne sont pas les mêmes échéances.
5. **Teacher** : §5.D génère les dialogues « par grand modèle ». Aucun teacher LLM
   n'est disponible localement sur la machine de dev (contrainte matérielle).

## Options considérées

### A. Partage du n-gram entre hub et harnais

1. **Réimplémenter le n-gram en Python pour l'éval** — simple à build, mais deux
   implémentations divergent : l'éval mesurerait un fallback *différent* de celui
   livré. Rejeté (contredit §8.A).
2. **Crate Rust unique + binding PyO3/maturin** — une seule logique, mesurée telle
   qu'elle tourne dans le hub ; coût : build maturin dans le job CI Python.
3. **CLI Rust appelée en sous-processus par l'éval** — pas de binding, mais un
   sous-processus par prédiction (milliers d'appels) est trop lent en offline.

### B. Où vivent les formats

1. Tout dans `fluence_eval`. 2. Dialogue/corpus dans `fluence_data`, résultat dans
   `fluence_eval` (l'éval dépend de data). 3. Schémas dans le contrat Rust
   (`fluence-protocol`).

### C. Corpus v0

1. **Généré par teacher LLM** maintenant — fidèle à §5.D mais bloqué (pas de teacher).
2. **Graine écrite à la main** (quelques dialogues par registre) + générateurs de
   variantes déterministes, le pipeline teacher arrivant quand un teacher est dispo.
3. Reporter tout le corpus — mais alors le harnais n'a rien à mesurer en Phase 3.

## Décision

- **A → option 2.** `fluence-ngram` est un **crate Rust** (API = source de prédiction,
  réutilisable par le hub en Phase 4) **avec un binding PyO3** exposé à `ml/eval` via
  maturin. Le harnais mesure exactement le binaire embarqué. Le job CI Python gagne une
  étape de build du binding (mise en cache).
- **B → option 2.** Le format de **dialogue/corpus** (public, FluenceBench-FR) vit dans
  `fluence_data` ; le format de **résultat d'éval** dans `fluence_eval`, qui dépend de
  `fluence_data`. Chaque format porte un `schema_version` entier ; un changement
  incompatible l'incrémente (les goldens du bench public en dépendent).
- **C → option 2.** Le corpus **v0 est écrit à la main** sur la matrice 12 situations ×
  4 registres, sous grille anti-pathos, avec splits gelés ; les **générateurs de
  variantes** (télégraphique/bruitée AZERTY/abrégée) sont du code déterministe et réel.
  La génération par teacher LLM (§5.D étage 1) est **différée en dette** jusqu'à
  disponibilité d'un teacher.
- **Déterminisme** : toute la comptabilité du harnais est en **arithmétique entière**
  (frappes = compteurs ; temps = millisecondes entières), seul le ratio final (KS%, WPM)
  est flottant. Le hasard passe par un PRNG explicitement seedé (`random.Random(seed)`),
  aucune fonction transcendante sur le chemin des métriques → résultats identiques
  Win/Linux. Les tests assertent sur les **compteurs entiers** exacts.
- **Volumes (§8.A vs PLAN)** : pas une contradiction mais un **staging v0→v1**. Phase 3
  livre les valeurs v0 du PLAN (PR 20 / corpus ~100) ; les cibles §8.A (PR 50 / nightly
  500+ / KS% ≥ 25 %) restent l'horizon v1, atteint quand le corpus teacher arrive. Aucun
  amendement SPEC : §8.A décrit la cible mûre, le PLAN décrit le chemin.

## Conséquences

- **Plus simple** : une seule vérité pour le n-gram ; un format de corpus stable et
  publiable ; des métriques reproductibles donc des portes de CI crédibles.
- **Plus contraint** : le job CI Python doit disposer du toolchain Rust + maturin pour
  bâtir le binding (surveillé par le temps de job, < 15 min) ; le binding est importé
  paresseusement pour que les tests purs (formats/métriques) tournent sans lui.
- **Dette créée** : génération du corpus par teacher LLM (§5.D étage 1) → issue `debt` ;
  acceptation **sémantique** des suggestions (embeddings locaux) → branchée en interface
  mais implémentée en Phase 4 quand le modèle d'embeddings arrive (le v0 ne mesure que
  les modes lettre-à-lettre et prédiction, lexicaux et déterministes).
- **SPEC** : inchangée. Le staging v0→v1 est documenté ici et dans le journal PLAN §5.
