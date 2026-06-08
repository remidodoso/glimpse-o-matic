# Blind `--auto` sweep — multi-image robustness

_Generated 2026-06-08 20:07:05 UTC · glimr 0.1.0 · commit `bf06e80-dirty` · config ALPHA=0.15, levels=[2, 3], mask=0.5, ECC=BCH(192,160) t=4._

Embed (CDF 5/3, ALPHA=0.15, levels [2, 3], mask 0.5) → optional JPEG q80 → crop →
rescale, then **fully blind** `decode_blind_auto` (recover scale + crop offset, decode —
including ECC).  `errs` are residual bit errors *after* ECC, `crc` ✓ is the definitive verdict,
`confidence` is the phase-peak prominence.  Measures the real end-to-end envelope across content.

Fixtures: 5 images via `tests/fixtures/*.jpg`.  **Clean (0-error) cells: 40/40; CRC-verified: 40/40.**

Regenerate: `cargo test -p glimr --features registration blind_auto_sweep -- --ignored --nocapture`

| image         | jpg | scale | crop         | scale err | confidence | bit errors | crc |
|---------------|-----|-------|--------------|-----------|------------|------------|-----|
| collibrina.jpg | raw | 1.00 | none         |   +0.0% |   12.0 |   0/128 |  ✓  |
| collibrina.jpg | raw | 1.00 | crop 130,200 |   +0.0% |    9.6 |   0/128 |  ✓  |
| collibrina.jpg | raw | 0.50 | none         | +100.0% |    3.6 |   0/128 |  ✓  |
| collibrina.jpg | raw | 0.50 | crop 130,200 |   +0.0% |    7.6 |   0/128 |  ✓  |
| collibrina.jpg | q80 | 1.00 | none         |   +0.0% |   11.2 |   0/128 |  ✓  |
| collibrina.jpg | q80 | 1.00 | crop 130,200 |   +0.0% |    8.9 |   0/128 |  ✓  |
| collibrina.jpg | q80 | 0.50 | none         | +100.0% |    3.5 |   0/128 |  ✓  |
| collibrina.jpg | q80 | 0.50 | crop 130,200 |   +0.0% |    7.3 |   0/128 |  ✓  |
| fairbanks.jpg | raw | 1.00 | none         |   +0.0% |   10.2 |   0/128 |  ✓  |
| fairbanks.jpg | raw | 1.00 | crop 130,200 |   +0.0% |    7.5 |   0/128 |  ✓  |
| fairbanks.jpg | raw | 0.50 | none         | +100.0% |    3.3 |   0/128 |  ✓  |
| fairbanks.jpg | raw | 0.50 | crop 130,200 |   +0.0% |    5.9 |   0/128 |  ✓  |
| fairbanks.jpg | q80 | 1.00 | none         |   +0.0% |    9.8 |   0/128 |  ✓  |
| fairbanks.jpg | q80 | 1.00 | crop 130,200 |   -0.1% |    6.4 |   0/128 |  ✓  |
| fairbanks.jpg | q80 | 0.50 | none         | +100.0% |    3.2 |   0/128 |  ✓  |
| fairbanks.jpg | q80 | 0.50 | crop 130,200 |   +0.0% |    5.7 |   0/128 |  ✓  |
| quyen.jpg     | raw | 1.00 | none         |   +0.0% |    7.8 |   0/128 |  ✓  |
| quyen.jpg     | raw | 1.00 | crop 130,200 |   +0.0% |    5.9 |   0/128 |  ✓  |
| quyen.jpg     | raw | 0.50 | none         |   +0.0% |    5.5 |   0/128 |  ✓  |
| quyen.jpg     | raw | 0.50 | crop 130,200 |   +0.0% |    4.2 |   0/128 |  ✓  |
| quyen.jpg     | q80 | 1.00 | none         |   +0.0% |    6.5 |   0/128 |  ✓  |
| quyen.jpg     | q80 | 1.00 | crop 130,200 |   +0.0% |    5.0 |   0/128 |  ✓  |
| quyen.jpg     | q80 | 0.50 | none         |   +0.0% |    5.0 |   0/128 |  ✓  |
| quyen.jpg     | q80 | 0.50 | crop 130,200 |   +0.0% |    3.8 |   0/128 |  ✓  |
| riley.jpg     | raw | 1.00 | none         |   +0.0% |    9.8 |   0/128 |  ✓  |
| riley.jpg     | raw | 1.00 | crop 130,200 |   +0.0% |    7.6 |   0/128 |  ✓  |
| riley.jpg     | raw | 0.50 | none         |   +0.0% |    7.7 |   0/128 |  ✓  |
| riley.jpg     | raw | 0.50 | crop 130,200 |   +0.0% |    5.9 |   0/128 |  ✓  |
| riley.jpg     | q80 | 1.00 | none         |   +0.0% |    8.0 |   0/128 |  ✓  |
| riley.jpg     | q80 | 1.00 | crop 130,200 |   +0.0% |    6.2 |   0/128 |  ✓  |
| riley.jpg     | q80 | 0.50 | none         |   +0.0% |    6.7 |   0/128 |  ✓  |
| riley.jpg     | q80 | 0.50 | crop 130,200 |   +0.0% |    5.1 |   0/128 |  ✓  |
| zia.jpg       | raw | 1.00 | none         |   +0.0% |   11.7 |   0/128 |  ✓  |
| zia.jpg       | raw | 1.00 | crop 130,200 |   +0.0% |    9.2 |   0/128 |  ✓  |
| zia.jpg       | raw | 0.50 | none         |   +0.0% |    9.5 |   0/128 |  ✓  |
| zia.jpg       | raw | 0.50 | crop 130,200 |   +0.0% |    7.3 |   0/128 |  ✓  |
| zia.jpg       | q80 | 1.00 | none         |   +0.0% |   11.3 |   0/128 |  ✓  |
| zia.jpg       | q80 | 1.00 | crop 130,200 |   +0.0% |    8.9 |   0/128 |  ✓  |
| zia.jpg       | q80 | 0.50 | none         |   +0.0% |    9.3 |   0/128 |  ✓  |
| zia.jpg       | q80 | 0.50 | crop 130,200 |   +0.0% |    7.1 |   0/128 |  ✓  |

_`crc` ✓ = the embedded CRC-32 verified (definitive). Cells with a few errors are ECC's job;cells near 64/128 are registration failures (need more signal)._
