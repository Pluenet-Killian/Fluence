# Description

<!-- Quoi et pourquoi. Référencer l'issue (#NN), la phase du PLAN et les décisions SPEC (D-x.y) concernées. -->

## Definition of Done (PLAN §0.1 — aucune exception « c'est trivial »)

- [ ] Code + **tests du comportement nouveau** (un bug corrigé commence par un test rouge — PLAN §0.2)
- [ ] Doc d'API publique (rustdoc / tsdoc / docstring)
- [ ] CI verte **Windows ET Linux**
- [ ] Self-review faite (+ `/code-review` si PR substantielle)

## Invariants du projet

- [ ] **Aucune donnée P0** (conversations, mémoire, voix — SPEC §9.A) dans les logs, erreurs, fixtures
- [ ] Si le hub est touché : les **kill-tests** passent (« le clavier parle toujours », SPEC §2.C)
- [ ] Si une décision d'architecture a été prise : **ADR** ajouté dans `docs/adr/`
- [ ] Si la réalité contredit la SPEC : amendement explicite proposé (jamais de contournement silencieux)
