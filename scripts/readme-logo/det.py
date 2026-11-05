"""Deterministic arithmetic for the README logo's generator.

The platform's libm, and NumPy's own SIMD loops, may round `sin`, `cos`,
`exp`, `hypot` and `pow` differently from one machine to the next, and a
generator that prints a strand's place to four decimals and branches on
it would then draw a different logo on another machine. Everything here is
built from `+`, `-`, `*`, `/`, `floor` and `sqrt`, which IEEE 754 rounds
correctly on every platform, Python's floats and NumPy's float64 arrays
alike, so the same input gives the same bits everywhere.

`sin` and `cos` take fdlibm's kernels (`k_sin.c`, `k_cos.c`) on the
argument reduced to [-pi/4, pi/4] by a three-part pi/2; `exp` reduces by
ln 2 in two parts and sums the Taylor series to the thirteenth power,
then scales by a power of two exactly. Each is within a few units in the
last place of the true value, which the generator does not need closer:
it needs the same value on every machine.
"""

import math

import numpy as np

# pi/2 in three parts, fdlibm's: the first two hold 33 bits each, so k
# times either is exact for |k| < 2^20.
_PIO2_1 = 1.57079632673412561417e00
_PIO2_2 = 6.07710050650619224932e-11
_PIO2_3 = 2.02226624879595063154e-21
_TWO_OVER_PI = 6.36619772367581382433e-01

_S = (
    -1.66666666666666324348e-01,
    8.33333333332248946124e-03,
    -1.98412698298579493134e-04,
    2.75573137070700676789e-06,
    -2.50507602534068634195e-08,
    1.58969099521155010221e-10,
)
_C = (
    4.16666666666666019037e-02,
    -1.38888888888741095749e-03,
    2.48015872894767294178e-05,
    -2.75573143513906633035e-07,
    2.08757232129817482790e-09,
    -1.13596475577881948265e-11,
)

_LN2_HI = 6.93147180369123816490e-01
_LN2_LO = 1.90821492927058770002e-10
_INV_LN2 = 1.44269504088896338700e00


def _ksin(r):
    z = r * r
    p = _S[1] + z * (_S[2] + z * (_S[3] + z * (_S[4] + z * _S[5])))
    return r + r * z * (_S[0] + z * p)


def _kcos(r):
    z = r * r
    p = z * (_C[0] + z * (_C[1] + z * (_C[2] + z * (_C[3] + z * (_C[4] + z * _C[5])))))
    return 1.0 - 0.5 * z + z * p


def _reduce(x):
    """k and r with x = k pi/2 + r, |r| <= about pi/4."""
    if isinstance(x, np.ndarray):
        k = np.floor(x * _TWO_OVER_PI + 0.5)
    else:
        k = float(math.floor(x * _TWO_OVER_PI + 0.5))
    r = ((x - k * _PIO2_1) - k * _PIO2_2) - k * _PIO2_3
    return k, r


def sin(x):
    """sin(x), the same bits on every platform; x a float or an array."""
    k, r = _reduce(x)
    s, c = _ksin(r), _kcos(r)
    if isinstance(x, np.ndarray):
        q = np.mod(k, 4.0)
        return np.where(q == 0.0, s, np.where(q == 1.0, c, np.where(q == 2.0, -s, -c)))
    q = k % 4.0
    return s if q == 0.0 else c if q == 1.0 else -s if q == 2.0 else -c


def cos(x):
    """cos(x), as `sin`."""
    k, r = _reduce(x)
    s, c = _ksin(r), _kcos(r)
    if isinstance(x, np.ndarray):
        q = np.mod(k, 4.0)
        return np.where(q == 0.0, c, np.where(q == 1.0, -s, np.where(q == 2.0, -c, s)))
    q = k % 4.0
    return c if q == 0.0 else -s if q == 1.0 else -c if q == 2.0 else s


def exp(x):
    """e^x, the same bits on every platform; x a float or an array, each
    above -700 so the power of two stays normal."""
    if isinstance(x, np.ndarray):
        k = np.floor(x * _INV_LN2 + 0.5)
    else:
        k = float(math.floor(x * _INV_LN2 + 0.5))
    r = (x - k * _LN2_HI) - k * _LN2_LO
    # The Taylor series to r^13 / 13!, Horner's way from the top.
    s = 1.0 / 6227020800.0
    for n in range(12, 0, -1):
        s = 1.0 / math.factorial(n) + r * s
    s = 1.0 + r * s
    if isinstance(x, np.ndarray):
        return np.ldexp(s, k.astype(np.int64))
    return math.ldexp(s, int(k))


def hypot(x, y):
    """sqrt(x^2 + y^2), the square root correctly rounded everywhere."""
    if isinstance(x, np.ndarray) or isinstance(y, np.ndarray):
        return np.sqrt(x * x + y * y)
    return math.sqrt(x * x + y * y)


def fsum_mean(values):
    """The mean of `values`, its sum correctly rounded, whatever order a
    platform's reduction would take."""
    v = [float(a) for a in values]
    return math.fsum(v) / len(v)
