# Politique de sécurité

Fluence traite des données de classe **P0 intime** (conversations, mémoire personnelle, voix
clonée — voir `docs/SPEC.md` §9.A) pour des personnes en situation de handicap moteur lourd.
Une vulnérabilité peut exposer ce qu'une personne a de plus privé, ou la priver de son moyen
de communication. Nous prenons chaque signalement au sérieux.

## Signaler une vulnérabilité

- **Canal privilégié** : [GitHub Private Vulnerability Reporting](../../security/advisories/new)
  (onglet *Security* → *Report a vulnerability*). N'ouvrez **jamais** d'issue publique pour
  une vulnérabilité.
- Décrivez : version/commit affecté, scénario d'attaque, impact estimé (en particulier sur
  les données P0 et sur la disponibilité de la parole), preuve de concept si possible.

## Engagements (divulgation coordonnée — D-9.3)

| Étape | Délai visé |
|---|---|
| Accusé de réception | 72 h |
| Première évaluation (triage, sévérité) | 7 jours |
| Correctif ou plan de correction communiqué | 90 jours max |
| Publication coordonnée de l'avis | après correctif, avec crédit au rapporteur |

Le projet est porté par des bénévoles ; ces délais sont des objectifs de bonne foi.

## Périmètre

- **Couvert** (cf. threat model, SPEC §9.A) : vol du PC (chiffrement au repos), site web
  malveillant visant le port local (anti drive-by), curieux du LAN (TLS + tokens à scopes),
  aidant outrepassant ses droits (ACL + journal d'accès).
- **Hors périmètre documenté** : administrateur OS malveillant, attaquant physique
  persistant, acteurs étatiques.

## Versions supportées

Le projet est en pré-alpha (aucune release publiée). Seule la branche `main` reçoit des
correctifs. À partir de la première release : canaux `beta` et `stable` (D-11.2).

## Mesures en place

- Fuzzing des parseurs réseau (protocole d'entrée, API, IPC) en CI nightly dès leur création.
- `cargo-deny` (advisories RustSec) bloquant en CI.
- Threat model publié et tenu à jour dans `docs/SPEC.md` §9.A.
- Revue de sécurité communautaire organisée avant toute bêta publique ; audit professionnel
  documenté comme dette assumée d'ici là (D-9.3).
