# Phase 5a — channel-quality waterfall (matched decode)

Embed → scale → JPEG q → **decode at the known original size** (registration exact, so
the only error source is channel noise).  `raw` = pre-ECC bit errors over the 192-bit
codeword; `ecc` = bits BCH corrected; `crc` ✓ = verified after correction.  Shows
whether the 1..4-error band exists and how much quality range t=4 buys.

Regenerate: `cargo test -p glimr --release channel_waterfall -- --ignored --nocapture`

| image       | scale  | q | raw | ecc | crc |
|-------------|--------|---|-----|-----|-----|
| test_a.jpg  | native |  90 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  80 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  70 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  60 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  50 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  45 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  40 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  35 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  30 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  25 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  20 |   0 |   0 |  ✓  |
| test_a.jpg  | native |  15 |   2 |   2 |  ✓  |
| test_a.jpg  | native |  10 |   4 |   4 |  ✓  |
| test_a.jpg  | 0.5x   |  90 |   0 |   0 |  ✓  |
| test_a.jpg  | 0.5x   |  80 |   1 |   1 |  ✓  |
| test_a.jpg  | 0.5x   |  70 |   5 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  60 |   5 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  50 |   6 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  45 |   7 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  40 |   7 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  35 |   9 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  30 |  17 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  25 |  14 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  20 |  15 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  15 |  24 |   0 |  ·  |
| test_a.jpg  | 0.5x   |  10 |  31 |   0 |  ·  |
| test_e.jpg  | native |  90 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  80 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  70 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  60 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  50 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  45 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  40 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  35 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  30 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  25 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  20 |   0 |   0 |  ✓  |
| test_e.jpg  | native |  15 |   2 |   2 |  ✓  |
| test_e.jpg  | native |  10 |   1 |   1 |  ✓  |
| test_e.jpg  | 0.5x   |  90 |   0 |   0 |  ✓  |
| test_e.jpg  | 0.5x   |  80 |   0 |   0 |  ✓  |
| test_e.jpg  | 0.5x   |  70 |   0 |   0 |  ✓  |
| test_e.jpg  | 0.5x   |  60 |   4 |   4 |  ✓  |
| test_e.jpg  | 0.5x   |  50 |   5 |   0 |  ·  |
| test_e.jpg  | 0.5x   |  45 |   5 |   0 |  ·  |
| test_e.jpg  | 0.5x   |  40 |   8 |   0 |  ·  |
| test_e.jpg  | 0.5x   |  35 |   9 |   0 |  ·  |
| test_e.jpg  | 0.5x   |  30 |  16 |   0 |  ·  |
| test_e.jpg  | 0.5x   |  25 |  15 |   0 |  ·  |
| test_e.jpg  | 0.5x   |  20 |  16 |   0 |  ·  |
| test_e.jpg  | 0.5x   |  15 |  25 |   0 |  ·  |
| test_e.jpg  | 0.5x   |  10 |  23 |   0 |  ·  |

_If `raw` steps 0→1→2→3→4 before climbing, t=4 buys real range; if it jumps 0→≫4 thewaterfall is too steep for hard ECC and soft-decision is the lever._
