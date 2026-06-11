# 3.1 — source-size floor (full-accumulation fold)

_Generated 2026-06-11 01:30:02 -07:00 · glimr 0.1.0 · commit `155e3ce-dirty` · config ALPHA=0.15, levels=[2, 3], mask=0.5, ECC=BCH(192,160) t=4._

The marked canonical fixture (2500×2500) downscaled (lossless, Lanczos) to a descending
long-edge ladder, then decoded two ways. **Blind** = the production `decode_blind_auto`
(scale finder + the now-full-accumulation `fold_tile` reader). **Matched** = `decode_y_at_size`
at the known original size — a signal-present reference (it does *not* use `fold_tile`, so it
is unaffected by the 3.1 change; it shows whether the mark survived the downscale at all).
Baseline before full-accumulation fold: blind finder gave up ~1024 px, clean matched floor ≈896.

Regenerate: `cargo test -p glimr --features registration --release source_size_floor -- --ignored --nocapture`

| long | resized   | blind | errs |  prom  | match | errs | score |
|------|-----------|-------|------|--------|-------|------|-------|
| 1280 | 1280×1280 |   ✓   |    0 |   5.94 |    ✓    |    0 |   56.4 |
| 1152 | 1152×1152 |   ✓   |    0 |   5.67 |    ✓    |    0 |   50.3 |
| 1024 | 1024×1024 |   ✓   |    0 |   3.71 |    ✓    |    0 |   43.5 |
|  960 |   960×960 |   ·   |    3 |   2.18 |    ✓    |    0 |   39.8 |
|  896 |   896×896 |   ·   |   22 |   1.88 |    ✓    |    0 |   36.0 |
|  832 |   832×832 |   ·   |   61 |   1.71 |    ✓    |    0 |   32.1 |
|  768 |   768×768 |   ·   |    6 |   2.18 |    ✓    |    0 |   28.0 |
|  704 |   704×704 |   ·   |    7 |   3.39 |    ✓    |    0 |   24.0 |
|  640 |   640×640 |   ✓   |    0 |   4.12 |    ✓    |    0 |   19.6 |
|  576 |   576×576 |   ✓   |    0 |   3.79 |    ✓    |    0 |   15.5 |
|  512 |   512×512 |   ·   |    7 |   2.59 |    ✓    |    0 |   11.6 |

_The smallest `long` with blind crc ✓ is the production floor; matched ✓ below that marks the
gap the finder/reader still leave on the table. Compare the blind floor against the ~1024 px
baseline to read off what full accumulation bought._
