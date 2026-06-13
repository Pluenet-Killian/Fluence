# SPDX-License-Identifier: Apache-2.0
"""Module entry point: ``python -m fluence_eval`` runs the evaluation CLI."""

import sys

from fluence_eval.cli import main

if __name__ == "__main__":
    sys.exit(main())
