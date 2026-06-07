// Spread-spectrum DWT watermarking.
// Phase 1: 2D DWT (CDF 5/3 lifting) with round-trip tests.
// Phase 2: PN generation, embedding, blind correlation decode.

// ── Constants ────────────────────────────────────────────────────────────────

pub const WM_KEY:       u64    = 0xDEAD_BEEF_C0FF_EE42u64;
pub const ALPHA:        f32    = 0.15;
pub const EMBED_LEVELS: &[u32] = &[2, 3];
pub const TILE_SIDE:    usize  = 64;   // PN grid: each subband normalized to TILE_SIDE×TILE_SIDE
pub const DECOMP_DEPTH:  u32   = 4;
pub const PAYLOAD_BITS:  usize = 128;
pub const RESIDUAL_AMP:  f32   = 20.0;

// Perceptual masking (Stage 2): per-coefficient embedding strength is scaled by
// local detail energy, then renormalized to mean 1 (energy-neutral — detection
// strength unchanged, only the spatial distribution of the grain changes).
pub const MASK_FLOOR:    f32 = 0.30;  // min strength multiplier (smooth regions)
pub const MASK_CEIL:     f32 = 3.00;  // max strength multiplier (busy regions)
pub const MASK_GAMMA:    f32 = 0.50;  // softening exponent on the activity ratio
pub const MASK_STRENGTH: f32 = 0.50;  // blend: 0 = uniform, 1 = full masking

// ── Y channel helpers ────────────────────────────────────────────────────────

/// Extract luminance from RGBA pixels (4 bytes/pixel) as f32.
pub fn extract_y(pixels: &[u8]) -> Vec<f32> {
    pixels.chunks(4)
        .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
        .collect()
}

/// Extract luminance from RGB pixels (3 bytes/pixel) as f32.
pub fn extract_y_rgb(pixels: &[u8]) -> Vec<f32> {
    pixels.chunks(3)
        .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
        .collect()
}

/// Write the delta between watermarked and original Y back into RGBA.
/// Equal delta applied to R, G, B; alpha untouched.
pub fn write_y_delta(pixels: &mut [u8], y_orig: &[f32], y_new: &[f32]) {
    for (chunk, (&yo, &yn)) in pixels.chunks_mut(4).zip(y_orig.iter().zip(y_new.iter())) {
        let d = yn - yo;
        chunk[0] = (chunk[0] as f32 + d).clamp(0.0, 255.0) as u8;
        chunk[1] = (chunk[1] as f32 + d).clamp(0.0, 255.0) as u8;
        chunk[2] = (chunk[2] as f32 + d).clamp(0.0, 255.0) as u8;
    }
}

// ── 1D CDF 5/3 DWT (LeGall, lifting scheme) ──────────────────────────────────
//
// One predict (linear interpolation) + one update step.  Unlike Haar's box
// basis, the synthesis is smooth and overlapping, so modifying one coefficient
// paints a gentle ramp rather than a hard ±block — the watermark grain becomes
// continuous instead of tiled.  Two vanishing moments: both constant *and*
// linear content vanish in the detail bands (better energy compaction than Haar).
//
// Whole-sample symmetric (mirror) boundary extension gives perfect
// reconstruction at the edges for any length, even or odd.  Output is
// deinterleaved into Mallat layout: [ approx (even samples) | detail (odd) ].
// The approximation half holds ceil(n/2) coefficients (`lo_len`), detail floor.

/// Length of the approximation (low-pass) half of an `n`-sample 1D transform.
#[inline]
fn lo_len(n: usize) -> usize { (n + 1) / 2 }

fn dwt_1d_fwd(buf: &mut [f32], n: usize) {
    if n < 2 { return; }
    // Predict: each odd sample -= mean of its even neighbours.
    let mut k = 1;
    while k < n {
        let left  = buf[k - 1];
        let right = if k + 1 < n { buf[k + 1] } else { buf[k - 1] }; // mirror x[n]=x[n-2]
        buf[k] -= 0.5 * (left + right);
        k += 2;
    }
    // Update: each even sample += quarter of its (new) odd neighbours.
    let mut k = 0;
    while k < n {
        let left  = if k >= 1    { buf[k - 1] } else { buf[1] };      // mirror x[-1]=x[1]
        let right = if k + 1 < n { buf[k + 1] } else { buf[k - 1] };  // mirror x[n]=x[n-2]
        buf[k] += 0.25 * (left + right);
        k += 2;
    }
    // Deinterleave: evens → [0, lo), odds → [lo, n).
    let lo = lo_len(n);
    let mut tmp = vec![0f32; n];
    let (mut e, mut o) = (0usize, lo);
    for i in 0..n {
        if i & 1 == 0 { tmp[e] = buf[i]; e += 1; }
        else          { tmp[o] = buf[i]; o += 1; }
    }
    buf[..n].copy_from_slice(&tmp[..n]);
}

fn dwt_1d_inv(buf: &mut [f32], n: usize) {
    if n < 2 { return; }
    let lo = lo_len(n);
    // Re-interleave: [0, lo) → evens, [lo, n) → odds.
    let mut tmp = vec![0f32; n];
    let (mut e, mut o) = (0usize, lo);
    for i in 0..n {
        if i & 1 == 0 { tmp[i] = buf[e]; e += 1; }
        else          { tmp[i] = buf[o]; o += 1; }
    }
    buf[..n].copy_from_slice(&tmp[..n]);
    // Exact reverse of forward: undo update (evens) then undo predict (odds).
    let mut k = 0;
    while k < n {
        let left  = if k >= 1    { buf[k - 1] } else { buf[1] };
        let right = if k + 1 < n { buf[k + 1] } else { buf[k - 1] };
        buf[k] -= 0.25 * (left + right);
        k += 2;
    }
    let mut k = 1;
    while k < n {
        let left  = buf[k - 1];
        let right = if k + 1 < n { buf[k + 1] } else { buf[k - 1] };
        buf[k] += 0.5 * (left + right);
        k += 2;
    }
}

// ── 2D DWT (separable, row-major, in-place) ──────────────────────────────────
//
// Subband layout after forward DWT (Mallat scheme, 1-indexed levels):
//
//   At level L, the subbands occupy the following rows/cols in the flat array:
//     region width  rw = lo_len^(L-1)(W),  height rh = lo_len^(L-1)(H)
//     subband split sw = lo_len(rw),       sh = lo_len(rh)   (ceil; see lo_len)
//     LL: rows [0,sh)  cols [0,sw)     — smooth approximation
//     HL: rows [0,sh)  cols [sw,rw)    — vertical edges  (row-high, col-low)
//     LH: rows [sh,rh) cols [0,sw)    — horizontal edges (row-low, col-high)
//     HH: rows [sh,rh) cols [sw,rw)   — diagonal detail

fn apply_rows(data: &mut [f32], stride: usize, w: usize, h: usize, fwd: bool) {
    let mut row = vec![0f32; w];
    for r in 0..h {
        let base = r * stride;
        row[..w].copy_from_slice(&data[base..base + w]);
        if fwd { dwt_1d_fwd(&mut row, w); } else { dwt_1d_inv(&mut row, w); }
        data[base..base + w].copy_from_slice(&row[..w]);
    }
}

fn apply_cols(data: &mut [f32], stride: usize, w: usize, h: usize, fwd: bool) {
    let mut col = vec![0f32; h];
    for c in 0..w {
        for r in 0..h { col[r] = data[r * stride + c]; }
        if fwd { dwt_1d_fwd(&mut col, h); } else { dwt_1d_inv(&mut col, h); }
        for r in 0..h { data[r * stride + c] = col[r]; }
    }
}

/// Forward multi-level 2D DWT. Each level subdivides the LL subband.
pub fn dwt_2d_fwd(data: &mut [f32], width: usize, height: usize, levels: u32) {
    let mut w = width;
    let mut h = height;
    for _ in 0..levels {
        if w < 2 || h < 2 { break; }
        apply_rows(data, width, w, h, true);
        apply_cols(data, width, w, h, true);
        w = lo_len(w);
        h = lo_len(h);
    }
}

/// Inverse multi-level 2D DWT. Pass the same `levels` as the forward call.
pub fn dwt_2d_inv(data: &mut [f32], width: usize, height: usize, levels: u32) {
    let mut sizes: Vec<(usize, usize)> = Vec::new();
    let mut w = width;
    let mut h = height;
    for _ in 0..levels {
        if w < 2 || h < 2 { break; }
        sizes.push((w, h));
        w = lo_len(w);
        h = lo_len(h);
    }
    for &(w, h) in sizes.iter().rev() {
        apply_cols(data, width, w, h, false);
        apply_rows(data, width, w, h, false);
    }
}

// ── Subband bounds ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Subband { LL, HL, LH, HH }

/// Returns `(row_start, row_end, col_start, col_end)` for a named subband.
/// `level` is 1-indexed: 1 = subbands produced by the first DWT pass.
pub fn subband_bounds(
    width: usize, height: usize, level: u32, band: Subband,
) -> (usize, usize, usize, usize) {
    debug_assert!(level >= 1);
    let mut rw = width;
    let mut rh = height;
    for _ in 1..level { rw = lo_len(rw); rh = lo_len(rh); }
    let sw = lo_len(rw);
    let sh = lo_len(rh);
    match band {
        Subband::LL => (0,  sh,  0,  sw),
        Subband::HL => (0,  sh,  sw, rw),
        Subband::LH => (sh, rh,  0,  sw),
        Subband::HH => (sh, rh,  sw, rw),
    }
}

/// Coefficient count for one subband at the given level.
pub fn subband_len(width: usize, height: usize, level: u32) -> usize {
    let (r0, r1, c0, c1) = subband_bounds(width, height, level, Subband::LL);
    (r1 - r0) * (c1 - c0)
}

// ── Phase 2: PN generation, embedding, blind decoding ────────────────────────

// ── XorShift64 PRNG ──────────────────────────────────────────────────────────

#[inline]
fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

// ── PN tile ───────────────────────────────────────────────────────────────────
//
// Each payload bit gets a dedicated TILE_SIDE×TILE_SIDE PN pattern of ±1 values
// seeded from WM_KEY and the bit index.  The tile is repeated across the 2D
// subband, giving crop robustness: any surviving sub-region contains complete tiles.

fn pn_tile(bit_idx: usize) -> Vec<f32> {
    let tile_len = TILE_SIDE * TILE_SIDE;
    // Spread bit_idx so adjacent indices produce uncorrelated sequences.
    let mut state = WM_KEY ^ (bit_idx as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for _ in 0..16 { xorshift64(&mut state); }   // warm-up
    (0..tile_len)
        .map(|_| if xorshift64(&mut state) & 1 == 0 { 1.0f32 } else { -1.0 })
        .collect()
}

// ── Payload ↔ bits ────────────────────────────────────────────────────────────

fn payload_to_bits(payload: &[u8; 16]) -> [bool; PAYLOAD_BITS] {
    let mut bits = [false; PAYLOAD_BITS];
    for (b, &byte) in payload.iter().enumerate() {
        for k in 0..8 { bits[b * 8 + k] = (byte >> k) & 1 == 1; }
    }
    bits
}

fn bits_to_payload(bits: &[bool; PAYLOAD_BITS]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (b, byte) in out.iter_mut().enumerate() {
        for k in 0..8 { if bits[b * 8 + k] { *byte |= 1 << k; } }
    }
    out
}

// ── Subband embed / decode ────────────────────────────────────────────────────
//
// Embedding: for each bit b with sign s_b = ±1, every coefficient at subband
// position (r, c) receives:
//   coeff += ALPHA * s_b * pn_b[ tile_idx(r, c) ]
//
// tile_idx tiles a TILE_SIDE×TILE_SIDE PN grid across the subband by *repetition*
// (modulo), so each coefficient gets its own PN value with period TILE_SIDE:
//   tile_idx = ((r % TILE_SIDE) * TILE_SIDE) + (c % TILE_SIDE)
//
// Per-coefficient (rather than block-stretched) PN keeps the spatial texture fine
// and pseudo-random instead of a coarse regular lattice.  The repeating tile also
// gives crop robustness — any surviving region still contains whole tiles.
//
// Resize robustness does NOT depend on the tiling: the size-informed decoder
// (`decode_y_at_size`) resamples a suspect back to the original embedding
// dimensions, so each subband regains its original size and the modulo indices
// reproduce exactly.  (A normalized/stretched tiling was tried for an abandoned
// decode-at-arbitrary-size approach; it bought nothing here and produced a
// visible ~39px lattice — the "meat tenderizer" — so it was reverted.)
//
// We precompute a single "weighted tile" combining all bits' contributions,
// reducing the per-coefficient inner loop from O(PAYLOAD_BITS) to O(1).
//
// Detection: accumulate subband coefficients by tile index, then correlate
// tile_sums with each PN tile.  Image content averages out; the watermark
// signal integrates to ≈ ALPHA * s_b per bit.

// 3×3 box blur (edge-clamped) of an `w×h` map — smooths the activity estimate so
// the masking gain doesn't itself become a high-frequency texture.
fn box_blur_3x3(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    for i in 0..h {
        for j in 0..w {
            let mut sum = 0.0f32;
            let mut cnt = 0.0f32;
            for di in -1i32..=1 {
                for dj in -1i32..=1 {
                    let ni = i as i32 + di;
                    let nj = j as i32 + dj;
                    if ni >= 0 && ni < h as i32 && nj >= 0 && nj < w as i32 {
                        sum += src[ni as usize * w + nj as usize];
                        cnt += 1.0;
                    }
                }
            }
            out[i * w + j] = sum / cnt;
        }
    }
    out
}

/// Per-level perceptual-masking gain map (one value per detail-coefficient
/// position, `sh×sw`).  Gain derives from co-located detail energy
/// (|LH|+|HL|+|HH|, 3×3-smoothed): the watermark is scaled DOWN where the image
/// is smooth (grain visible, eye sensitive) and UP where it is busy (content
/// masks it).  Renormalized to mean 1, so total embedding energy — hence
/// detection strength — matches uniform embedding; only the distribution changes.
///
/// Computed from the forward-DWT coefficients *before* this level is modified,
/// so it reflects the original image content.
fn masking_gain(coeffs: &[f32], width: usize, height: usize, level: u32, mask_strength: f32) -> (Vec<f32>, usize, usize) {
    let mut rw = width;
    let mut rh = height;
    for _ in 1..level { rw /= 2; rh /= 2; }
    let sw = rw / 2;
    let sh = rh / 2;
    if sh == 0 || sw == 0 { return (vec![1.0], 1, 1); }

    // Co-located detail energy.  Subband offsets at this level:
    //   LH: rows [sh,·) cols [0,·)   HL: rows [0,·) cols [sw,·)   HH: rows [sh,·) cols [sw,·)
    let mut act = vec![0.0f32; sh * sw];
    for i in 0..sh {
        for j in 0..sw {
            let lh = coeffs[(sh + i) * width + j].abs();
            let hl = coeffs[i * width + (sw + j)].abs();
            let hh = coeffs[(sh + i) * width + (sw + j)].abs();
            act[i * sw + j] = lh + hl + hh;
        }
    }
    let act = box_blur_3x3(&act, sw, sh);

    let mean_act = (act.iter().sum::<f32>() / (sh * sw) as f32).max(1e-6);

    // Map activity → gain, clamp, then renormalize to mean 1.
    let mut gain = vec![0.0f32; sh * sw];
    let mut gsum = 0.0f32;
    for (g, &a) in gain.iter_mut().zip(act.iter()) {
        *g = (a / mean_act).powf(MASK_GAMMA).clamp(MASK_FLOOR, MASK_CEIL);
        gsum += *g;
    }
    let renorm = (sh * sw) as f32 / gsum.max(1e-6);
    // Renormalize to mean 1, then blend toward uniform (1.0) by `mask_strength`.
    // The blend stays mean-1 (energy-neutral) and tames the energy that full
    // masking piles onto edges — which otherwise reads like JPEG ringing.
    for g in gain.iter_mut() {
        let normalized = *g * renorm;
        *g = 1.0 + mask_strength * (normalized - 1.0);
    }
    (gain, sh, sw)
}

fn embed_in_subband(
    data:   &mut [f32],
    stride: usize,
    r0: usize, r1: usize, c0: usize, c1: usize,
    bits:   &[bool; PAYLOAD_BITS],
    gain:   &[f32], gsh: usize, gsw: usize,
) {
    let tile_len = TILE_SIDE * TILE_SIDE;
    // Build weighted sum of all PN tiles: weighted[t] = Σ_b ( sign_b · pn_b[t] )
    let mut weighted = vec![0.0f32; tile_len];
    for bit_idx in 0..PAYLOAD_BITS {
        let sign = if bits[bit_idx] { 1.0f32 } else { -1.0 };
        let tile = pn_tile(bit_idx);
        for t in 0..tile_len { weighted[t] += sign * tile[t]; }
    }
    // Apply weighted pattern × per-coefficient masking gain.  PN grid is tiled by
    // repetition (modulo); the gain map is co-located (clamped at odd-size edges).
    for row in r0..r1 {
        let gi = (row - r0).min(gsh - 1);
        for col in c0..c1 {
            let gj = (col - c0).min(gsw - 1);
            let ti = ((row - r0) % TILE_SIDE) * TILE_SIDE + (col - c0) % TILE_SIDE;
            let g  = gain[gi * gsw + gj];
            data[row * stride + col] += ALPHA * g * weighted[ti];
        }
    }
}

// Returns normalised correlation per bit: positive → 1, negative → 0.
fn decode_subband(
    data:   &[f32],
    stride: usize,
    r0: usize, r1: usize, c0: usize, c1: usize,
) -> [f32; PAYLOAD_BITS] {
    let tile_len = TILE_SIDE * TILE_SIDE;
    // Accumulate subband coefficients by their (modulo) tile position.
    // tile_sums[t] = Σ_{(r,c): tile_idx(r,c)=t} coeff[r,c]
    let mut tile_sums = vec![0.0f32; tile_len];
    for row in r0..r1 {
        for col in c0..c1 {
            let ti = ((row - r0) % TILE_SIDE) * TILE_SIDE + (col - c0) % TILE_SIDE;
            tile_sums[ti] += data[row * stride + col];
        }
    }
    // corr_b = (1/N) · Σ_t ( tile_sums[t] · pn_b[t] ) ≈ ALPHA · sign_b
    let n = ((r1 - r0) * (c1 - c0)) as f32;
    let mut corrs = [0.0f32; PAYLOAD_BITS];
    for bit_idx in 0..PAYLOAD_BITS {
        let tile = pn_tile(bit_idx);
        corrs[bit_idx] = tile_sums.iter().zip(tile.iter())
            .map(|(&s, &t)| s * t)
            .sum::<f32>() / n;
    }
    corrs
}

// ── Resampling (decode-time scale search) ─────────────────────────────────────
//
// Separable triangle-filter resampler over the f32 Y plane.  Dependency-free so
// it works in both the WASM build and the native CLI.  Used only by the scale
// search in `decode_y_search`; the embed path never resamples.
//
// The triangle support widens with the downscale ratio, giving a low-pass that
// suppresses aliasing — important because the search must preserve the
// mid-frequency band the watermark lives in.

fn axis_contribs(src_len: usize, dst_len: usize) -> Vec<(usize, Vec<f32>)> {
    let scale   = src_len as f32 / dst_len as f32;
    let support = scale.max(1.0);            // triangle half-width in source px
    let mut contribs = Vec::with_capacity(dst_len);
    for o in 0..dst_len {
        let center = (o as f32 + 0.5) * scale - 0.5;
        let left   = (center - support).ceil().max(0.0) as usize;
        let right  = (center + support).floor().min(src_len as f32 - 1.0).max(0.0) as usize;
        let mut weights = Vec::with_capacity(right - left + 1);
        let mut sum = 0.0f32;
        for s in left..=right {
            let w = (1.0 - (s as f32 - center).abs() / support).max(0.0);
            weights.push(w);
            sum += w;
        }
        if sum > 0.0 { for w in &mut weights { *w /= sum; } }
        contribs.push((left, weights));
    }
    contribs
}

/// Resample an f32 Y plane from `sw×sh` to `dw×dh` (separable, triangle filter).
fn resample_y(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    if sw == dw && sh == dh { return src.to_vec(); }
    // Horizontal pass: (sw × sh) → (dw × sh)
    let hc = axis_contribs(sw, dw);
    let mut tmp = vec![0.0f32; dw * sh];
    for r in 0..sh {
        let row = &src[r * sw..r * sw + sw];
        for (o, (start, weights)) in hc.iter().enumerate() {
            let mut acc = 0.0f32;
            for (k, &w) in weights.iter().enumerate() { acc += row[start + k] * w; }
            tmp[r * dw + o] = acc;
        }
    }
    // Vertical pass: (dw × sh) → (dw × dh)
    let vc = axis_contribs(sh, dh);
    let mut out = vec![0.0f32; dw * dh];
    for (o, (start, weights)) in vc.iter().enumerate() {
        for x in 0..dw {
            let mut acc = 0.0f32;
            for (k, &w) in weights.iter().enumerate() { acc += tmp[(start + k) * dw + x] * w; }
            out[o * dw + x] = acc;
        }
    }
    out
}

// ── Public embed / decode ─────────────────────────────────────────────────────

/// Embed `payload` (16 bytes = 128 bits) into the Y channel in-place.
/// `y` is a row-major f32 array of `width × height` luminance values.
pub fn embed_y(y: &mut [f32], width: usize, height: usize, payload: &[u8; 16]) {
    embed_y_masked(y, width, height, payload, MASK_STRENGTH);
}

/// Like `embed_y` but with an explicit perceptual-masking blend strength
/// (0 = uniform, 1 = full). Exposed so experiments can compare masked vs uniform
/// embedding (e.g. its effect on the watermark's self-synchronizing periodicity).
pub fn embed_y_masked(y: &mut [f32], width: usize, height: usize, payload: &[u8; 16], mask_strength: f32) {
    dwt_2d_fwd(y, width, height, DECOMP_DEPTH);
    let bits = payload_to_bits(payload);
    for &level in EMBED_LEVELS {
        // Masking gain from this level's detail energy, before it is modified.
        let (gain, gsh, gsw) = masking_gain(y, width, height, level, mask_strength);
        for &band in &[Subband::LH, Subband::HL] {
            let (r0, r1, c0, c1) = subband_bounds(width, height, level, band);
            if r1 > r0 && c1 > c0 {
                embed_in_subband(y, width, r0, r1, c0, c1, &bits, &gain, gsh, gsw);
            }
        }
    }
    dwt_2d_inv(y, width, height, DECOMP_DEPTH);
}

/// Blindly decode the watermark payload from the Y channel (no original needed).
/// Aggregates correlation evidence across all embedded subbands and levels.
pub fn decode_y(y: &[f32], width: usize, height: usize) -> [u8; 16] {
    let total = correlate_embed_levels(y, width, height);
    let mut bits = [false; PAYLOAD_BITS];
    for (i, b) in bits.iter_mut().enumerate() { *b = total[i] > 0.0; }
    bits_to_payload(&bits)
}

/// Accumulate per-bit correlation over exactly the embedded subbands
/// (`EMBED_LEVELS` × {LH, HL}).  This is the matched decoder for an image on the
/// *same pixel grid* it was embedded on — values are ≈ ALPHA·sign_b per bit.
fn correlate_embed_levels(y: &[f32], width: usize, height: usize) -> [f32; PAYLOAD_BITS] {
    let mut coeffs = y.to_vec();
    dwt_2d_fwd(&mut coeffs, width, height, DECOMP_DEPTH);

    let mut total = [0.0f32; PAYLOAD_BITS];
    for &level in EMBED_LEVELS {
        for &band in &[Subband::LH, Subband::HL] {
            let (r0, r1, c0, c1) = subband_bounds(width, height, level, band);
            if r1 > r0 && c1 > c0 {
                let corrs = decode_subband(&coeffs, width, r0, r1, c0, c1);
                for i in 0..PAYLOAD_BITS { total[i] += corrs[i]; }
            }
        }
    }
    total
}

/// Size-informed decoder — recovers the payload after **any** scale factor
/// (arbitrary, not just power-of-2) given the original embedding dimensions.
///
/// The critically-sampled DWT is shift-variant, so a watermark is only
/// recoverable on the exact pixel grid it was embedded on.  A rescaled suspect
/// has lost that grid; this decoder restores it by resampling the suspect back
/// to `(orig_w, orig_h)` before running the matched decoder.  The alignment peak
/// is extremely sharp (correct size → strong; off by ~2% → noise), so the
/// original dimensions must be known — which, for a known gallery source image,
/// they are.
///
/// Returns the recovered payload.  Use `decode_y_at_size_verbose` for the
/// alignment score (a detection-confidence / gallery-matching metric).
pub fn decode_y_at_size(y: &[f32], width: usize, height: usize, orig_w: usize, orig_h: usize) -> [u8; 16] {
    decode_y_at_size_verbose(y, width, height, orig_w, orig_h).0
}

/// Alignment-score thresholds for interpreting `decode_y_at_size_verbose`.
/// Scores scale with `ALPHA` and the number of embedded subbands, so these track
/// `ALPHA` rather than being hardcoded.  Empirically (ALPHA=0.15, levels [2,3]):
/// clean detections score ~37–60, the off-grid / no-watermark floor is ~6–15.
///   score ≥ `detection_strong()` → confident match
///   score ≥ `detection_floor()`  → weak (partial: wrong size or heavy distortion)
///   below                        → not detected
pub fn detection_strong() -> f32 { PAYLOAD_BITS as f32 * ALPHA * 1.6 }
pub fn detection_floor()  -> f32 { PAYLOAD_BITS as f32 * ALPHA * 0.9 }

/// Like `decode_y_at_size` but also returns the alignment score — the L1 norm of
/// the per-bit correlation vector.  A high score (≫ the off-grid noise floor)
/// indicates a confident detection at this candidate size; trying several known
/// gallery sizes and taking the peak identifies which source image leaked.
pub fn decode_y_at_size_verbose(
    y: &[f32], width: usize, height: usize, orig_w: usize, orig_h: usize,
) -> ([u8; 16], f32) {
    let regridded = resample_y(y, width, height, orig_w, orig_h);
    let total = correlate_embed_levels(&regridded, orig_w, orig_h);
    let score: f32 = total.iter().map(|v| v.abs()).sum();
    let mut bits = [false; PAYLOAD_BITS];
    for (i, b) in bits.iter_mut().enumerate() { *b = total[i] > 0.0; }
    (bits_to_payload(&bits), score)
}

// ── Debug / visual verification ──────────────────────────────────────────────

/// Build a grayscale RGB image (3 bytes/pixel) amplifying the Y-channel delta
/// by `amp`, centered at 128.  Negative deltas → dark, positive → bright.
/// Call emit_residual(&orig_y, &watermarked_y, RESIDUAL_AMP) then save as PNG
/// to visually confirm that the watermark is imperceptible and structurally sane.
pub fn emit_residual(y_orig: &[f32], y_new: &[f32], amp: f32) -> Vec<u8> {
    y_orig.iter().zip(y_new.iter())
        .flat_map(|(&a, &b)| {
            let v = ((b - a) * amp + 128.0).clamp(0.0, 255.0) as u8;
            [v, v, v]
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn max_err(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).fold(0f32, f32::max)
    }

    fn psnr(orig: &[f32], modified: &[f32]) -> f32 {
        let mse: f32 = orig.iter().zip(modified.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>() / orig.len() as f32;
        if mse < 1e-10 { return f32::INFINITY; }
        20.0 * (255.0f32 / mse.sqrt()).log10()
    }

    // ── Phase 1: DWT round-trips ─────────────────────────────────────────────

    #[test]
    fn dwt_1d_even_roundtrip() {
        let orig: Vec<f32> = (0..8).map(|i| i as f32 * 13.7 + 5.0).collect();
        let mut buf = orig.clone();
        dwt_1d_fwd(&mut buf, 8);
        assert_ne!(buf, orig);
        dwt_1d_inv(&mut buf, 8);
        assert!(max_err(&orig, &buf) < 1e-5, "err={}", max_err(&orig, &buf));
    }

    #[test]
    fn dwt_1d_odd_roundtrip() {
        let orig: Vec<f32> = (0..7).map(|i| i as f32 * 9.1 + 3.0).collect();
        let mut buf = orig.clone();
        dwt_1d_fwd(&mut buf, 7);
        dwt_1d_inv(&mut buf, 7);
        assert!(max_err(&orig, &buf) < 1e-5, "err={}", max_err(&orig, &buf));
    }

    #[test]
    fn dwt_1d_constant_signal() {
        let mut buf = vec![5.0f32; 8];
        dwt_1d_fwd(&mut buf, 8);
        for &d in &buf[4..8] {
            assert!(d.abs() < 1e-6, "detail non-zero for constant: {}", d);
        }
    }

    #[test]
    fn dwt_2d_small_1level() {
        let (w, h) = (8, 6);
        let orig: Vec<f32> = (0..w * h).map(|i| (i % 11) as f32 * 7.3 + 1.0).collect();
        let mut data = orig.clone();
        dwt_2d_fwd(&mut data, w, h, 1);
        assert_ne!(data, orig);
        dwt_2d_inv(&mut data, w, h, 1);
        assert!(max_err(&orig, &data) < 1e-4, "err={}", max_err(&orig, &data));
    }

    #[test]
    fn dwt_2d_multi_level() {
        let (w, h) = (32, 24);
        let orig: Vec<f32> = (0..w * h).map(|i| (i % 17) as f32 * 11.9 + 4.0).collect();
        let mut data = orig.clone();
        dwt_2d_fwd(&mut data, w, h, 3);
        dwt_2d_inv(&mut data, w, h, 3);
        assert!(max_err(&orig, &data) < 1e-3, "err={}", max_err(&orig, &data));
    }

    #[test]
    fn dwt_2d_photo_scale() {
        let (w, h) = (256, 256);
        let orig: Vec<f32> = (0..w * h).map(|i| {
            let x = (i % w) as f32;
            let y = (i / w) as f32;
            ((x * 0.05 + y * 0.03).sin() * 64.0 + 128.0).clamp(0.0, 255.0)
        }).collect();
        let mut data = orig.clone();
        dwt_2d_fwd(&mut data, w, h, 4);
        dwt_2d_inv(&mut data, w, h, 4);
        let err = max_err(&orig, &data);
        println!("photo_scale round-trip max error: {:.2e}", err);
        assert!(err < 1.0, "err={} exceeds 1 LSB", err);
    }

    #[test]
    fn dwt_2d_non_power_of_two() {
        let (w, h) = (120, 80);
        let orig: Vec<f32> = (0..w * h).map(|i| (i % 23) as f32 * 5.5 + 10.0).collect();
        let mut data = orig.clone();
        dwt_2d_fwd(&mut data, w, h, 4);
        dwt_2d_inv(&mut data, w, h, 4);
        assert!(max_err(&orig, &data) < 1.0, "err={}", max_err(&orig, &data));
    }

    #[test]
    fn subband_bounds_level1_16x8() {
        assert_eq!(subband_bounds(16, 8, 1, Subband::LL), (0, 4, 0,  8));
        assert_eq!(subband_bounds(16, 8, 1, Subband::HL), (0, 4, 8,  16));
        assert_eq!(subband_bounds(16, 8, 1, Subband::LH), (4, 8, 0,  8));
        assert_eq!(subband_bounds(16, 8, 1, Subband::HH), (4, 8, 8,  16));
    }

    #[test]
    fn subband_bounds_level3_3000x2000() {
        assert_eq!(subband_bounds(3000, 2000, 3, Subband::LL), (0,   250, 0,   375));
        assert_eq!(subband_bounds(3000, 2000, 3, Subband::HL), (0,   250, 375, 750));
        assert_eq!(subband_bounds(3000, 2000, 3, Subband::LH), (250, 500, 0,   375));
        assert_eq!(subband_bounds(3000, 2000, 3, Subband::HH), (250, 500, 375, 750));
    }

    #[test]
    fn subband_len_matches_coefficient_count() {
        assert_eq!(subband_len(3000, 2000, 3), 375 * 250);
        assert_eq!(subband_len(3000, 2000, 4), 188 * 125); // ceil split: lo_len(375)=188
    }

    #[test]
    fn extract_y_values() {
        let pixels = [255u8, 0, 0, 255,   0, 255, 0, 255];
        let y = extract_y(&pixels);
        assert!((y[0] - 76.245).abs() < 0.01, "red Y={}", y[0]);
        assert!((y[1] - 149.685).abs() < 0.01, "green Y={}", y[1]);
    }

    #[test]
    fn write_y_delta_roundtrip() {
        let mut pixels = vec![100u8, 120, 80, 255,  200, 180, 160, 255];
        let y_orig = extract_y(&pixels);
        let y_new: Vec<f32> = y_orig.iter().map(|&v| v + 3.0).collect();
        write_y_delta(&mut pixels, &y_orig, &y_new);
        assert_eq!(pixels[0], 103);
        assert_eq!(pixels[1], 123);
        assert_eq!(pixels[2], 83);
        assert_eq!(pixels[3], 255, "alpha must not change");
    }

    // ── Phase 2: embed + immediate decode ────────────────────────────────────
    //
    // Reliable blind decode requires N >> PAYLOAD_BITS per embedded subband.
    // SNR ≈ √(N / PAYLOAD_BITS).  For 128-bit payloads:
    //   256×256 → LH3 = 1024 coeff → SNR ≈ 2.8  (too small, errors expected)
    //   1024×768 → LH3 = 12288   → SNR ≈ 9.8  (reliable)
    // Synthetic tests therefore use 1024×768 minimum.

    fn synthetic_y(w: usize, h: usize) -> Vec<f32> {
        (0..w * h).map(|i| {
            let x = (i % w) as f32;
            let yc = (i / w) as f32;
            ((x * 0.07 + yc * 0.05).sin() * 64.0 + 128.0).clamp(0.0, 255.0)
        }).collect()
    }

    #[test]
    fn embed_decode_all_zeros_payload() {
        let (w, h) = (1024, 768);
        let orig = vec![128.0f32; w * h];
        let payload = [0u8; 16];
        let mut y = orig.clone();
        embed_y(&mut y, w, h, &payload);
        assert_eq!(decode_y(&y, w, h), payload);
    }

    #[test]
    fn embed_decode_all_ones_payload() {
        let (w, h) = (1024, 768);
        let orig = vec![128.0f32; w * h];
        let payload = [0xFFu8; 16];
        let mut y = orig.clone();
        embed_y(&mut y, w, h, &payload);
        assert_eq!(decode_y(&y, w, h), payload);
    }

    #[test]
    fn embed_decode_synthetic_exact() {
        // Minimum reliable size scales with ALPHA: need SNR = ALPHA×sqrt(N/128) >> 1.
        // At ALPHA=0.3, LH3 needs ≥~50k coefficients → 2048×1536 (LH3=49,152, SNR≈9).
        let (w, h) = (2048, 1536);
        let orig = synthetic_y(w, h);
        let payload: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
            0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10,
        ];
        let mut y = orig.clone();
        embed_y(&mut y, w, h, &payload);
        let p = psnr(&orig, &y);
        println!("synthetic {}×{}  PSNR: {:.1} dB  (alpha={})", w, h, p, ALPHA);
        assert_eq!(decode_y(&y, w, h), payload, "bit-perfect decode failed");
    }

    #[test]
    fn embed_decode_image_a() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .join("tests")
            .join("test_a.jpg");

        let img = image::open(&path)
            .unwrap_or_else(|e| panic!("could not open {}: {}", path.display(), e))
            .into_rgb8();
        let (w, h) = (img.width() as usize, img.height() as usize);

        let orig_y: Vec<f32> = img.pixels()
            .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
            .collect();

        let payload: [u8; 16] = [
            0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE,
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
        ];

        let mut y = orig_y.clone();
        embed_y(&mut y, w, h, &payload);

        let p = psnr(&orig_y, &y);
        println!("image_a.jpg ({}×{})  PSNR: {:.1} dB  (alpha={})", w, h, p, ALPHA);
        assert!(p > 20.0, "PSNR {:.1} dB is suspiciously low", p);

        assert_eq!(decode_y(&y, w, h), payload, "payload mismatch on image_a.jpg");
    }

    // ── Phase 3: JPEG roundtrip ───────────────────────────────────────────────
    //
    // Embed watermark into test_a.jpg, JPEG-compress at quality 90 / 80 / 70,
    // reload, decode, count bit errors.  Requirement: 0 errors at q≥80.
    // ALPHA tuning: raise ALPHA if errors appear; lower if PSNR drops below ~30 dB.
    // Residual image (amplified delta ×RESIDUAL_AMP) saved to tests/residual_wm.png
    // on q90 run for visual confirmation that the watermark is imperceptible.

    fn extract_y_rgb(pixels: &[u8]) -> Vec<f32> {
        pixels.chunks(3)
            .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
            .collect()
    }

    fn write_y_delta_rgb(pixels: &mut [u8], y_orig: &[f32], y_new: &[f32]) {
        for (chunk, (&yo, &yn)) in pixels.chunks_mut(3).zip(y_orig.iter().zip(y_new.iter())) {
            let d = yn - yo;
            chunk[0] = (chunk[0] as f32 + d).clamp(0.0, 255.0) as u8;
            chunk[1] = (chunk[1] as f32 + d).clamp(0.0, 255.0) as u8;
            chunk[2] = (chunk[2] as f32 + d).clamp(0.0, 255.0) as u8;
        }
    }

    const PHASE3_PAYLOAD: [u8; 16] = [
        0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE,
        0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
    ];

    /// Embed watermark into test_a.jpg, round-trip through JPEG at `quality`, decode.
    /// Returns (bit_errors, recovered_payload).  Saves residual PNG on quality=90.
    fn jpeg_roundtrip(quality: u8) -> (usize, [u8; 16]) {
        use image::{codecs::jpeg::JpegEncoder, ColorType, ExtendedColorType, ImageEncoder};

        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .join("tests");
        let path = tests_dir.join("test_a.jpg");

        let img = image::open(&path)
            .unwrap_or_else(|e| panic!("cannot open {}: {}", path.display(), e))
            .into_rgb8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let pixels = img.into_raw();

        // Extract Y, embed, write delta back into a pixel copy.
        let orig_y = extract_y_rgb(&pixels);
        let mut wm_y = orig_y.clone();
        embed_y(&mut wm_y, w, h, &PHASE3_PAYLOAD);
        let mut pixels_wm = pixels.clone();
        write_y_delta_rgb(&mut pixels_wm, &orig_y, &wm_y);

        let p = psnr(&orig_y, &wm_y);

        // Save amplified residual once for visual inspection.
        if quality == 90 {
            let residual = emit_residual(&orig_y, &wm_y, RESIDUAL_AMP);
            let rpath = tests_dir.join("residual_wm.png");
            image::save_buffer(&rpath, &residual, w as u32, h as u32, ColorType::Rgb8).ok();
            println!("residual → {}  PSNR={:.1} dB", rpath.display(), p);
        }

        // JPEG-encode the watermarked image.
        let mut jpeg_bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg_bytes, quality)
            .write_image(&pixels_wm, w as u32, h as u32, ExtendedColorType::Rgb8)
            .unwrap();

        // Decode and run blind detector.
        let decoded = image::load_from_memory(&jpeg_bytes).unwrap().into_rgb8();
        let decoded_y = extract_y_rgb(decoded.as_raw());
        let recovered = decode_y(&decoded_y, w, h);

        let errors: usize = PHASE3_PAYLOAD.iter().zip(recovered.iter())
            .map(|(&a, &b)| (a ^ b).count_ones() as usize)
            .sum();

        println!(
            "JPEG q{}  {}×{}  errors={}/{}  BER={:.4}  PSNR={:.1}dB  (alpha={})",
            quality, w, h, errors, PAYLOAD_BITS,
            errors as f64 / PAYLOAD_BITS as f64,
            p, ALPHA,
        );

        (errors, recovered)
    }

    #[test]
    fn jpeg_roundtrip_q90() {
        let (errors, recovered) = jpeg_roundtrip(90);
        assert_eq!(recovered, PHASE3_PAYLOAD,
            "q90: {errors}/{PAYLOAD_BITS} bit errors — raise ALPHA if this fails");
    }

    #[test]
    fn jpeg_roundtrip_q80() {
        let (errors, recovered) = jpeg_roundtrip(80);
        assert_eq!(recovered, PHASE3_PAYLOAD,
            "q80: {errors}/{PAYLOAD_BITS} bit errors — raise ALPHA if this fails");
    }

    #[test]
    fn jpeg_roundtrip_q70() {
        // q70 is below the stated requirement; informational only, no assertion.
        let (errors, _) = jpeg_roundtrip(70);
        println!("q70 diagnostic: {errors}/{PAYLOAD_BITS} errors (no assertion)");
    }

    /// Emit before/after sample images for visual (eyeball) quality assessment at
    /// the current ALPHA / EMBED_LEVELS.  Writes lossless PNGs so JPEG artifacts
    /// don't confound the comparison:
    ///   tests/sample_original.png    — original Y rendered to RGB
    ///   tests/sample_watermarked.png — watermarked (what the viewer sees)
    ///   tests/sample_residual.png    — amplified delta (×RESIDUAL_AMP), structure
    #[test]
    fn emit_visual_samples() {
        use image::ColorType;

        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join("tests");
        let img = image::open(tests_dir.join("test_a.jpg")).unwrap().into_rgb8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let pixels = img.into_raw();

        let orig_y = extract_y_rgb(&pixels);
        let mut wm_y = orig_y.clone();
        embed_y(&mut wm_y, w, h, &PHASE3_PAYLOAD);

        let mut pixels_wm = pixels.clone();
        write_y_delta_rgb(&mut pixels_wm, &orig_y, &wm_y);

        let residual = emit_residual(&orig_y, &wm_y, RESIDUAL_AMP);

        image::save_buffer(tests_dir.join("sample_original.png"),
            &pixels, w as u32, h as u32, ColorType::Rgb8).unwrap();
        image::save_buffer(tests_dir.join("sample_watermarked.png"),
            &pixels_wm, w as u32, h as u32, ColorType::Rgb8).unwrap();
        image::save_buffer(tests_dir.join("sample_residual.png"),
            &residual, w as u32, h as u32, ColorType::Rgb8).unwrap();

        let p = psnr(&orig_y, &wm_y);
        let max_d = orig_y.iter().zip(wm_y.iter())
            .map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        println!("visual samples → {}  PSNR={:.1} dB  max|Δ|={:.1} LSB  (alpha={}, levels={:?})",
            tests_dir.display(), p, max_d, ALPHA, EMBED_LEVELS);
    }

    // ── Phase 4: Resize robustness ────────────────────────────────────────────
    //
    // 2× downscale: DWT level k of original → level k-1 of scaled image.
    // LH3 (313×312 coefficients in 2500×2500) becomes LH2 in the 1250×1250 image,
    // same size and same PN tile grid — correlation is exact, 0 errors expected.
    //
    // 3× downscale: non-power-of-2, log₂(3)≈1.58 level shift.  LH3 energy spreads
    // across levels 1–2 of the 833×833 image.  Level-scanning decoder sums all
    // evidence; 0 errors expected given the large subband counts at those levels.

    /// Embed watermark into test_a.jpg, resize by (scale_num/scale_den),
    /// run level-scanning decode, return (bit_errors, recovered_payload).
    fn embed_resize_decode(scale_num: u32, scale_den: u32) -> (usize, [u8; 16]) {
        use image::imageops;

        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .join("tests");

        let img = image::open(tests_dir.join("test_a.jpg")).unwrap().into_rgb8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let pixels = img.into_raw();

        let orig_y = extract_y_rgb(&pixels);
        let mut wm_y = orig_y.clone();
        embed_y(&mut wm_y, w, h, &PHASE3_PAYLOAD);
        let mut pixels_wm = pixels.clone();
        write_y_delta_rgb(&mut pixels_wm, &orig_y, &wm_y);

        let wm_img = image::RgbImage::from_raw(w as u32, h as u32, pixels_wm).unwrap();
        let new_w = (w as u64 * scale_num as u64 / scale_den as u64) as u32;
        let new_h = (h as u64 * scale_num as u64 / scale_den as u64) as u32;
        let scaled = imageops::resize(&wm_img, new_w, new_h, imageops::FilterType::Lanczos3);

        let (sw, sh) = (scaled.width() as usize, scaled.height() as usize);
        let scaled_y = extract_y_rgb(scaled.as_raw());
        // Size-informed decode: the original (embed) dimensions are known.
        let (recovered, score) = decode_y_at_size_verbose(&scaled_y, sw, sh, w, h);

        let errors: usize = PHASE3_PAYLOAD.iter().zip(recovered.iter())
            .map(|(&a, &b)| (a ^ b).count_ones() as usize)
            .sum();

        println!(
            "resize {}/{}  {}×{} → {}×{} → regrid {}×{}  errors={}/{}  score={:.1}  (alpha={})",
            scale_num, scale_den,
            w, h, sw, sh, w, h,
            errors, PAYLOAD_BITS, score, ALPHA,
        );

        (errors, recovered)
    }

    // All cases below decode via `decode_y_at_size` — the suspect is resampled
    // back to the known original dimensions (the embedding grid) before decoding.
    // The critically-sampled DWT is shift-variant, so this regridding is what
    // makes arbitrary (non-power-of-2) scale factors recoverable.  See
    // watermarking.md for why a blind size-search is not viable (peak too sharp).

    #[test]
    fn resize_50pct() {
        let (errors, recovered) = embed_resize_decode(1, 2);
        assert_eq!(recovered, PHASE3_PAYLOAD,
            "50% resize: {errors}/{PAYLOAD_BITS} errors");
    }

    #[test]
    fn resize_33pct() {
        // 3× downscale → 0.7 MP, *below* the stated requirement (~1 MP from a
        // 3–6 MP source = 0.4–0.6× linear).  At the Stage-1 low-visibility
        // settings (ALPHA=0.15, finer levels [2,3]) the embedded energy no longer
        // survives this aggressive a downscale + regrid round-trip.  Informational
        // only — recovering it would need higher ALPHA, coarser levels, or ECC.
        let (errors, _) = embed_resize_decode(1, 3);
        println!("resize 1/3 diagnostic: {errors}/{PAYLOAD_BITS} errors (below requirement — no assertion)");
    }

    #[test]
    fn resize_70pct() {
        let (errors, recovered) = embed_resize_decode(7, 10);
        assert_eq!(recovered, PHASE3_PAYLOAD,
            "70% resize: {errors}/{PAYLOAD_BITS} errors");
    }

    #[test]
    fn resize_60pct() {
        let (errors, recovered) = embed_resize_decode(3, 5);
        assert_eq!(recovered, PHASE3_PAYLOAD,
            "60% resize: {errors}/{PAYLOAD_BITS} errors");
    }

    #[test]
    fn resize_57pct() {
        // 4/7 ≈ 57.1% — an irregular, non-round scale factor.
        let (errors, recovered) = embed_resize_decode(4, 7);
        assert_eq!(recovered, PHASE3_PAYLOAD,
            "4/7 resize: {errors}/{PAYLOAD_BITS} errors");
    }

    #[test]
    fn resize_120pct() {
        // Upscale: regridding downsamples back to the original embed dimensions.
        let (errors, recovered) = embed_resize_decode(6, 5);
        assert_eq!(recovered, PHASE3_PAYLOAD,
            "120% upscale: {errors}/{PAYLOAD_BITS} errors");
    }

    #[test]
    fn wrong_size_does_not_false_positive() {
        // Decoding at the wrong original size must NOT yield a confident match:
        // the alignment score should collapse to the off-grid noise floor.
        use image::imageops;
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join("tests");
        let img = image::open(tests_dir.join("test_a.jpg")).unwrap().into_rgb8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let pixels = img.into_raw();
        let orig_y = extract_y_rgb(&pixels);
        let mut wm_y = orig_y.clone();
        embed_y(&mut wm_y, w, h, &PHASE3_PAYLOAD);
        let mut pixels_wm = pixels.clone();
        write_y_delta_rgb(&mut pixels_wm, &orig_y, &wm_y);
        let wm_img = image::RgbImage::from_raw(w as u32, h as u32, pixels_wm).unwrap();
        let scaled = imageops::resize(&wm_img, 1750, 1750, imageops::FilterType::Lanczos3);
        let scaled_y = extract_y_rgb(scaled.as_raw());

        let (_, good) = decode_y_at_size_verbose(&scaled_y, 1750, 1750, w, h);
        let (_, bad)  = decode_y_at_size_verbose(&scaled_y, 1750, 1750, w + 60, h + 60);
        println!("score @correct={:.1}  @wrong(+60px)={:.1}", good, bad);
        assert!(good > 3.0 * bad,
            "alignment peak not distinctive: correct={good:.1} wrong={bad:.1}");
    }

    // ── Crop tolerance characterization ───────────────────────────────────────
    //
    // Measures how much edge-cropping the watermark survives, via three decode
    // strategies, and writes tests/crop_tolerance.md.  Ignored by default (slow,
    // and it's a report generator, not a gate):
    //   cargo test -p glimr --release crop_tolerance -- --ignored --nocapture
    //
    //   A — resample to original (today's `decode_y_at_size`): stretch the cropped
    //       sub-rectangle back to the original dimensions.
    //   B — pad at the *known* crop offset (oracle): drop the cropped pixels at
    //       their original (x0,y0) in a full-size frame.  If this decodes, the
    //       signal SURVIVED the crop — registration is all that's missing.
    //   C — pad at offset 0: ignore the origin shift.  B-vs-C isolates translation.

    // Home for generated characterization reports/artifacts (created on demand).
    fn reports_dir() -> std::path::PathBuf {
        let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join("tests").join("reports");
        std::fs::create_dir_all(&d).ok();
        d
    }

    fn crop_errs(p: &[u8; 16]) -> usize {
        PHASE3_PAYLOAD.iter().zip(p.iter()).map(|(a, b)| (a ^ b).count_ones() as usize).sum()
    }

    // Copy a row-major RGB sub-rectangle out of a full-frame buffer.
    fn crop_rgb(src: &[u8], ow: usize, x0: usize, y0: usize, cw: usize, ch: usize) -> Vec<u8> {
        let mut out = vec![0u8; cw * ch * 3];
        for ry in 0..ch {
            let s = ((y0 + ry) * ow + x0) * 3;
            let d = ry * cw * 3;
            out[d..d + cw * 3].copy_from_slice(&src[s..s + cw * 3]);
        }
        out
    }

    // Place a cw×ch Y patch into a fresh ow×oh frame at (x0,y0); rest = `fill`.
    fn pad_y(patch: &[f32], cw: usize, ch: usize, ow: usize, oh: usize,
             x0: usize, y0: usize, fill: f32) -> Vec<f32> {
        let mut out = vec![fill; ow * oh];
        for ry in 0..ch {
            let s = ry * cw;
            let d = (y0 + ry) * ow + x0;
            out[d..d + cw].copy_from_slice(&patch[s..s + cw]);
        }
        out
    }

    #[test]
    #[ignore]
    fn crop_tolerance() {
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join("tests");
        let img = image::open(tests_dir.join("test_a.jpg")).unwrap().into_rgb8();
        let (ow, oh) = (img.width() as usize, img.height() as usize);
        let pixels = img.into_raw();

        // Embed once; every case crops this same watermarked buffer.
        let orig_y = extract_y_rgb(&pixels);
        let mut wm_y = orig_y.clone();
        embed_y(&mut wm_y, ow, oh, &PHASE3_PAYLOAD);
        let mut pixels_wm = pixels.clone();
        write_y_delta_rgb(&mut pixels_wm, &orig_y, &wm_y);

        // (label, left, top, right, bottom) = pixels removed per edge.
        let cases: &[(&str, usize, usize, usize, usize)] = &[
            ("1px right",       0, 0, 1, 0),
            ("1px bottom",      0, 0, 0, 1),
            ("1px left",        1, 0, 0, 0),
            ("1px top",         0, 1, 0, 0),
            ("1px L+R",         1, 0, 1, 0),
            ("1px T+B",         0, 1, 0, 1),
            ("2px right",       0, 0, 2, 0),
            ("4px right",       0, 0, 4, 0),
            ("8px right",       0, 0, 8, 0),
            ("16px right",      0, 0, 16, 0),
            ("32px right",      0, 0, 32, 0),
            ("64px right",      0, 0, 64, 0),
            ("128px right",     0, 0, 128, 0),
            ("8px corner T+L",  8, 8, 0, 0),
            ("16px all",        16, 16, 16, 16),
            ("2% all (50px)",   50, 50, 50, 50),
            ("5% all (125px)",  125, 125, 125, 125),
            ("10% all (250px)", 250, 250, 250, 250),
        ];

        let mut rows = String::new();
        for &(label, l, t, r, b) in cases {
            let (x0, y0) = (l, t);
            let (cw, ch) = (ow - l - r, oh - t - b);
            let cropped_rgb = crop_rgb(&pixels_wm, ow, x0, y0, cw, ch);
            let cropped_y   = extract_y_rgb(&cropped_rgb);
            let fill = cropped_y.iter().sum::<f32>() / cropped_y.len() as f32;

            // A — resample the cropped sub-rect back to original dimensions.
            let (pa, sa) = decode_y_at_size_verbose(&cropped_y, cw, ch, ow, oh);
            // B — pad at the true offset, matched decode (no-op resample).
            let yb = pad_y(&cropped_y, cw, ch, ow, oh, x0, y0, fill);
            let (pb, sb) = decode_y_at_size_verbose(&yb, ow, oh, ow, oh);
            // C — pad at offset 0.
            let yc = pad_y(&cropped_y, cw, ch, ow, oh, 0, 0, fill);
            let (pc, sc) = decode_y_at_size_verbose(&yc, ow, oh, ow, oh);

            rows.push_str(&format!(
                "| {:<15} | {:>11} | {:>9} | {:>3} / {:>3.0} | {:>3} / {:>3.0} | {:>3} / {:>3.0} |\n",
                label,
                format!("{},{},{},{}", l, t, r, b),
                format!("{}×{}", cw, ch),
                crop_errs(&pa), sa,
                crop_errs(&pb), sb,
                crop_errs(&pc), sc,
            ));
            println!("{:<15} A={:>3}/{:>3.0}  B={:>3}/{:>3.0}  C={:>3}/{:>3.0}",
                label, crop_errs(&pa), sa, crop_errs(&pb), sb, crop_errs(&pc), sc);
        }

        let report = format!(
"# Crop tolerance — characterization

Source: `tests/test_a.jpg` ({ow}×{oh}).  Wavelet: CDF 5/3.  ALPHA={alpha}, levels {levels:?}.
Payload: `DEADBEEF CAFEBABE 01234567 89ABCDEF` (test pattern, so `version` reads invalid — \
expected).  Errors are out of {bits}; detection floor ≈ {floor:.0}, strong ≈ {strong:.0}, \
off-grid noise ≈ 7.

Regenerate: `cargo test -p glimr --release crop_tolerance -- --ignored --nocapture`

**Strategies** (cell = `errors / score`):
- **A — resample to original** (current `decode_y_at_size`): stretches the cropped sub-rect
  back to {ow}×{oh}.  This is what the shipped decoder does today.
- **B — pad at known offset** (oracle): drops the cropped pixels at their original (x0,y0)
  in a full {ow}×{oh} frame.  If B decodes, the watermark **survived** the crop and only
  registration (recovering the offset) is missing.
- **C — pad at offset 0**: same padding but ignores the origin shift.  B-vs-C isolates the
  translation: right/bottom crops don't shift the origin, left/top do.

| case            | removed L,T,R,B | retained  |  A resample | B pad@offset |   C pad@0 |
|-----------------|-----------------|-----------|-------------|--------------|-----------|
{rows}
_Lower errors / higher score = better. 0 errors = exact recovery._
",
            ow = ow, oh = oh, alpha = ALPHA, levels = EMBED_LEVELS, bits = PAYLOAD_BITS,
            floor = detection_floor(), strong = detection_strong(), rows = rows,
        );

        let path = reports_dir().join("crop_tolerance.md");
        std::fs::write(&path, report).unwrap();
        println!("crop tolerance report → {}", path.display());
    }

    // ── Stage 1: registration feasibility — autocorrelation of a small excerpt ──
    //
    // Question: in a 512×512 excerpt, is the watermark's periodic lattice visible
    // enough to read its period (→ scale)?  The level-2 tile period is 256 px at
    // embed scale, so at scale s a B-px block holds B/(256·s) periods (need ≥2).
    //
    //   cargo test -p glimr --release registration_stage1 -- --ignored --nocapture
    //
    // Compares the blind case (whitened watermarked block) against an ORACLE
    // (autocorrelation of the pure watermark delta, content removed) to separate
    // "is the periodicity present" from "is it visible through image content".
    // Also probes whether perceptual masking smears the lattice (mask 0 vs 0.5).

    use rustfft::{FftPlanner, num_complex::Complex};

    fn hann(i: usize, n: usize) -> f32 {
        let x = std::f32::consts::PI * i as f32 / (n as f32 - 1.0);
        x.sin() * x.sin()
    }

    fn fft_2d(buf: &mut [Complex<f32>], n: usize, planner: &mut FftPlanner<f32>, inverse: bool) {
        let fft = if inverse { planner.plan_fft_inverse(n) } else { planner.plan_fft_forward(n) };
        for r in 0..n { fft.process(&mut buf[r * n..r * n + n]); }
        let mut col = vec![Complex::new(0.0, 0.0); n];
        for c in 0..n {
            for r in 0..n { col[r] = buf[r * n + c]; }
            fft.process(&mut col);
            for r in 0..n { buf[r * n + c] = col[r]; }
        }
    }

    /// Linear 2-D autocorrelation of a b×b block: Hann-windowed, mean-removed,
    /// zero-padded to 2b (so the result is linear, not circular), zero-lag shifted
    /// to the centre (b,b).  Returns the (2b×2b) real autocorrelation.
    fn autocorr_2d(block: &[f32], b: usize, planner: &mut FftPlanner<f32>) -> Vec<f32> {
        let n = 2 * b;
        let mean = block.iter().sum::<f32>() / (b * b) as f32;
        let mut buf = vec![Complex::new(0.0f32, 0.0); n * n];
        for y in 0..b {
            let wy = hann(y, b);
            for x in 0..b {
                buf[y * n + x] = Complex::new((block[y * b + x] - mean) * wy * hann(x, b), 0.0);
            }
        }
        fft_2d(&mut buf, n, planner, false);
        for z in buf.iter_mut() { *z = Complex::new(z.norm_sqr(), 0.0); }
        fft_2d(&mut buf, n, planner, true);
        let norm = (n * n) as f32;
        let mut out = vec![0.0f32; n * n];
        for y in 0..n {
            for x in 0..n {
                out[((y + b) % n) * n + (x + b) % n] = buf[y * n + x].re / norm;
            }
        }
        out
    }

    fn extract_block(src: &[f32], w: usize, x0: usize, y0: usize, b: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; b * b];
        for ry in 0..b {
            let s = (y0 + ry) * w + x0;
            out[ry * b..ry * b + b].copy_from_slice(&src[s..s + b]);
        }
        out
    }

    // Separable box blur (edge-clamped); used for a high-pass content suppressor.
    fn box_blur(src: &[f32], b: usize, radius: usize) -> Vec<f32> {
        let k = (2 * radius + 1) as f32;
        let mut tmp = vec![0.0f32; b * b];
        for y in 0..b {
            for x in 0..b {
                let mut s = 0.0;
                for d in 0..=2 * radius {
                    let xx = (x as isize + d as isize - radius as isize).clamp(0, b as isize - 1) as usize;
                    s += src[y * b + xx];
                }
                tmp[y * b + x] = s / k;
            }
        }
        let mut out = vec![0.0f32; b * b];
        for x in 0..b {
            for y in 0..b {
                let mut s = 0.0;
                for d in 0..=2 * radius {
                    let yy = (y as isize + d as isize - radius as isize).clamp(0, b as isize - 1) as usize;
                    s += tmp[yy * b + x];
                }
                out[y * b + x] = s / k;
            }
        }
        out
    }

    fn highpass(block: &[f32], b: usize, radius: usize) -> Vec<f32> {
        let lp = box_blur(block, b, radius);
        block.iter().zip(lp.iter()).map(|(a, l)| a - l).collect()
    }

    /// Spectral-whitened autocorrelation: divide the power spectrum by a locally
    /// smoothed copy of itself, flattening the broad image-content envelope so the
    /// watermark's periodic spectral peaks (and thus the lag-`period` peak) stand
    /// out — at whatever frequency they sit, so it's scale-agnostic.
    fn autocorr_2d_whitened(block: &[f32], b: usize, planner: &mut FftPlanner<f32>) -> Vec<f32> {
        let n = 2 * b;
        let mean = block.iter().sum::<f32>() / (b * b) as f32;
        let mut buf = vec![Complex::new(0.0f32, 0.0); n * n];
        for y in 0..b {
            let wy = hann(y, b);
            for x in 0..b {
                buf[y * n + x] = Complex::new((block[y * b + x] - mean) * wy * hann(x, b), 0.0);
            }
        }
        fft_2d(&mut buf, n, planner, false);
        let power: Vec<f32> = buf.iter().map(|z| z.norm_sqr()).collect();
        let env = box_blur(&power, n, 6); // content power envelope
        for (z, (&p, &e)) in buf.iter_mut().zip(power.iter().zip(env.iter())) {
            *z = Complex::new(p / (e + 1e-3), 0.0);
        }
        fft_2d(&mut buf, n, planner, true);
        let norm = (n * n) as f32;
        let mut out = vec![0.0f32; n * n];
        for y in 0..n {
            for x in 0..n {
                out[((y + b) % n) * n + (x + b) % n] = buf[y * n + x].re / norm;
            }
        }
        out
    }

    /// Wavelet band-pass: keep the mid detail levels (where the watermark lives),
    /// zero the LL approximation and the finest level-1 detail, inverse-transform.
    /// Content suppressor matched to the embedding band, broad enough for the scale
    /// range we care about.
    fn dwt_bandpass(block: &[f32], b: usize) -> Vec<f32> {
        let mut c = block.to_vec();
        dwt_2d_fwd(&mut c, b, b, DECOMP_DEPTH);
        for &band in &[Subband::LH, Subband::HL, Subband::HH] {
            let (r0, r1, c0, c1) = subband_bounds(b, b, 1, band); // drop finest detail
            for r in r0..r1 { for cc in c0..c1 { c[r * b + cc] = 0.0; } }
        }
        let (r0, r1, c0, c1) = subband_bounds(b, b, DECOMP_DEPTH, Subband::LL); // drop coarse content
        for r in r0..r1 { for cc in c0..c1 { c[r * b + cc] = 0.0; } }
        dwt_2d_inv(&mut c, b, b, DECOMP_DEPTH);
        c
    }

    /// Find the autocorrelation peak along the +x and +y axes near the expected
    /// period, returning (detected_period_px, prominence) where prominence is the
    /// peak over the mean |autocorr| of the off-peak band along those axes.
    fn lattice_peak(ac: &[f32], n: usize, b: usize, period: f32) -> (f32, f32) {
        let c = b; // centre (zero-lag) index
        let lo = (period * 0.75).round() as usize;
        let hi = ((period * 1.25).round() as usize).min(b - 2);
        let mut px = (0usize, f32::MIN);
        let mut py = (0usize, f32::MIN);
        for lag in lo..=hi {
            let vx = ac[c * n + (c + lag)];
            if vx > px.1 { px = (lag, vx); }
            let vy = ac[(c + lag) * n + c];
            if vy > py.1 { py = (lag, vy); }
        }
        // Background: mean |ac| over lags [20, b) along both axes.
        let mut bg = 0.0f32;
        let mut m = 0usize;
        for lag in 20..b {
            bg += ac[c * n + (c + lag)].abs() + ac[(c + lag) * n + c].abs();
            m += 2;
        }
        let bg = (bg / m as f32).max(1e-6);
        let period_px = (px.0 + py.0) as f32 / 2.0;
        let prominence = ((px.1 + py.1) / 2.0) / bg;
        (period_px, prominence)
    }

    fn save_autocorr_png(ac: &[f32], n: usize, b: usize, path: &std::path::Path) {
        // Zero a small central disk (the giant zero-lag lobe) so the lattice shows,
        // then normalise to 0..255.
        let mut v = ac.to_vec();
        let c = b as isize;
        for y in 0..n as isize {
            for x in 0..n as isize {
                if (x - c).pow(2) + (y - c).pow(2) < 12 * 12 { v[(y * n as isize + x) as usize] = 0.0; }
            }
        }
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for &x in &v { lo = lo.min(x); hi = hi.max(x); }
        let span = (hi - lo).max(1e-6);
        let px: Vec<u8> = v.iter().map(|x| (((x - lo) / span) * 255.0) as u8).collect();
        image::save_buffer(path, &px, n as u32, n as u32, image::ColorType::L8).ok();
    }

    /// One measurement: embed (at `mask`), rescale to `s`, take a `b`-block at
    /// (x0,y0); return (detected_period, blind_prominence, oracle_prominence).
    /// `blind_y`/`delta` are the full-frame scaled watermarked Y and watermark delta.
    // Returns (detected_period_from_spectral, prom_spectral, prom_dwt, prom_oracle).
    fn autocorr_case(
        blind_y: &[f32], delta: &[f32], sw: usize, x0: usize, y0: usize, b: usize,
        period: f32, planner: &mut FftPlanner<f32>, png: Option<&std::path::Path>,
    ) -> (f32, f32, f32, f32) {
        let n = 2 * b;
        let blk = extract_block(blind_y, sw, x0, y0, b);

        let ac_sp = autocorr_2d_whitened(&blk, b, planner);
        let (per_sp, prom_sp) = lattice_peak(&ac_sp, n, b, period);

        let ac_dw = autocorr_2d(&dwt_bandpass(&blk, b), b, planner);
        let (_pd, prom_dw) = lattice_peak(&ac_dw, n, b, period);

        let dblk = extract_block(delta, sw, x0, y0, b);
        let ac_or = autocorr_2d(&dblk, b, planner);
        let (_po, prom_o) = lattice_peak(&ac_or, n, b, period);

        if let Some(p) = png { save_autocorr_png(&ac_sp, n, b, p); }
        (per_sp, prom_sp, prom_dw, prom_o)
    }

    #[test]
    #[ignore]
    fn registration_stage1() {
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join("tests");
        let reports = reports_dir();
        let img = image::open(tests_dir.join("test_a.jpg")).unwrap().into_rgb8();
        let (ow, oh) = (img.width() as usize, img.height() as usize);
        let pixels = img.into_raw();
        let orig_y = extract_y_rgb(&pixels);
        let mut planner = FftPlanner::<f32>::new();
        const TILE_PERIOD: f32 = (TILE_SIDE * 4) as f32; // level-2 spatial period @ embed scale

        // Embed once per mask strength; keep wm_y and delta = wm_y - orig_y.
        let embed_at = |mask: f32| -> (Vec<f32>, Vec<f32>) {
            let mut wm = orig_y.clone();
            embed_y_masked(&mut wm, ow, oh, &PHASE3_PAYLOAD, mask);
            let delta: Vec<f32> = wm.iter().zip(orig_y.iter()).map(|(a, b)| a - b).collect();
            (wm, delta)
        };
        let (wm_main, delta_main) = embed_at(MASK_STRENGTH);

        // Rescale a full-frame Y by s (and return scaled dims).
        let scale = |buf: &[f32], s: f32| -> (Vec<f32>, usize, usize) {
            let nw = (ow as f32 * s).round() as usize;
            let nh = (oh as f32 * s).round() as usize;
            (resample_y(buf, ow, oh, nw, nh), nw, nh)
        };

        // ── Slice 1: block size × scale (centre block, masked embed) ──────────
        let mut s1 = String::new();
        for &b in &[256usize, 512, 1024] {
            for &s in &[1.0f32, 0.7, 0.5, 0.33, 0.25] {
                let (wm_s, sw, sh) = scale(&wm_main, s);
                let (dl_s, _, _)   = scale(&delta_main, s);
                if b + 2 > sw.min(sh) { // need room for the block (+slack)
                    s1.push_str(&format!("| {:>4} | {:>4.2} | n/a (block > image) | | | | | |\n", b, s));
                    continue;
                }
                let (x0, y0) = ((sw - b) / 2, (sh - b) / 2);
                let period = TILE_PERIOD * s;
                let png = if b == 512 && (s == 1.0 || s == 0.5 || s == 0.25) {
                    Some(reports.join(format!("autocorr_b512_s{:02.0}.png", s * 100.0)))
                } else { None };
                let (per, prom_sp, prom_dw, prom_o) =
                    autocorr_case(&wm_s, &dl_s, sw, x0, y0, b, period, &mut planner, png.as_deref());
                let serr = (per / period - 1.0) * 100.0;
                s1.push_str(&format!(
                    "| {:>4} | {:>4.2} | {:>6.1} | {:>6.1} | {:>+5.1}% | {:>6.1} | {:>6.1} | {:>6.1} |\n",
                    b, s, period, per, serr, prom_sp, prom_dw, prom_o));
                println!("b={b} s={s:.2} per~{per:.0}/{period:.0} err{serr:+.1}% spectral={prom_sp:.1} dwt={prom_dw:.1} oracle={prom_o:.1}");
            }
        }

        // ── Slice 2: masking strength × block content, at 512 / 0.5× ──────────
        let b = 512usize;
        let (wm_h, sw, sh) = scale(&wm_main, 0.5);
        // Find busy / smooth 512-blocks by local variance over a grid.
        let mut busy = (0usize, 0usize, f32::MIN);
        let mut smooth = (0usize, 0usize, f32::MAX);
        let step = (sw.min(sh) - b) / 3;
        for gy in 0..4 { for gx in 0..4 {
            let (x0, y0) = (gx * step, gy * step);
            if x0 + b > sw || y0 + b > sh { continue; }
            let blk = extract_block(&wm_h, sw, x0, y0, b);
            let m = blk.iter().sum::<f32>() / blk.len() as f32;
            let var = blk.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / blk.len() as f32;
            if var > busy.2 { busy = (x0, y0, var); }
            if var < smooth.2 { smooth = (x0, y0, var); }
        }}
        let mut s2 = String::new();
        for &mask in &[0.0f32, 0.5] {
            let (wm_m, dl_m) = if mask == MASK_STRENGTH { (wm_main.clone(), delta_main.clone()) } else { embed_at(mask) };
            let (wm_ms, mw, _) = scale(&wm_m, 0.5);
            let (dl_ms, _, _)  = scale(&dl_m, 0.5);
            for &(label, (x0, y0)) in &[("busy", (busy.0, busy.1)), ("smooth", (smooth.0, smooth.1))] {
                let (_per, prom_sp, prom_dw, prom_o) =
                    autocorr_case(&wm_ms, &dl_ms, mw, x0, y0, b, TILE_PERIOD * 0.5, &mut planner, None);
                s2.push_str(&format!("| {:>4.1} | {:<6} | {:>6.1} | {:>6.1} | {:>6.1} |\n",
                    mask, label, prom_sp, prom_dw, prom_o));
                println!("mask={mask:.1} {label} spectral={prom_sp:.1} dwt={prom_dw:.1} oracle={prom_o:.1}");
            }
        }
        let _ = (sh,);

        // ── Slice 3: whitening method comparison at 512, scales 1.0× and 0.5× ──
        let scales3 = [1.0f32, 0.5];
        let mut cols: Vec<[f32; 5]> = Vec::new(); // [raw, highpass, spectral, dwt, oracle] per scale
        for &s in &scales3 {
            let (wm_s, sw3, sh3) = scale(&wm_main, s);
            let (dl_s, _, _) = scale(&delta_main, s);
            let (x0, y0) = ((sw3 - b) / 2, (sh3 - b) / 2);
            let blk = extract_block(&wm_s, sw3, x0, y0, b);
            let per = TILE_PERIOD * s;
            let prom = |ac: &[f32], pl: &mut FftPlanner<f32>| { let _ = pl; lattice_peak(ac, 2 * b, b, per).1 };
            let raw = prom(&autocorr_2d(&blk, b, &mut planner), &mut planner);
            let hp  = prom(&autocorr_2d(&highpass(&blk, b, 16), b, &mut planner), &mut planner);
            let sp  = prom(&autocorr_2d_whitened(&blk, b, &mut planner), &mut planner);
            let dw  = prom(&autocorr_2d(&dwt_bandpass(&blk, b), b, &mut planner), &mut planner);
            let orc = prom(&autocorr_2d(&extract_block(&dl_s, sw3, x0, y0, b), b, &mut planner), &mut planner);
            cols.push([raw, hp, sp, dw, orc]);
        }
        let names = ["raw", "high-pass", "spectral", "dwt-band", "oracle Δ"];
        let mut s3 = String::new();
        for (i, nm) in names.iter().enumerate() {
            s3.push_str(&format!("| {:<10} | {:>7.1} | {:>7.1} |\n", nm, cols[0][i], cols[1][i]));
            println!("whitening {nm}: 1.0×={:.1}  0.5×={:.1}", cols[0][i], cols[1][i]);
        }

        let report = format!(
"# Registration Stage 1 — does a 512×512 excerpt reveal the watermark period?

Source: `tests/test_a.jpg` ({ow}×{oh}).  CDF 5/3, ALPHA={alpha}, levels {levels:?}.
Level-2 tile period = {tp:.0} px at embed scale; in a B-px block at scale s there are
B/(period·s) periods (≥2 needed).  **prominence** = autocorrelation peak ÷ off-peak band
(≫1 = clear lattice; ~1 = invisible).  **oracle** = autocorrelation of the pure watermark
delta (content removed) = the ceiling.  Blind methods: **spectral** = spectral-whitened
autocorrelation (scale-agnostic), **dwt-band** = wavelet band-pass then autocorrelation.

Regenerate: `cargo test -p glimr --release registration_stage1 -- --ignored --nocapture`

## Slice 1 — block size × scale (centre block, masked embed @ {alpha_mask})

| block | scale | period px | det px | scale err | spectral | dwt-band | oracle |
|-------|-------|-----------|--------|-----------|----------|----------|--------|
{s1}
## Slice 2 — masking strength × block content (512 block, 0.5× scale)

| mask | block  | spectral | dwt-band | oracle |
|------|--------|----------|----------|--------|
{s2}
## Slice 3 — whitening method × scale (512 block, centre) — prominence

| method     |   1.0× |   0.5× |
|------------|--------|--------|
{s3}
Heatmaps (spectral-whitened autocorr): `autocorr_b512_s100.png` (1.0×), `_s50.png`
(0.5×), `_s25.png` (0.25×) — the lattice should sharpen as scale drops.
",
            ow = ow, oh = oh, alpha = ALPHA, levels = EMBED_LEVELS, tp = TILE_PERIOD,
            alpha_mask = MASK_STRENGTH, s1 = s1, s2 = s2, s3 = s3,
        );
        let path = reports.join("registration_stage1.md");
        std::fs::write(&path, report).unwrap();
        println!("registration stage 1 report → {}", path.display());
    }

    // ── Stage 2: blind registration + decode ──────────────────────────────────
    //
    // End-to-end blind pipeline (no size hint): recover scale (autocorrelation) →
    // band-pass to the embed band → fold to one tile → keyed cross-correlation
    // against per-bit spatial templates → the offset is the score peak and the
    // per-bit correlation SIGNS are the payload.  Target: match the crop table's
    // B-oracle (0 errors with the offset known).  Level-2 only (period 256) for v1.
    //
    //   cargo test -p glimr --release registration_stage2 -- --ignored --nocapture

    const FOLD: usize = TILE_SIDE * 4; // level-2 spatial tile period = 256

    /// Spatial footprint of one payload bit: pn_b tiled into LH2/HL2, inverse-DWT,
    /// one FOLD×FOLD tile extracted from a clean interior region.
    fn bit_template(bit: usize, planner: &mut FftPlanner<f32>) -> Vec<f32> {
        let _ = planner;
        let frame = 768usize;
        let tile = pn_tile(bit);
        let mut c = vec![0.0f32; frame * frame];
        for &band in &[Subband::LH, Subband::HL] {
            let (r0, r1, c0, c1) = subband_bounds(frame, frame, 2, band);
            for r in r0..r1 {
                for cc in c0..c1 {
                    let ti = ((r - r0) % TILE_SIDE) * TILE_SIDE + (cc - c0) % TILE_SIDE;
                    c[r * frame + cc] = tile[ti];
                }
            }
        }
        dwt_2d_inv(&mut c, frame, frame, DECOMP_DEPTH);
        extract_block(&c, frame, FOLD, FOLD, FOLD) // interior tile at (256,256)
    }

    /// Band-pass an image to the level-2 detail band (keep LH2/HL2, zero the rest).
    fn keep_level2(img: &[f32], w: usize, h: usize) -> Vec<f32> {
        let mut c = img.to_vec();
        dwt_2d_fwd(&mut c, w, h, DECOMP_DEPTH);
        let keep_lh = subband_bounds(w, h, 2, Subband::LH);
        let keep_hl = subband_bounds(w, h, 2, Subband::HL);
        let mut out = vec![0.0f32; w * h];
        for &(r0, r1, c0, c1) in &[keep_lh, keep_hl] {
            for r in r0..r1 { for cc in c0..c1 { out[r * w + cc] = c[r * w + cc]; } }
        }
        dwt_2d_inv(&mut out, w, h, DECOMP_DEPTH);
        out
    }

    /// Fold an image into one FOLD×FOLD tile by summing all period-FOLD shifts.
    /// Content averages down (incoherent); the periodic watermark adds coherently.
    fn fold_tile(img: &[f32], w: usize, h: usize) -> Vec<f32> {
        let mut f = vec![0.0f32; FOLD * FOLD];
        let (tw, th) = ((w / FOLD) * FOLD, (h / FOLD) * FOLD); // whole tiles only
        for y in 0..th {
            for x in 0..tw {
                f[(y % FOLD) * FOLD + (x % FOLD)] += img[y * w + x];
            }
        }
        f
    }

    /// Cyclic cross-correlation map (FOLD×FOLD): c[φ] = Σ_x a(x)·b(x−φ), via FFT.
    fn xcorr_cyclic(a: &[f32], b: &[f32], planner: &mut FftPlanner<f32>) -> Vec<f32> {
        let n = FOLD;
        let mut fa: Vec<Complex<f32>> = a.iter().map(|&v| Complex::new(v, 0.0)).collect();
        let mut fb: Vec<Complex<f32>> = b.iter().map(|&v| Complex::new(v, 0.0)).collect();
        fft_2d(&mut fa, n, planner, false);
        fft_2d(&mut fb, n, planner, false);
        for (x, y) in fa.iter_mut().zip(fb.iter()) { *x *= y.conj(); }
        fft_2d(&mut fa, n, planner, true);
        let norm = (n * n) as f32;
        fa.iter().map(|z| z.re / norm).collect()
    }

    /// Blind scale estimate from a block: spectral-whitened autocorrelation, take
    /// the global peak over the plausible lag band, then fold down to the
    /// fundamental (prefer L/2 while a comparable peak sits there).  Returns
    /// estimated scale s = period_observed / FOLD, clamped to a sane range.
    fn blind_scale(block: &[f32], b: usize, planner: &mut FftPlanner<f32>) -> f32 {
        let ac = autocorr_2d_whitened(block, b, planner);
        let n = 2 * b;
        let c = b;
        let (min_lag, max_lag) = (24usize, b - 2);
        let prof: Vec<f32> = (0..max_lag)
            .map(|lag| if lag < min_lag { f32::MIN } else { ac[c * n + c + lag].max(ac[(c + lag) * n + c]) })
            .collect();
        let mut peak = (min_lag, prof[min_lag]);
        for lag in min_lag..max_lag { if prof[lag] > peak.1 { peak = (lag, prof[lag]); } }
        let mut period = peak.0;
        loop {
            let cand = period / 2;
            if cand < min_lag { break; }
            let mut local = f32::MIN;
            for l in (cand - 2)..=(cand + 2) { local = local.max(prof[l]); }
            if local > 0.5 * peak.1 { period = cand; } else { break; }
        }
        (period as f32 / FOLD as f32).clamp(0.1, 3.0)
    }

    /// Blind decode at a chosen rescale target: rescale suspect → level-2 band-pass
    /// → fold → keyed cross-correlation; returns (bit_errors, phase_prominence,
    /// offset_x, offset_y).
    fn decode_blind(
        suspect: &[f32], nw: usize, nh: usize, tw: usize, th: usize,
        templates: &[Vec<f32>], planner: &mut FftPlanner<f32>,
    ) -> (usize, f32, usize, usize) {
        let rescaled = resample_y(suspect, nw, nh, tw, th);
        let band = keep_level2(&rescaled, tw, th);
        let folded = fold_tile(&band, tw, th);
        let mut score = vec![0.0f32; FOLD * FOLD];
        let mut maps: Vec<Vec<f32>> = Vec::with_capacity(templates.len());
        for t in templates {
            let c = xcorr_cyclic(&folded, t, planner);
            for (s_, v) in score.iter_mut().zip(c.iter()) { *s_ += v.abs(); }
            maps.push(c);
        }
        let (mut bp, mut bv) = (0usize, f32::MIN);
        for (i, &v) in score.iter().enumerate() { if v > bv { bv = v; bp = i; } }
        let smed = { let mut s = score.clone(); s.sort_by(|a, b| a.partial_cmp(b).unwrap()); s[s.len()/2].max(1e-6) };
        let mut bits = [false; PAYLOAD_BITS];
        for (b, m) in maps.iter().enumerate() { bits[b] = m[bp] > 0.0; }
        (crop_errs(&bits_to_payload(&bits)), bv / smed, bp % FOLD, bp / FOLD)
    }

    #[test]
    #[ignore]
    fn registration_stage2() {
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join("tests");
        let reports = reports_dir();
        let img = image::open(tests_dir.join("test_a.jpg")).unwrap().into_rgb8();
        let (ow, oh) = (img.width() as usize, img.height() as usize);
        let pixels = img.into_raw();
        let orig_y = extract_y_rgb(&pixels);
        let mut planner = FftPlanner::<f32>::new();

        // Embed once (masked), full frame.
        let mut wm = orig_y.clone();
        embed_y_masked(&mut wm, ow, oh, &PHASE3_PAYLOAD, MASK_STRENGTH);

        // Precompute per-bit templates and their FFTs.
        let templates: Vec<Vec<f32>> = (0..PAYLOAD_BITS).map(|b| bit_template(b, &mut planner)).collect();

        // Resample helper.
        let scale_to = |buf: &[f32], w: usize, h: usize, nw: usize, nh: usize| resample_y(buf, w, h, nw, nh);

        let offsets = [("none", 0usize, 0usize), ("(37,53)", 37, 53), ("(130,200)", 130, 200), ("10% (250,250)", 250, 250)];
        let scales = [1.0f32, 0.7, 0.5, 0.33];

        let mut rows = String::new();
        for &s in &scales {
            for &(olabel, ox, oy) in &offsets {
                // Build suspect: crop (origin shift ox,oy) then rescale by s.
                let (cw, ch) = (ow - ox, oh - oy);
                let cropped = crop_rgb_y(&wm, ow, ox, oy, cw, ch);
                let (nw, nh) = ((cw as f32 * s).round() as usize, (ch as f32 * s).round() as usize);
                let suspect = scale_to(&cropped, cw, ch, nw, nh);

                // Blind scale from a centre block.
                let sb = 512.min(nw).min(nh);
                let blk = extract_block(&suspect, nw, (nw - sb) / 2, (nh - sb) / 2, sb);
                let s_est = blind_scale(&blk, sb, &mut planner);
                let s_err = (s_est / s - 1.0) * 100.0;

                // Blind decode: rescale by the *estimated* inverse scale.
                let inv = 1.0 / s_est;
                let (tw_b, th_b) = ((nw as f32 * inv).round() as usize, (nh as f32 * inv).round() as usize);
                let (eb, _pb, _, _) = decode_blind(&suspect, nw, nh, tw_b, th_b, &templates, &mut planner);

                // Known-scale decode (oracle on scale only): rescale back to the
                // cropped embed-scale dims (cw,ch).  Isolates the phase pipeline.
                let (ek, pk, phx, phy) = decode_blind(&suspect, nw, nh, cw, ch, &templates, &mut planner);

                rows.push_str(&format!(
                    "| {:>4.2} | {:<13} | {:>+6.1}% | {:>3}/128 | {:>3}/128 | {:>5.0} | ({:>3},{:>3}) |\n",
                    s, olabel, s_err, eb, ek, pk, phx, phy));
                println!("s={s:.2} off={olabel} s_err={s_err:+.1}% errs_blind={eb} errs_known={ek} prom={pk:.0} phi=({phx},{phy})");
            }
        }

        let report = format!(
"# Registration Stage 2 — blind scale + offset recovery + decode

Source: `tests/test_a.jpg` ({ow}×{oh}).  CDF 5/3, ALPHA={alpha}, levels {levels:?}, mask {mask}.
Fully blind: recover scale (autocorrelation) → rescale → level-2 band-pass → fold to a
{fold}-tile → keyed cross-correlation vs per-bit spatial templates.  The score peak is the
crop offset (mod {fold}); the per-bit correlation signs are the payload.  Target = the crop
table's B-oracle (0/128).  v1 uses level-2 only.

Regenerate: `cargo test -p glimr --release registration_stage2 -- --ignored --nocapture`

`errs blind` uses the blind scale estimate; `errs known` forces the true scale (rescale
back to the cropped dims) to isolate the phase/decode pipeline from scale estimation.

| scale | crop offset   | scale err | errs blind | errs known | phase prom | offset φ |
|-------|---------------|-----------|------------|------------|------------|----------|
{rows}
_offset φ is the recovered (x,y) mod {fold} at known scale; phase prom = peak/median._
",
            ow = ow, oh = oh, alpha = ALPHA, levels = EMBED_LEVELS, mask = MASK_STRENGTH,
            fold = FOLD, rows = rows,
        );
        let path = reports.join("registration_stage2.md");
        std::fs::write(&path, report).unwrap();
        println!("registration stage 2 report → {}", path.display());
    }

    // Crop a row-major RGB-derived Y? No — wm is Y (f32). Crop a Y sub-rectangle.
    fn crop_rgb_y(src: &[f32], w: usize, x0: usize, y0: usize, cw: usize, ch: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; cw * ch];
        for ry in 0..ch {
            let s = (y0 + ry) * w + x0;
            out[ry * cw..ry * cw + cw].copy_from_slice(&src[s..s + cw]);
        }
        out
    }
}
