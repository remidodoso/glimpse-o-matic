# 3.2 — cliff error-confidence profile (matched decode at known scale)

_Generated 2026-06-11 01:32:11 -07:00 · glimr 0.1.0 · commit `155e3ce-dirty` · config ALPHA=0.15, levels=[2, 3], mask=0.5, ECC=BCH(192,160) t=4._

Diagnostic for the **soft-decision / Chase** question. The canonical fixture is embedded
(CDF 5/3, ALPHA=0.15, levels [2, 3], mask 0.5), pushed through a ladder of casual
channels (display rescale → crop → low-q JPEG), then decoded **at the known-true scale**
via `decode_corr_at` (the crop offset is registered internally, so registration is removed
as a variable but cropping is still exercised). The 192 codeword bits are ranked by
correlation confidence |corr| (rank 0 = least confident); the table counts how many of the
pre-ECC error bits land among the k least-confident.

Regenerate: `cargo test -p glimr --features registration --release cliff_error_profile -- --ignored --nocapture`

| channel       | errs | in ≤8 | in ≤16 | in ≤32 | median rank |
|---------------|------|-------|--------|--------|-------------|
| q90 1.00 0%   |    0 |     0 |      0 |      0 |           0 |
| q70 0.80 5%   |    3 |     3 |      3 |      3 |           1 |
| q50 0.66 10%  |    2 |     2 |      2 |      2 |           2 |
| q40 0.60 12%  |    8 |     4 |      6 |      7 |           9 |
| q40 0.50 15%  |   33 |     6 |     10 |     16 |          32 |

_Error bits concentrated at low ranks (small median, most within ≤8–16) ⇒ Chase decoding
(flip the k least-confident, CRC-verify) rescues cliff cases past t=4. Errors spread toward
rank ~96 (uniform) ⇒ interference-structured, Chase won't help. Caveat: rescale rounding can
leave a sub-notch scale residual; treat the error **distribution** (shape), not the absolute
count, as the signal here._
