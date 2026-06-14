# models/ — registre de manifestes de modèles

Ce répertoire contient les **manifestes versionnés** du registre de modèles (SPEC D-3.2) :
`{id, version, rôle, fichiers (sha256, taille), sources (HF + miroir), licence, palier
matériel minimal}`, signés (minisign).

**Jamais de poids de modèles dans git** — uniquement leurs descriptions vérifiables.
Les poids sont téléchargés à l'installation (reprise sur coupure, vérification sha256)
ou fournis par le pack hors-ligne USB.

## Intégrité (D-3.2, `fluence-models`)

Deux couches indépendantes, **fail-closed** :

1. **sha256 par fichier** — le contrat d'intégrité de chaque poids (l'URL n'est
   qu'un indice ; le contenu est vérifié, jamais présumé).
2. **Signature minisign du manifeste** — prouve que le manifeste vient de la clé
   de release du projet (chaîne d'approvisionnement). `download-test-assets`
   vérifie la signature **avant** de faire confiance au manifeste **quand** un
   `<manifeste>.minisig` et `FLUENCE_MODELS_PUBKEY` sont tous deux présents
   (sinon, en dev/CI, le manifeste in-repo est git-trusted ; une moitié présente
   sans l'autre = erreur dure, pas de downgrade silencieux).

### Signer un manifeste de release (étape opérateur)

La **clé privée** de signature est un secret d'opérateur (jamais dans ce dépôt) :

```sh
# Une fois : générer la paire de clés de release (garder la clé privée hors-ligne).
minisign -G -p fluence-release.pub -s fluence-release.key
# À chaque release : signer le manifeste, puis publier le .minisig à côté.
minisign -S -s fluence-release.key -m models/test-assets.json
# Côté machine de provisioning : exporter la clé publique pour activer la vérif.
export FLUENCE_MODELS_PUBKEY="$(tail -n1 fluence-release.pub)"
```

### GC du cache

`cargo xtask models-gc` liste les poids en cache que le manifeste ne référence
plus (dry-run) ; `--apply` les supprime (les `.part` en cours sont épargnés).
