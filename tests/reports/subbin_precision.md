# 3.3 — sub-bin autocorr-peak precision (integer vs parabolic)

_Generated 2026-06-11 01:23:54 -07:00 · glimr 0.1.0 · commit `155e3ce-dirty` · config ALPHA=0.15, levels=[2, 3], mask=0.5, ECC=BCH(192,160) t=4._

The canonical fixture is embedded then resampled to a set of *known* true scales. For each,
the whitened-autocorrelation peak nearest the true level-2 period (256·s) is taken from
[`scale_peaks`] (integer lag) and [`scale_peaks_subbin`] (parabola-refined lag); the implied
scale is `lag / 256` and the error is vs the known true scale. Integer lag
quantizes to ~0.4% at period 256; the matched-decode notch is <0.25% wide.

Regenerate: `cargo test -p glimr --features registration --release subbin_precision -- --ignored --nocapture`

| scale | true lag | int lag | int err | sub lag | sub err |
|-------|----------|---------|---------|---------|---------|
|  0.50 |   128.0 |  128.00 |   0.000% |  128.003 |   0.002% |
|  0.66 |   169.0 |  169.00 |   0.024% |  168.983 |   0.013% |
|  0.80 |   204.8 |  205.00 |   0.098% |  204.879 |   0.038% |
|  0.95 |   243.2 |  243.00 |   0.082% |  243.112 |   0.036% |
|  1.00 |   256.0 |  256.00 |   0.000% |  255.989 |   0.004% |
|  1.20 |   307.2 |  307.00 |   0.065% |  307.047 |   0.050% |
|  1.49 |   381.4 |  381.00 |   0.115% |  381.251 |   0.049% |
|  2.00 |   512.0 |  512.00 |   0.000% |  511.991 |   0.002% |

**Mean |scale error|: integer 0.048%, sub-bin 0.025%.**

_If sub-bin error is consistently below the ~0.25% notch width (and well under the integer
error), wiring `scale_peaks_subbin` into candidate generation lands more candidates in the
notch directly — fewer refine rungs fired, faster *and* more reliable blind decode._
