# Registration Stage 2 — blind scale + offset recovery + decode

Source: `tests/test_a.jpg` (2500×2500).  CDF 5/3, ALPHA=0.15, levels [2, 3], mask 0.5.
Fully blind: recover scale (autocorrelation) → rescale → level-2 band-pass → fold to a
256-tile → keyed cross-correlation vs per-bit spatial templates.  The score peak is the
crop offset (mod 256); the per-bit correlation signs are the payload.  Target = the crop
table's B-oracle (0/128).  v1 uses level-2 only.

Regenerate: `cargo test -p glimr --release registration_stage2 -- --ignored --nocapture`

`errs blind` uses the blind scale estimate; `errs known` forces the true scale (rescale
back to the cropped dims) to isolate the phase/decode pipeline from scale estimation.

| scale | crop offset   | scale err | errs blind | errs known | phase prom | offset φ |
|-------|---------------|-----------|------------|------------|------------|----------|
| 1.00 | none          |   +0.0% |   0/128 |   0/128 |    12 | (  0,  0) |
| 1.00 | (37,53)       |   +0.0% |   2/128 |   2/128 |     5 | (220,204) |
| 1.00 | (130,200)     |   +0.0% |   0/128 |   0/128 |     8 | (126, 56) |
| 1.00 | 10% (250,250) |   +0.0% | 128/128 | 128/128 |     6 | (  8,  8) |
| 0.70 | none          |   -0.1% | 128/128 |   0/128 |    10 | (  0,  0) |
| 0.70 | (37,53)       |   -0.1% |   0/128 |   2/128 |     5 | (220,204) |
| 0.70 | (130,200)     |   -0.1% |   0/128 |   0/128 |     6 | (126, 56) |
| 0.70 | 10% (250,250) |   -0.1% |   0/128 | 125/128 |     4 | (  8,  8) |
| 0.50 | none          |   +0.0% |   0/128 |   0/128 |     8 | (  0,  0) |
| 0.50 | (37,53)       |   +0.0% |   0/128 |   3/128 |     5 | (220,204) |
| 0.50 | (130,200)     |   +0.0% |   0/128 |   0/128 |     5 | (126, 56) |
| 0.50 | 10% (250,250) |   +0.0% | 122/128 | 122/128 |     3 | (  8,  8) |
| 0.33 | none          |   -0.6% |  68/128 |   0/128 |     7 | (  0,  0) |
| 0.33 | (37,53)       |   -0.6% |  60/128 |   5/128 |     4 | (220,204) |
| 0.33 | (130,200)     |   -0.6% |  59/128 |   1/128 |     3 | (126, 56) |
| 0.33 | 10% (250,250) |   -0.6% |  63/128 |  61/128 |     2 | (228, 52) |

_offset φ is the recovered (x,y) mod 256 at known scale; phase prom = peak/median._
