<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Si ça ne marche plus — carte d'urgence (1 page)

> **À imprimer en gros caractères et garder près de l'appareil.** Conçue pour Jean
> (aidant non technophile, SPEC personas) : une panne ne doit jamais couper la
> parole sans recours.

## La règle d'or

**Le clavier parle toujours.** Même si l'IA, Internet ou la voix « belle » tombent,
**composer et appuyer sur PARLER fonctionnent** (voix de secours du système). Si
ce n'est pas le cas, c'est le **hub** qu'il faut relancer (ci-dessous).

## Les 4 gestes qui règlent presque tout

1. **Rien ne répond ?** Fermez puis **relancez le hub** (rouvrez l'application, ou
   relancez `fluence-hub`). Le brouillon en cours est **sauvegardé** : on perd au
   pire la dernière seconde de frappe.
2. **La page composeur est blanche / « connexion impossible » ?** Vérifiez que le
   hub tourne, puis rechargez **http://127.0.0.1:7411**. Si on vous redemande un
   jeton, ré-appairez l'appareil (écran principal → « appareils » → nouveau code).
3. **La belle voix se tait ?** C'est normal en mode dégradé : la **voix du système**
   prend le relais automatiquement. La parole continue. (On rétablira Piper plus
   tard, sans urgence.)
4. **Les suggestions ont disparu / sont bizarres ?** L'accélérateur IA s'est mis en
   retrait : le clavier prédit avec son **modèle de secours**. On peut composer
   normalement ; relancer le hub remet souvent l'IA.

## Voyants (état du système)

Dans l'**espace aidant** (`…/#care`) : un composant en **rouge / down** redémarre
tout seul (compteur de redémarrages affiché). S'il reste rouge après deux
minutes, relancez le hub.

## Urgence

Le bouton **Urgence** (double confirmation) alerte les autres écrans appairés. Il
ne dépend pas de l'IA.

## Repartir de zéro (en dernier recours)

- **Réinstaller** l'application sans toucher aux données : relancer l'installeur.
- **Restaurer** depuis une sauvegarde : il faut le **kit de secours** (QR + phrase)
  imprimé lors de la sauvegarde — voir `getting-started.md` §9.
- **Tout effacer** (remise à neuf) : `fluence-hub wipe --yes`.

## Demander de l'aide

Notez ce qui s'affiche (sans recopier le **contenu** d'une conversation : Fluence
n'en met jamais dans ses messages d'erreur) et contactez le référent technique
de l'installation.
