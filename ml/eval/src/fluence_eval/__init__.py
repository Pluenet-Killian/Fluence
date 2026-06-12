# SPDX-License-Identifier: Apache-2.0
"""Simulation harness — the project's compass (SPEC §8.A, D-8.1).

Will implement the simulated-user model (motor noise via spatial confusion
matrices, dwell timing, billed suggestion-scan cost), the metrics (KS%,
simulated WPM, acceptance rate, harmful-suggestion rate), mandatory
baselines and ablations, and the CI gates (KS% regression > 2 points fails
the build). Runs offline per PR and end-to-end against the real hub on
reference machines. Same harness, public subset: FluenceBench-FR (D-8.3).

PLAN Phase 3 builds it (« la boussole ») ; this package stays empty until
then.
"""
