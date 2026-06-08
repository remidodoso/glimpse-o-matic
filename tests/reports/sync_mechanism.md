# Phase 5b — blind-sync mechanism (white-seamless vs detail-rich)

_Generated 2026-06-08 20:02:35 UTC · glimr 0.1.0 · commit `bf06e80-dirty` · config ALPHA=0.15, levels=[2, 3], mask=0.5, ECC=BCH(192,160) t=4._

At the q80 scales, for detail-rich quyen and white-seamless riley: recovered **blind
scale**, whether **matched `--size`** decode still verifies (signal survived ⇒ a sync
problem, not loss), the **top autocorr peak** (lag and implied scale), and **where the
true tile period ranks** among the top peaks.

Regenerate: `cargo test -p glimr --features registration --release sync_mechanism -- --ignored --nocapture`

| image       | config  | blind scale | blind crc | matched crc | top lag | top scale | true rank |
|-------------|---------|-------------|-----------|-------------|---------|-----------|-----------|
| quyen.jpg   | s=1.00  |  1.000 |   ✓   |    ✓    |   256 |  1.000 |   #1    |
| quyen.jpg   | s=0.50  |  0.500 |   ✓   |    ✓    |   256 |  1.000 |   #2    |
| riley.jpg   | s=1.00  |  1.000 |   ✓   |    ✓    |   190 |  0.742 |   #2    |
| riley.jpg   | s=0.50  |  0.500 |   ✓   |    ✓    |   256 |  1.000 |   #2    |

_`matched crc` ✓ while `blind crc` · and the true period low-ranked/absent ⇒ coarse-syncfailure (fixable by detail-aware block selection + harmonic candidates, Phases 6/7) —not ECC or fine search._
