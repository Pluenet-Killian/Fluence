# Licences de Fluence

Fluence applique un **double licensing par couche** (SPEC D-10.1) : les briques réutilisables
sont sous licence permissive pour installer nos standards dans tout l'écosystème (y compris
commercial) ; l'application complète est sous copyleft réseau pour protéger le produit du
fork fermé.

| Répertoire | Contenu | Licence |
|---|---|---|
| `crates/` | briques Rust : protocole, input engine, accélération, voix, store… | [Apache-2.0](LICENSES/Apache-2.0.txt) |
| `packages/` | briques TypeScript : SDK, composants UI, intégrations | [Apache-2.0](LICENSES/Apache-2.0.txt) |
| `ml/` | pipelines de données et harnais d'évaluation (publié, D-8.3) | [Apache-2.0](LICENSES/Apache-2.0.txt) |
| `xtask/` | outillage du dépôt | [Apache-2.0](LICENSES/Apache-2.0.txt) |
| `apps/` | application complète : desktop, client web, CLI | [AGPL-3.0-only](LICENSES/AGPL-3.0-only.txt) |
| `docs/`, `models/` | documentation et manifestes | Apache-2.0 |

Chaque fichier source porte un en-tête `SPDX-License-Identifier` ; la règle est vérifiée en
CI par `cargo xtask check-licenses`. Les textes complets font foi : copie par sous-arbre
(`crates/LICENSE`, `apps/LICENSE`, …) et exemplaires canoniques dans [`LICENSES/`](LICENSES/).
