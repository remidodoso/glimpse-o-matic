# Blind `--auto` sweep — multi-image, pre-ECC

Embed (CDF 5/3, ALPHA=0.15, levels [2, 3], mask 0.5) → optional JPEG q80 → crop →
rescale, then **fully blind** `decode_blind_auto` (recover scale + crop offset, decode).
Raw bit errors are pre-ECC; `confidence` is the phase-peak prominence.  This measures the
real envelope across varied content and sizes the ECC budget.

Fixtures: 5 images via `tests/*.jpg`.  **Clean (0-error) cells: 36/40; CRC-verified: 36/40.**

Regenerate: `cargo test -p glimr --features registration blind_auto_sweep -- --ignored --nocapture`

| image         | jpg | scale | crop         | scale err | confidence | bit errors | crc |
|---------------|-----|-------|--------------|-----------|------------|------------|-----|
| test_a.jpg    | raw | 1.00 | none         |   +0.0% |    7.8 |   0/128 |  ✓  |
| test_a.jpg    | raw | 1.00 | crop 130,200 |   +0.0% |    5.8 |   0/128 |  ✓  |
| test_a.jpg    | raw | 0.50 | none         |   +0.0% |    5.5 |   0/128 |  ✓  |
| test_a.jpg    | raw | 0.50 | crop 130,200 |   +0.0% |    4.1 |   1/128 |  ·  |
| test_a.jpg    | q80 | 1.00 | none         |   +0.0% |    6.5 |   0/128 |  ✓  |
| test_a.jpg    | q80 | 1.00 | crop 130,200 |   +0.0% |    4.9 |   0/128 |  ✓  |
| test_a.jpg    | q80 | 0.50 | none         |   +0.0% |    5.0 |   0/128 |  ✓  |
| test_a.jpg    | q80 | 0.50 | crop 130,200 |   +0.0% |    3.8 |   1/128 |  ·  |
| test_b.jpg    | raw | 1.00 | none         |   +0.0% |   11.9 |   0/128 |  ✓  |
| test_b.jpg    | raw | 1.00 | crop 130,200 |   +0.0% |    9.5 |   0/128 |  ✓  |
| test_b.jpg    | raw | 0.50 | none         |   +0.0% |    9.7 |   0/128 |  ✓  |
| test_b.jpg    | raw | 0.50 | crop 130,200 |   +0.0% |    7.5 |   0/128 |  ✓  |
| test_b.jpg    | q80 | 1.00 | none         |   +0.0% |   11.1 |   0/128 |  ✓  |
| test_b.jpg    | q80 | 1.00 | crop 130,200 |   +0.0% |    8.9 |   0/128 |  ✓  |
| test_b.jpg    | q80 | 0.50 | none         |   +0.0% |    9.3 |   0/128 |  ✓  |
| test_b.jpg    | q80 | 0.50 | crop 130,200 |   +0.0% |    7.3 |   0/128 |  ✓  |
| test_c.jpg    | raw | 1.00 | none         |   +0.0% |   11.6 |   0/128 |  ✓  |
| test_c.jpg    | raw | 1.00 | crop 130,200 |   +0.0% |    9.2 |   0/128 |  ✓  |
| test_c.jpg    | raw | 0.50 | none         |   +0.0% |    9.4 |   0/128 |  ✓  |
| test_c.jpg    | raw | 0.50 | crop 130,200 |   +0.0% |    7.2 |   0/128 |  ✓  |
| test_c.jpg    | q80 | 1.00 | none         |   +0.0% |   11.2 |   0/128 |  ✓  |
| test_c.jpg    | q80 | 1.00 | crop 130,200 |   +0.0% |    8.8 |   0/128 |  ✓  |
| test_c.jpg    | q80 | 0.50 | none         |   +0.0% |    9.2 |   0/128 |  ✓  |
| test_c.jpg    | q80 | 0.50 | crop 130,200 |   +0.0% |    7.1 |   0/128 |  ✓  |
| test_d.jpg    | raw | 1.00 | none         |   +0.0% |   10.1 |   0/128 |  ✓  |
| test_d.jpg    | raw | 1.00 | crop 130,200 |   +0.0% |    7.5 |   0/128 |  ✓  |
| test_d.jpg    | raw | 0.50 | none         |   +0.0% |    7.9 |   0/128 |  ✓  |
| test_d.jpg    | raw | 0.50 | crop 130,200 |   +0.0% |    5.8 |   0/128 |  ✓  |
| test_d.jpg    | q80 | 1.00 | none         |   +0.0% |    9.6 |   0/128 |  ✓  |
| test_d.jpg    | q80 | 1.00 | crop 130,200 |   +0.0% |    7.2 |   0/128 |  ✓  |
| test_d.jpg    | q80 | 0.50 | none         |   +0.0% |    7.6 |   0/128 |  ✓  |
| test_d.jpg    | q80 | 0.50 | crop 130,200 |   +0.0% |    5.7 |   0/128 |  ✓  |
| test_e.jpg    | raw | 1.00 | none         |   +0.0% |    9.6 |   0/128 |  ✓  |
| test_e.jpg    | raw | 1.00 | crop 130,200 |   +0.0% |    7.5 |   0/128 |  ✓  |
| test_e.jpg    | raw | 0.50 | none         |   +0.0% |    7.6 |   0/128 |  ✓  |
| test_e.jpg    | raw | 0.50 | crop 130,200 |   +0.0% |    5.9 |   0/128 |  ✓  |
| test_e.jpg    | q80 | 1.00 | none         |  -31.6% |    1.5 |  57/128 |  ·  |
| test_e.jpg    | q80 | 1.00 | crop 130,200 |  -31.2% |    1.6 |  56/128 |  ·  |
| test_e.jpg    | q80 | 0.50 | none         |   +0.0% |    6.7 |   0/128 |  ✓  |
| test_e.jpg    | q80 | 0.50 | crop 130,200 |   +0.0% |    5.1 |   0/128 |  ✓  |

_`crc` ✓ = the embedded CRC-32 verified (definitive). Cells with a few errors are ECC's job;cells near 64/128 are registration failures (need more signal)._
