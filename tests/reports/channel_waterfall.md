# Phase 5a — channel-quality waterfall (matched decode)

_Generated 2026-06-08 20:02:02 UTC · glimr 0.1.0 · commit `bf06e80-dirty` · config ALPHA=0.15, levels=[2, 3], mask=0.5, ECC=BCH(192,160) t=4._

Embed → scale → JPEG q → **decode at the known original size** (registration exact, so
the only error source is channel noise).  `raw` = pre-ECC bit errors over the 192-bit
codeword; `ecc` = bits BCH corrected; `crc` ✓ = verified after correction.  Shows
whether the 1..4-error band exists and how much quality range t=4 buys.

Regenerate: `cargo test -p glimr --release channel_waterfall -- --ignored --nocapture`

| image       | scale  | q | raw | ecc | crc |
|-------------|--------|---|-----|-----|-----|
| quyen.jpg   | native |  90 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  80 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  70 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  60 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  50 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  45 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  40 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  35 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  30 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  25 |   0 |   0 |  ✓  |
| quyen.jpg   | native |  20 |   1 |   1 |  ✓  |
| quyen.jpg   | native |  15 |   2 |   2 |  ✓  |
| quyen.jpg   | native |  10 |   4 |   4 |  ✓  |
| quyen.jpg   | 0.5x   |  90 |   0 |   0 |  ✓  |
| quyen.jpg   | 0.5x   |  80 |   1 |   1 |  ✓  |
| quyen.jpg   | 0.5x   |  70 |   4 |   4 |  ✓  |
| quyen.jpg   | 0.5x   |  60 |   5 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  50 |   5 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  45 |   6 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  40 |   6 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  35 |   8 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  30 |  14 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  25 |  13 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  20 |  16 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  15 |  21 |   0 |  ·  |
| quyen.jpg   | 0.5x   |  10 |  32 |   0 |  ·  |
| riley.jpg   | native |  90 |   0 |   0 |  ✓  |
| riley.jpg   | native |  80 |   0 |   0 |  ✓  |
| riley.jpg   | native |  70 |   0 |   0 |  ✓  |
| riley.jpg   | native |  60 |   0 |   0 |  ✓  |
| riley.jpg   | native |  50 |   0 |   0 |  ✓  |
| riley.jpg   | native |  45 |   0 |   0 |  ✓  |
| riley.jpg   | native |  40 |   0 |   0 |  ✓  |
| riley.jpg   | native |  35 |   0 |   0 |  ✓  |
| riley.jpg   | native |  30 |   0 |   0 |  ✓  |
| riley.jpg   | native |  25 |   0 |   0 |  ✓  |
| riley.jpg   | native |  20 |   0 |   0 |  ✓  |
| riley.jpg   | native |  15 |   1 |   0 |  ✓  |
| riley.jpg   | native |  10 |   1 |   1 |  ✓  |
| riley.jpg   | 0.5x   |  90 |   0 |   0 |  ✓  |
| riley.jpg   | 0.5x   |  80 |   0 |   0 |  ✓  |
| riley.jpg   | 0.5x   |  70 |   0 |   0 |  ✓  |
| riley.jpg   | 0.5x   |  60 |   1 |   1 |  ✓  |
| riley.jpg   | 0.5x   |  50 |   3 |   3 |  ✓  |
| riley.jpg   | 0.5x   |  45 |   4 |   4 |  ✓  |
| riley.jpg   | 0.5x   |  40 |   6 |   0 |  ·  |
| riley.jpg   | 0.5x   |  35 |   6 |   0 |  ·  |
| riley.jpg   | 0.5x   |  30 |  11 |   0 |  ·  |
| riley.jpg   | 0.5x   |  25 |   7 |   0 |  ·  |
| riley.jpg   | 0.5x   |  20 |  17 |   0 |  ·  |
| riley.jpg   | 0.5x   |  15 |  23 |   0 |  ·  |
| riley.jpg   | 0.5x   |  10 |  24 |   0 |  ·  |

_If `raw` steps 0→1→2→3→4 before climbing, t=4 buys real range; if it jumps 0→≫4 thewaterfall is too steep for hard ECC and soft-decision is the lever._
