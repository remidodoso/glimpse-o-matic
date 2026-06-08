# Phase 5c — scale-precision cliff (matched decode)

_Generated 2026-06-08 20:01:47 UTC · glimr 0.1.0 · commit `bf06e80-dirty` · config ALPHA=0.15, levels=[2, 3], mask=0.5, ECC=BCH(192,160) t=4._

A cleanly-embedded canonical fixture decoded at deliberately *wrong* target sizes (±3% in 0.25%
steps).  `raw` = pre-ECC codeword errors; `score` = alignment L1 (the candidate soft
metric).  Shows how sharp the registration cliff is and whether `score` tracks the
error count monotonically — i.e. is it a usable objective for fine-scale hill-climbing.

Regenerate: `cargo test -p glimr --release scale_precision -- --ignored --nocapture`

| scale err | target    | raw | score | crc |
|-----------|-----------|-----|-------|-----|
|  -3.00% | 2425×2425 | 106 |    10.1 |  ·  |
|  -2.75% | 2431×2431 | 101 |    10.0 |  ·  |
|  -2.50% | 2438×2438 | 106 |     8.9 |  ·  |
|  -2.25% | 2444×2444 | 100 |     9.8 |  ·  |
|  -2.00% | 2450×2450 |  94 |    10.4 |  ·  |
|  -1.75% | 2456×2456 |  95 |    10.9 |  ·  |
|  -1.50% | 2463×2463 |  86 |    10.3 |  ·  |
|  -1.25% | 2469×2469 | 105 |    10.3 |  ·  |
|  -1.00% | 2475×2475 |  95 |    10.2 |  ·  |
|  -0.75% | 2481×2481 |  87 |    10.1 |  ·  |
|  -0.50% | 2488×2488 | 112 |     9.1 |  ·  |
|  -0.25% | 2494×2494 | 126 |    10.7 |  ·  |
|  +0.00% | 2500×2500 |   0 |   111.6 |  ✓  |
|  +0.25% | 2506×2506 | 124 |     9.7 |  ·  |
|  +0.50% | 2513×2513 | 108 |     9.8 |  ·  |
|  +0.75% | 2519×2519 |  87 |    10.0 |  ·  |
|  +1.00% | 2525×2525 |  95 |     9.4 |  ·  |
|  +1.25% | 2531×2531 |  98 |     9.3 |  ·  |
|  +1.50% | 2538×2538 | 105 |     9.4 |  ·  |
|  +1.75% | 2544×2544 | 108 |     9.6 |  ·  |
|  +2.00% | 2550×2550 |  98 |     9.4 |  ·  |
|  +2.25% | 2556×2556 |  98 |    10.4 |  ·  |
|  +2.50% | 2563×2563 |  95 |    10.0 |  ·  |
|  +2.75% | 2569×2569 | 107 |     9.5 |  ·  |
|  +3.00% | 2575×2575 |  88 |     9.0 |  ·  |

_A narrow 0-error notch with `score` peaking there and falling off monotonically =a clean objective for the Phase-8 fine search._
