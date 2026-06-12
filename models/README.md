# models/ — registre de manifestes de modèles

Ce répertoire contient les **manifestes versionnés** du registre de modèles (SPEC D-3.2) :
`{id, version, rôle, fichiers (sha256, taille), sources (HF + miroir), licence, palier
matériel minimal}`, signés (minisign).

**Jamais de poids de modèles dans git** — uniquement leurs descriptions vérifiables.
Les poids sont téléchargés à l'installation (reprise sur coupure, vérification sha256)
ou fournis par le pack hors-ligne USB.

Premier manifeste attendu en Phase 4 (tiny-LLM de test, puis Gemma E2B/E4B) ;
signatures minisign complètes en Phase 7 (tâche 7.4).
