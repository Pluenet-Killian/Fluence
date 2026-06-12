# SPDX-License-Identifier: Apache-2.0
"""Synthetic dialogue generation and corpus preparation (SPEC §5.D, D-5.5).

Will implement the controlled generation matrix (12 situations × 4 registers,
strict anti-pathos instruction), the input-variant generators (telegraphic,
AZERTY spatial-confusion noise, abbreviations), automatic-judge filtering and
dataset hygiene (versioned JSONL, datasheets, frozen splits — the test split
never tunes prompts).

PLAN Phase 3 builds corpus v0 (task 3.4); this package stays empty until
then.
"""
