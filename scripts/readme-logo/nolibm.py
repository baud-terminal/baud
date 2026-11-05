"""Run a generator script with the platform's transcendentals taken away.

    nolibm.py <script> <args...>

Every function whose result a platform's libm or NumPy's SIMD loops may
round differently -- math's sin, cos, tan, exp, log, pow, hypot and atan2,
NumPy's sin, cos, tan, exp, log, power, hypot, arctan2, matmul, dot and
convolve -- is replaced by one that raises, and the script then runs as
its own `__main__`. A run that finishes used none of them, so its output
rests on IEEE's own arithmetic and `det.py` alone. Float `**`, which no
replacement reaches, the scripts spell as products instead. The modules
the scripts import are imported first, so what they compute once as they
load -- `random`'s constants -- is theirs, not the scripts'.
"""

import math
import random  # noqa: F401
import runpy
import sys
import tempfile  # noqa: F401

import numpy as np
from PIL import Image  # noqa: F401


def refused(name):
    def f(*_args, **_kwargs):
        raise SystemExit(f"nolibm: {name} would round as the platform does")

    return f


for name in ("sin", "cos", "tan", "exp", "log", "pow", "hypot", "atan2"):
    setattr(math, name, refused(f"math.{name}"))
for name in ("sin", "cos", "tan", "exp", "log", "power", "hypot", "arctan2", "matmul", "dot", "convolve"):
    setattr(np, name, refused(f"numpy.{name}"))

script = sys.argv[1]
sys.argv = sys.argv[1:]
runpy.run_path(script, run_name="__main__")
