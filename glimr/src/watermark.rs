// Spread-spectrum DWT watermarking.
// Phase 1: 2D DWT (CDF 5/3 lifting) with round-trip tests.
// Phase 2: PN generation, embedding, blind correlation decode.

// ── Constants ────────────────────────────────────────────────────────────────

pub const WM_KEY:       u64    = 0xDEAD_BEEF_C0FF_EE42u64;
pub const ALPHA:        f32    = 0.15;
pub const EMBED_LEVELS: &[u32] = &[2, 3];
pub const TILE_SIDE:    usize  = 64;   // PN grid: each subband normalized to TILE_SIDE×TILE_SIDE
pub const DECOMP_DEPTH:  u32   = 4;

// Payload layout (192 embedded bits = 24 bytes):
//   bytes  0..16  data       (the 128-bit message JS assembles)
//   bytes 16..20  CRC-32 of the data (LE)  — verification
//   bytes 20..24  BCH(192,160) t=4 parity over data+CRC  — error correction
// `embed_y` takes the 16 data bytes; the CRC and ECC parity are computed in WASM
// (one Rust place, shared with the decoder), so the JS boundary stays 16 bytes.
pub const PAYLOAD_BITS:  usize = 192;  // total embedded bits
pub const DATA_BYTES:    usize = 16;   // message bytes (128 data bits)
pub const FULL_BYTES:    usize = 24;   // data + CRC + reserved-ECC
pub const RESIDUAL_AMP:  f32   = 20.0;

/// Channel-layer format **generation**.  The channel — envelope size, integrity
/// slot, ECC scheme — is frozen per generation; only the *semantic* meaning of the
/// data bytes evolves, dispatched by the payload `version` byte (handled by the
/// consumer, e.g. the CLI's field printer).  Decoding is version-independent:
/// correct + verify against a generation, *then* interpret the data.
///
/// There is one generation today (`GEN1`).  A future channel change — a larger
/// envelope, CRC→MAC, or a different ECC — would add a second `Generation` that
/// `decode_bits` tries in turn: the first whose integrity check verifies wins,
/// which keeps images from older generations decodable indefinitely.
#[derive(Clone, Copy, Debug)]
pub struct Generation {
    pub id:           u32,   // generation number (the channel format, not the payload version)
    pub payload_bits: usize, // total embedded bits (the envelope)
    pub data_bytes:   usize, // semantic message bytes
    pub crc_bytes:    usize, // integrity field width (bytes)
    pub ecc_t:        usize, // BCH error-correction capability (0 = none)
}

/// The current (and only) channel generation: 192-bit envelope = 128 data + 32
/// CRC-32 + 32 BCH(192,160) parity, correcting up to 4 bit errors.
pub const GEN1: Generation = Generation {
    id: 1, payload_bits: PAYLOAD_BITS, data_bytes: DATA_BYTES, crc_bytes: 4, ecc_t: bch::T,
};

/// Decoded result: the 16 data bytes, whether the embedded CRC matched, and how
/// many bit errors ECC fixed to get there (0 if clean, or if still unverified).
#[derive(Clone, Copy, Debug)]
pub struct Decoded {
    pub data:     [u8; DATA_BYTES],
    pub verified: bool, // CRC-32 over `data` matches the embedded checksum
    pub errors_corrected: u8, // BCH bit-errors corrected before the CRC verified
}

/// CRC-32 (IEEE 802.3, reflected poly 0xEDB88320), bitwise — tiny payload, no table.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Assemble the 24-byte embedded payload: data ++ CRC32(data) ++ BCH parity.
fn full_payload(data: &[u8; DATA_BYTES]) -> [u8; FULL_BYTES] {
    let mut full = [0u8; FULL_BYTES];
    full[..DATA_BYTES].copy_from_slice(data);
    full[DATA_BYTES..DATA_BYTES + 4].copy_from_slice(&crc32(data).to_le_bytes());

    // ECC: BCH(192,160) parity over the 160 info bits (data ++ CRC), into bytes 20..24.
    // The bit order matches `payload_to_bits` (LSB-first within each byte) so the
    // embedded codeword is laid out [info(160) | parity(32)] — what the bch codec expects.
    let mut info = [false; bch::INFO_BITS];
    for (i, b) in info.iter_mut().enumerate() { *b = (full[i / 8] >> (i % 8)) & 1 == 1; }
    let parity = bch::shared().encode_parity(&info);
    for (i, &p) in parity.iter().enumerate() {
        if p { full[DATA_BYTES + 4 + i / 8] |= 1 << (i % 8); }
    }
    full
}

/// Split a decoded 24-byte payload into data + CRC-verification.
fn split_payload(full: &[u8; FULL_BYTES]) -> Decoded {
    let mut data = [0u8; DATA_BYTES];
    data.copy_from_slice(&full[..DATA_BYTES]);
    let crc_embedded = u32::from_le_bytes([full[16], full[17], full[18], full[19]]);
    Decoded { data, verified: crc32(&data) == crc_embedded, errors_corrected: 0 }
}

// ── BCH ECC: shortened BCH(192,160), t=4 over GF(2^8) ─────────────────────────
//
// Generation-1 error correction for the embedded payload.  The 160 info bits
// (128 data + 32 CRC) are protected by 32 parity bits (the reserved ECC field),
// correcting up to 4 bit errors anywhere in the 192-bit codeword.  This module is
// the standalone codec (Phase 1): systematic `encode_parity` + in-place `correct`.
// It is *not* yet wired into embed/decode — that is Phase 2/3.
//
// Code: narrow-sense BCH, primitive poly 0x11D (x^8+x^4+x^3+x^2+1); generator
// g = lcm(m_1,m_3,m_5,m_7), degree 32 → full code BCH(255,223,t=4), used
// *shortened* to (192,160) (the top 63 info positions are implicit zeros).  A
// binary code, so located errors are simple bit flips — no Forney magnitude step.
//
// Bit layout of a codeword (matches the embedded payload): `[info(160) | parity(32)]`.
//   external index → polynomial degree:  info  idx j      ↔ x^(32+j)
//                                         parity idx 160+i ↔ x^i
pub mod bch {
    use std::sync::OnceLock;

    const GF_POLY: usize = 0x11D; // x^8 + x^4 + x^3 + x^2 + 1 (primitive)
    const N:       usize = 255;   // full code length (2^8 − 1)
    pub const T:           usize = 4;   // correctable errors
    pub const PARITY_BITS: usize = 32;
    pub const INFO_BITS:   usize = 160;
    pub const CODE_BITS:   usize = INFO_BITS + PARITY_BITS; // 192

    // The codeword exactly fills the embedded payload (data+CRC = info, ECC = parity).
    const _: () = assert!(CODE_BITS == super::PAYLOAD_BITS);

    /// BCH codec: GF(2^8) exp/log tables + the degree-32 generator's binary taps.
    pub struct Bch {
        exp:  [u8; 512], // exp[i] = α^i  (doubled so log+log can't overflow the index)
        log:  [u8; 256], // log[α^i] = i
        gbit: [bool; PARITY_BITS + 1], // generator coefficients (all 0/1), x^0..x^32
    }

    /// Shared, lazily-built codec — the tables and generator are fixed constants.
    pub fn shared() -> &'static Bch {
        static B: OnceLock<Bch> = OnceLock::new();
        B.get_or_init(Bch::new)
    }

    impl Default for Bch { fn default() -> Self { Bch::new() } }

    impl Bch {
        pub fn new() -> Self {
            // GF(2^8) tables (α = 2 generates the field under 0x11D).
            let mut exp = [0u8; 512];
            let mut log = [0u8; 256];
            let mut x = 1usize;
            for i in 0..N {
                exp[i] = x as u8;
                log[x] = i as u8;
                x <<= 1;
                if x & 0x100 != 0 { x ^= GF_POLY; }
            }
            for i in N..(2 * N) { exp[i] = exp[i - N]; }

            let mut me = Bch { exp, log, gbit: [false; PARITY_BITS + 1] };

            // Roots = union of the cyclotomic cosets of 1..=2t (→ cosets of 1,3,5,7).
            let mut is_root = [false; N];
            for i in 1..=(2 * T) {
                let mut j = i;
                loop { is_root[j] = true; j = (j * 2) % N; if j == i { break; } }
            }
            // g(x) = Π_{j : is_root[j]} (x + α^j) — the coefficients come out binary.
            let mut g = vec![1u8];
            for j in 0..N {
                if !is_root[j] { continue; }
                let r = me.exp[j];
                let mut ng = vec![0u8; g.len() + 1];
                for k in 0..g.len() {
                    ng[k + 1] ^= g[k];              // x·g
                    ng[k]     ^= me.gf_mul(g[k], r); // r·g
                }
                g = ng;
            }
            assert_eq!(g.len(), PARITY_BITS + 1, "BCH generator must have degree 32");
            for (k, &c) in g.iter().enumerate() {
                debug_assert!(c == 0 || c == 1, "non-binary generator coefficient");
                me.gbit[k] = c != 0;
            }
            me
        }

        #[inline]
        fn gf_mul(&self, a: u8, b: u8) -> u8 {
            if a == 0 || b == 0 { 0 }
            else { self.exp[self.log[a as usize] as usize + self.log[b as usize] as usize] }
        }
        #[inline]
        fn gf_inv(&self, a: u8) -> u8 { self.exp[(N - self.log[a as usize] as usize) % N] }

        /// Systematic parity for `info` (length `INFO_BITS`): the remainder of
        /// (info · x^32) mod g(x) over GF(2).  `info[j]` is the coefficient of
        /// x^(32+j); `parity[i]` is the coefficient of x^i.
        pub fn encode_parity(&self, info: &[bool]) -> [bool; PARITY_BITS] {
            debug_assert_eq!(info.len(), INFO_BITS);
            let mut d = [false; CODE_BITS]; // degree-indexed dividend
            for j in 0..INFO_BITS { d[PARITY_BITS + j] = info[j]; }
            for deg in (PARITY_BITS..CODE_BITS).rev() {
                if d[deg] {
                    for k in 0..=PARITY_BITS {
                        if self.gbit[k] { d[deg - PARITY_BITS + k] ^= true; }
                    }
                }
            }
            let mut parity = [false; PARITY_BITS];
            parity.copy_from_slice(&d[..PARITY_BITS]);
            parity
        }

        /// Correct up to `T` bit errors in a 192-bit codeword laid out as
        /// `[info(160) | parity(32)]`.  Returns the number of errors corrected
        /// (0 if already clean), or `None` if uncorrectable (> T errors, or a root
        /// located in the implicit shortening region).
        pub fn correct(&self, code: &mut [bool; CODE_BITS]) -> Option<usize> {
            let deg_of = |idx: usize| if idx < INFO_BITS { idx + PARITY_BITS } else { idx - INFO_BITS };

            // Syndromes S_j = c(α^j), j = 1..=2t.
            let mut synd = [0u8; 2 * T];
            let mut any = false;
            for j in 1..=(2 * T) {
                let mut acc = 0u8;
                for idx in 0..CODE_BITS {
                    if code[idx] { acc ^= self.exp[(deg_of(idx) * j) % N]; }
                }
                synd[j - 1] = acc;
                if acc != 0 { any = true; }
            }
            if !any { return Some(0); }

            // Error-locator σ(x) via Berlekamp–Massey.
            let sigma = self.berlekamp_massey(&synd);
            let l = sigma.len() - 1;
            if l == 0 || l > T { return None; }

            // Chien search: an error sits at degree e ⟺ σ(α^{-e}) = 0.
            let mut errs = Vec::with_capacity(l);
            for e in 0..N {
                let xinv = self.exp[(N - (e % N)) % N];
                let mut v = 0u8;
                let mut xp = 1u8;
                for &s in &sigma {
                    v ^= self.gf_mul(s, xp);
                    xp = self.gf_mul(xp, xinv);
                }
                if v == 0 { errs.push(e); }
            }
            if errs.len() != l { return None; }                       // couldn't locate all errors
            for &e in &errs { if e >= CODE_BITS { return None; } }    // root in the shortened pad

            for &e in &errs {
                let idx = if e < PARITY_BITS { INFO_BITS + e } else { e - PARITY_BITS };
                code[idx] ^= true;
            }
            Some(errs.len())
        }

        /// Shortest-LFSR synthesis of the error locator from the syndromes.
        /// Returns σ with σ[0] = 1; deg σ = number of errors.
        fn berlekamp_massey(&self, s: &[u8]) -> Vec<u8> {
            let mut sigma = vec![1u8];
            let mut b     = vec![1u8];
            let mut l     = 0usize;
            let mut m     = 1usize;
            let mut bb    = 1u8;
            for n in 0..s.len() {
                let mut d = s[n];
                for i in 1..=l {
                    if i <= n { if let Some(&si) = sigma.get(i) { d ^= self.gf_mul(si, s[n - i]); } }
                }
                if d == 0 {
                    m += 1;
                } else if 2 * l <= n {
                    let prev = sigma.clone();
                    let coef = self.gf_mul(d, self.gf_inv(bb));
                    self.poly_add_shift(&mut sigma, &b, coef, m);
                    l = n + 1 - l;
                    b = prev;
                    bb = d;
                    m = 1;
                } else {
                    let coef = self.gf_mul(d, self.gf_inv(bb));
                    self.poly_add_shift(&mut sigma, &b, coef, m);
                    m += 1;
                }
            }
            while sigma.len() > 1 && *sigma.last().unwrap() == 0 { sigma.pop(); }
            sigma
        }

        /// σ(x) += coef · x^m · b(x)  (GF(2^8); add = XOR).
        fn poly_add_shift(&self, sigma: &mut Vec<u8>, b: &[u8], coef: u8, m: usize) {
            if coef == 0 { return; }
            if sigma.len() < b.len() + m { sigma.resize(b.len() + m, 0); }
            for i in 0..b.len() { sigma[i + m] ^= self.gf_mul(coef, b[i]); }
        }
    }
}

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

fn payload_to_bits(payload: &[u8; FULL_BYTES]) -> [bool; PAYLOAD_BITS] {
    let mut bits = [false; PAYLOAD_BITS];
    for (b, &byte) in payload.iter().enumerate() {
        for k in 0..8 { bits[b * 8 + k] = (byte >> k) & 1 == 1; }
    }
    bits
}

fn bits_to_payload(bits: &[bool; PAYLOAD_BITS]) -> [u8; FULL_BYTES] {
    let mut out = [0u8; FULL_BYTES];
    for (b, byte) in out.iter_mut().enumerate() {
        for k in 0..8 { if bits[b * 8 + k] { *byte |= 1 << k; } }
    }
    out
}

/// Recover a `Decoded` from 192 raw payload bits, applying error correction.
///
/// **CRC-first → ECC-on-failure → CRC-recheck.**  A clean read (CRC already valid)
/// is returned immediately with zero corrections — so a good decode is never
/// disturbed by ECC.  Only on CRC failure is BCH run; if it corrects the codeword
/// *and* the CRC then verifies, the corrected data is returned with the error
/// count.  Otherwise the unverified raw decode is returned.  The CRC stays the
/// final oracle — ECC proposes, CRC disposes, so false-accepts stay ~2⁻³².
fn decode_bits(bits: &[bool; PAYLOAD_BITS]) -> Decoded {
    // Generation dispatch: one channel generation today (`GEN1`).  When a second
    // exists, try each here in turn and accept the first whose integrity verifies.
    let raw = split_payload(&bits_to_payload(bits));
    if raw.verified { return raw; }
    let mut code = *bits; // codeword layout is already [info(160) | parity(32)]
    if let Some(n) = bch::shared().correct(&mut code) {
        if n > 0 {
            let fixed = split_payload(&bits_to_payload(&code));
            if fixed.verified { return Decoded { errors_corrected: n as u8, ..fixed }; }
        }
    }
    raw
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
pub fn embed_y(y: &mut [f32], width: usize, height: usize, payload: &[u8; DATA_BYTES]) {
    embed_y_masked(y, width, height, payload, MASK_STRENGTH);
}

/// Like `embed_y` but with an explicit perceptual-masking blend strength
/// (0 = uniform, 1 = full). Exposed so experiments can compare masked vs uniform
/// embedding (e.g. its effect on the watermark's self-synchronizing periodicity).
pub fn embed_y_masked(y: &mut [f32], width: usize, height: usize, payload: &[u8; DATA_BYTES], mask_strength: f32) {
    dwt_2d_fwd(y, width, height, DECOMP_DEPTH);
    let bits = payload_to_bits(&full_payload(payload)); // data + CRC + reserved-ECC → 192 bits
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
pub fn decode_y(y: &[f32], width: usize, height: usize) -> [u8; DATA_BYTES] {
    let total = correlate_embed_levels(y, width, height);
    let mut bits = [false; PAYLOAD_BITS];
    for (i, b) in bits.iter_mut().enumerate() { *b = total[i] > 0.0; }
    decode_bits(&bits).data
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
pub fn decode_y_at_size(y: &[f32], width: usize, height: usize, orig_w: usize, orig_h: usize) -> [u8; DATA_BYTES] {
    decode_y_at_size_verbose(y, width, height, orig_w, orig_h).0.data
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
) -> (Decoded, f32) {
    let regridded = resample_y(y, width, height, orig_w, orig_h);
    let total = correlate_embed_levels(&regridded, orig_w, orig_h);
    let score: f32 = total.iter().map(|v| v.abs()).sum();
    let mut bits = [false; PAYLOAD_BITS];
    for (i, b) in bits.iter_mut().enumerate() { *b = total[i] > 0.0; }
    (decode_bits(&bits), score)
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

// ── Blind registration + decode (native-only; gated by `registration`) ─────────
//
// Productized from the Stage 1/2 experiments.  Given a suspect Y plane that has
// been arbitrarily cropped and/or rescaled, recover the scale (autocorrelation of
// the watermark's periodic tiling) and the crop offset (keyed matched filter on
// the folded tile), then read the payload from the per-bit correlation signs — no
// original image or known dimensions required.  Uses an FFT (rustfft), so it is
// feature-gated to keep it out of the WASM build.

#[cfg(feature = "registration")]
pub mod registration {
    use super::*;
    use rustfft::{num_complex::Complex, FftPlanner};

    /// Matched-filter fold period (px @ embed scale).  LCM of the level-2 tile
    /// period (TILE_SIDE·4 = 256) and the level-3 tile period (TILE_SIDE·8 = 512),
    /// so both embedded levels align under one fold.
    pub const FOLD: usize = TILE_SIDE * 8; // 512
    /// Reference period the *scale* is measured against (level-2 = dominant).
    const SCALE_REF: usize = TILE_SIDE * 4; // 256
    const REFINE_STEPS: i32 = 2;            // ± this many 0.5% scale nudges
    const REFINE_FRAC:  f32 = 0.005;
    const CANDIDATES: usize = 4;            // top-K autocorr scale peaks (each expanded with ½×, ⅓× harmonics)
    const REFINE_CANDIDATES: usize = 4;     // how many of the (harmonic-expanded) candidates to ±refine on total coarse failure
    // Scale-estimation block: 1024 holds ≥4 tile periods even at full scale (a 512
    // block holds only 2 → weak/ambiguous autocorr peak, which caused the full-scale
    // misses in the sweep).  Stage-1 prominence at 1.0×: ~18 (512) vs ~111 (1024).
    const SCALE_BLOCK: usize = 1024;

    /// Outcome of a blind decode.
    pub struct BlindResult {
        pub data:       [u8; DATA_BYTES],
        pub verified:   bool,          // CRC-32 over `data` matched the embedded checksum
        pub errors_corrected: u8,      // BCH bit-errors corrected before the CRC verified
        pub scale:      f32,            // recovered suspect scale (≈ size / original)
        pub offset:     (usize, usize),// recovered tile-phase offset (mod FOLD)
        pub confidence: f32,           // phase-peak prominence (peak ÷ median); ≫1 = solid
    }

    // ── small FFT / DSP helpers ──
    fn fft_2d(buf: &mut [Complex<f32>], n: usize, planner: &mut FftPlanner<f32>, inv: bool) {
        let fft = if inv { planner.plan_fft_inverse(n) } else { planner.plan_fft_forward(n) };
        for r in 0..n { fft.process(&mut buf[r * n..r * n + n]); }
        let mut col = vec![Complex::new(0.0, 0.0); n];
        for c in 0..n {
            for r in 0..n { col[r] = buf[r * n + c]; }
            fft.process(&mut col);
            for r in 0..n { buf[r * n + c] = col[r]; }
        }
    }

    fn hann(i: usize, n: usize) -> f32 { let x = std::f32::consts::PI * i as f32 / (n as f32 - 1.0); x.sin() * x.sin() }

    fn box_blur(src: &[f32], b: usize, radius: usize) -> Vec<f32> {
        let k = (2 * radius + 1) as f32;
        let mut tmp = vec![0.0f32; b * b];
        for y in 0..b { for x in 0..b {
            let mut s = 0.0;
            for d in 0..=2 * radius { let xx = (x as isize + d as isize - radius as isize).clamp(0, b as isize - 1) as usize; s += src[y * b + xx]; }
            tmp[y * b + x] = s / k;
        }}
        let mut out = vec![0.0f32; b * b];
        for x in 0..b { for y in 0..b {
            let mut s = 0.0;
            for d in 0..=2 * radius { let yy = (y as isize + d as isize - radius as isize).clamp(0, b as isize - 1) as usize; s += tmp[yy * b + x]; }
            out[y * b + x] = s / k;
        }}
        out
    }

    fn extract(src: &[f32], w: usize, x0: usize, y0: usize, b: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; b * b];
        for ry in 0..b { let s = (y0 + ry) * w + x0; out[ry * b..ry * b + b].copy_from_slice(&src[s..s + b]); }
        out
    }

    /// Spectral-whitened autocorrelation of a b×b block (2b×2b, zero-lag centred).
    fn autocorr_whitened(block: &[f32], b: usize, planner: &mut FftPlanner<f32>) -> Vec<f32> {
        let n = 2 * b;
        let mean = block.iter().sum::<f32>() / (b * b) as f32;
        let mut buf = vec![Complex::new(0.0f32, 0.0); n * n];
        for y in 0..b { let wy = hann(y, b); for x in 0..b {
            buf[y * n + x] = Complex::new((block[y * b + x] - mean) * wy * hann(x, b), 0.0);
        }}
        fft_2d(&mut buf, n, planner, false);
        let power: Vec<f32> = buf.iter().map(|z| z.norm_sqr()).collect();
        let env = box_blur(&power, n, 6);
        for (z, (&p, &e)) in buf.iter_mut().zip(power.iter().zip(env.iter())) { *z = Complex::new(p / (e + 1e-3), 0.0); }
        fft_2d(&mut buf, n, planner, true);
        let norm = (n * n) as f32;
        let mut out = vec![0.0f32; n * n];
        for y in 0..n { for x in 0..n { out[((y + b) % n) * n + (x + b) % n] = buf[y * n + x].re / norm; } }
        out
    }

    /// Diagnostic (Phase 5): the ranked autocorrelation peaks of the centre scale
    /// block, as `(period_lag, strength)` strongest-first.  Lets a characterization
    /// test see where the *true* tile period sits versus spurious / JPEG-harmonic
    /// peaks — e.g. why a low-detail (white-seamless) frame mis-locks under JPEG.
    /// Mirrors `blind_scale`'s profile so the rankings match what it sees.  Not used
    /// by the decoder (yet) — but the natural seed for Phase-7 candidate diversity.
    pub fn scale_peaks(y: &[f32], w: usize, h: usize, top_k: usize) -> Vec<(usize, f32)> {
        let mut planner = FftPlanner::<f32>::new();
        let sb = SCALE_BLOCK.min(w).min(h);
        let blk = extract(y, w, (w - sb) / 2, (h - sb) / 2, sb);
        let ac = autocorr_whitened(&blk, sb, &mut planner);
        let n = 2 * sb;
        let c = sb;
        let (min_lag, max_lag) = (24usize, sb - 2);
        let prof: Vec<f32> = (0..max_lag)
            .map(|lag| if lag < min_lag { f32::MIN } else { ac[c * n + c + lag].max(ac[(c + lag) * n + c]) })
            .collect();
        let mut peaks: Vec<(usize, f32)> = ((min_lag + 1)..(max_lag - 1))
            .filter(|&lag| prof[lag] >= prof[lag - 1] && prof[lag] >= prof[lag + 1])
            .map(|lag| (lag, prof[lag]))
            .collect();
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        peaks.truncate(top_k);
        peaks
    }

    /// Diagnostic (Phase-7 investigation): brute-force, CRC/ECC-gated scale sweep.
    /// For each scale in `scales` (in order), rescale → fold → register → decode via
    /// `register_decode` (its own offset recovery), **stopping at the first verified
    /// scale**.  Returns `(scale, prominence, verified, errors_corrected)` per scale
    /// tried.  Bypasses the autocorr scale *ranking* entirely, to separate "the true
    /// scale was never tried/ranked" from "signal too weak at any scale."
    pub fn scale_sweep(y: &[f32], w: usize, h: usize, scales: &[f32]) -> Vec<(f32, f32, bool, u8)> {
        let mut planner = FftPlanner::<f32>::new();
        let templates = bit_templates();
        let tfft: Vec<Vec<Complex<f32>>> = templates.iter().map(|t| {
            let mut f: Vec<Complex<f32>> = t.iter().map(|&v| Complex::new(v, 0.0)).collect();
            fft_2d(&mut f, FOLD, &mut planner, false);
            f
        }).collect();
        let mut out = Vec::with_capacity(scales.len());
        for &s in scales {
            let inv = 1.0 / s;
            let tw = ((w as f32 * inv).round() as usize).max(2 * FOLD);
            let th = ((h as f32 * inv).round() as usize).max(2 * FOLD);
            let (dec, prom, _, _) = register_decode(y, w, h, tw, th, &tfft, &mut planner);
            let v = dec.verified;
            out.push((s, prom, v, dec.errors_corrected));
            if v { break; }
        }
        out
    }

    /// Diagnostic: like `scale_peaks` but **sums** the whitened autocorrelation over a
    /// grid of blocks tiling the whole image (~b/2 spacing, ≤4 per axis), then ranks the
    /// summed lag profile.  The watermark period reinforces across blocks while content /
    /// JPEG peaks average down, and it uses the full extent of oblong crops (the single
    /// centre block wastes the long axis).
    pub fn scale_peaks_multi(y: &[f32], w: usize, h: usize, top_k: usize) -> Vec<(usize, f32)> {
        let mut planner = FftPlanner::<f32>::new();
        let b = SCALE_BLOCK.min(w).min(h);
        let n = 2 * b;
        let c = b;
        let (min_lag, max_lag) = (24usize, b - 2);
        let axis_pos = |span: usize| -> Vec<usize> {
            if span == 0 { return vec![0]; }
            let count = (span / (b / 2) + 1).clamp(1, 4);
            (0..count).map(|i| if count == 1 { span / 2 } else { span * i / (count - 1) }).collect()
        };
        let xs = axis_pos(w - b);
        let ys = axis_pos(h - b);
        let mut prof = vec![0.0f32; max_lag];
        for &y0 in &ys {
            for &x0 in &xs {
                let blk = extract(y, w, x0, y0, b);
                let ac = autocorr_whitened(&blk, b, &mut planner);
                for lag in min_lag..max_lag {
                    prof[lag] += ac[c * n + c + lag].max(ac[(c + lag) * n + c]);
                }
            }
        }
        let mut peaks: Vec<(usize, f32)> = ((min_lag + 1)..(max_lag - 1))
            .filter(|&lag| prof[lag] >= prof[lag - 1] && prof[lag] >= prof[lag + 1])
            .map(|lag| (lag, prof[lag]))
            .collect();
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        peaks.truncate(top_k);
        peaks
    }

    /// Per-bit spatial templates: pn_b tiled into the embed bands (LH/HL at
    /// EMBED_LEVELS), inverse-DWT, one FOLD×FOLD interior tile.  Payload-independent
    /// references; the secret key (WM_KEY via pn_tile) is what makes them keyed.
    fn bit_templates() -> Vec<Vec<f32>> {
        let frame = 2 * FOLD; // 1024 — interior tile avoids inverse-DWT edge effects
        (0..PAYLOAD_BITS).map(|bit| {
            let tile = pn_tile(bit);
            let mut c = vec![0.0f32; frame * frame];
            for &level in EMBED_LEVELS {
                for &band in &[Subband::LH, Subband::HL] {
                    let (r0, r1, c0, c1) = subband_bounds(frame, frame, level, band);
                    for r in r0..r1 { for cc in c0..c1 {
                        let ti = ((r - r0) % TILE_SIDE) * TILE_SIDE + (cc - c0) % TILE_SIDE;
                        c[r * frame + cc] = tile[ti];
                    }}
                }
            }
            dwt_2d_inv(&mut c, frame, frame, DECOMP_DEPTH);
            extract(&c, frame, FOLD / 2, FOLD / 2, FOLD) // tile at (256,256)
        }).collect()
    }

    /// Band-pass an image to the embed detail bands (keep LH/HL at EMBED_LEVELS).
    fn keep_embed_bands(img: &[f32], w: usize, h: usize) -> Vec<f32> {
        let mut c = img.to_vec();
        dwt_2d_fwd(&mut c, w, h, DECOMP_DEPTH);
        let mut out = vec![0.0f32; w * h];
        for &level in EMBED_LEVELS {
            for &band in &[Subband::LH, Subband::HL] {
                let (r0, r1, c0, c1) = subband_bounds(w, h, level, band);
                for r in r0..r1 { for cc in c0..c1 { out[r * w + cc] = c[r * w + cc]; } }
            }
        }
        dwt_2d_inv(&mut out, w, h, DECOMP_DEPTH);
        out
    }

    /// Fold an image into one FOLD×FOLD tile by summing all whole period-FOLD shifts.
    fn fold_tile(img: &[f32], w: usize, h: usize) -> Vec<f32> {
        let mut f = vec![0.0f32; FOLD * FOLD];
        let (tw, th) = ((w / FOLD) * FOLD, (h / FOLD) * FOLD);
        for y in 0..th { for x in 0..tw { f[(y % FOLD) * FOLD + (x % FOLD)] += img[y * w + x]; } }
        f
    }

    /// Register + decode at a chosen rescale target.  Returns (payload, prominence,
    /// offset_x, offset_y).  `tfft` are the precomputed template FFTs.
    fn register_decode(
        suspect: &[f32], nw: usize, nh: usize, tw: usize, th: usize,
        tfft: &[Vec<Complex<f32>>], planner: &mut FftPlanner<f32>,
    ) -> (Decoded, f32, usize, usize) {
        let rescaled = resample_y(suspect, nw, nh, tw, th);
        let band = keep_embed_bands(&rescaled, tw, th);
        let folded = fold_tile(&band, tw, th);

        let mut ff: Vec<Complex<f32>> = folded.iter().map(|&v| Complex::new(v, 0.0)).collect();
        fft_2d(&mut ff, FOLD, planner, false);

        let mut score = vec![0.0f32; FOLD * FOLD];
        let mut maps: Vec<Vec<f32>> = Vec::with_capacity(tfft.len());
        let norm = (FOLD * FOLD) as f32;
        for tf in tfft {
            let mut prod: Vec<Complex<f32>> = ff.iter().zip(tf.iter()).map(|(a, b)| *a * b.conj()).collect();
            fft_2d(&mut prod, FOLD, planner, true);
            let c: Vec<f32> = prod.iter().map(|z| z.re / norm).collect();
            for (s, v) in score.iter_mut().zip(c.iter()) { *s += v.abs(); }
            maps.push(c);
        }
        let (mut bp, mut bv) = (0usize, f32::MIN);
        for (i, &v) in score.iter().enumerate() { if v > bv { bv = v; bp = i; } }
        let smed = { let mut s = score.clone(); s.sort_by(|a, b| a.partial_cmp(b).unwrap()); s[s.len() / 2].max(1e-6) };
        let mut bits = [false; PAYLOAD_BITS];
        for (b, m) in maps.iter().enumerate() { bits[b] = m[bp] > 0.0; }
        (decode_bits(&bits), bv / smed, bp % FOLD, bp / FOLD)
    }

    /// Progress events emitted during a blind decode, for an optional UI.  The
    /// library stays print-free (and WASM-safe); the caller renders these however
    /// it likes (a one-line bar, verbose log, or ignores them).
    pub enum Progress {
        Phase(&'static str), // "building templates", "estimating scale", "refining…"
        /// A coarse scale candidate, tried strongest-autocorr-first (rank 1 = strongest).
        Candidate { rank: usize, total: usize, scale: f32, strength: f32, prominence: f32, verified: bool, errors: u8 },
        /// A fine ±refinement attempt (only reached when no coarse candidate verified).
        Refine { candidate: usize, scale: f32, prominence: f32, verified: bool, errors: u8 },
    }

    /// Blindly recover scale + crop offset and decode the payload from a suspect Y
    /// plane (cropped/rescaled, dimensions unknown).  No original image needed.
    pub fn decode_blind_auto(y: &[f32], w: usize, h: usize) -> BlindResult {
        decode_blind_auto_cb(y, w, h, &mut |_| {})
    }

    /// Like `decode_blind_auto` but reports `Progress` events to `progress`.
    pub fn decode_blind_auto_cb(
        y: &[f32], w: usize, h: usize, progress: &mut dyn FnMut(Progress),
    ) -> BlindResult {
        let mut planner = FftPlanner::<f32>::new();

        // Precompute template FFTs once.
        progress(Progress::Phase("building templates"));
        let templates = bit_templates();
        let tfft: Vec<Vec<Complex<f32>>> = templates.iter().map(|t| {
            let mut f: Vec<Complex<f32>> = t.iter().map(|&v| Complex::new(v, 0.0)).collect();
            fft_2d(&mut f, FOLD, &mut planner, false);
            f
        }).collect();

        // Candidate scales: the top-K autocorr peaks of the centre block, strongest
        // first, **each expanded with its ½× and ⅓× harmonics**.  Downscaling low-passes
        // the mark, so the strongest peak is often the level-3 / 2× harmonic and the true
        // (level-2) period is its half — trying {s, s/2, s/3} recovers it.  CRC (with ECC)
        // is the verdict: the first verified decode IS the answer, full stop — the exact
        // scale/offset are incidental once the payload checks out.
        progress(Progress::Phase("estimating scale"));
        let peaks = scale_peaks(y, w, h, CANDIDATES);
        let mut tries: Vec<(f32, f32)> = Vec::new(); // (scale, parent autocorr strength), in try order
        for &(lag, strength) in &peaks {
            let s0 = lag as f32 / SCALE_REF as f32;
            for div in [1.0f32, 2.0, 3.0] {
                let s = (s0 / div).clamp(0.1, 4.0);
                if !tries.iter().any(|&(t, _)| (t - s).abs() < 1e-3) { tries.push((s, strength)); }
            }
        }
        if tries.is_empty() { tries.push((1.0, 0.0)); } // degenerate fallback
        let total = tries.len();

        let mut best = (Decoded { data: [0u8; DATA_BYTES], verified: false, errors_corrected: 0 },
                        f32::MIN, (0usize, 0usize), tries[0].0);

        // Coarse pass — every candidate (peaks + harmonics) at its scale, strongest-peak
        // first.  First CRC/ECC-verified decode wins.
        for (ci, &(s, strength)) in tries.iter().enumerate() {
            let inv = 1.0 / s;
            let tw = ((w as f32 * inv).round() as usize).max(2 * FOLD);
            let th = ((h as f32 * inv).round() as usize).max(2 * FOLD);
            let (dec, prom, ox, oy) = register_decode(y, w, h, tw, th, &tfft, &mut planner);
            progress(Progress::Candidate { rank: ci + 1, total, scale: s, strength, prominence: prom, verified: dec.verified, errors: dec.errors_corrected });
            if dec.verified {
                return BlindResult { data: dec.data, verified: true, errors_corrected: dec.errors_corrected, scale: s, offset: (ox, oy), confidence: prom };
            }
            if prom > best.1 { best = (dec, prom, (ox, oy), s); }
        }

        // Refine pass (the "extra effort") — only when no coarse candidate verified:
        // ±nudge the most-promising scale into the narrow notch (the coarse autocorr lag
        // is ~0.4%-quantized; the matched notch is <0.25%).
        progress(Progress::Phase("refining (no clean candidate)"));
        for (ci, &(s0, _)) in tries.iter().take(REFINE_CANDIDATES).enumerate() {
            for k in -REFINE_STEPS..=REFINE_STEPS {
                if k == 0 { continue; } // k=0 already covered by the coarse pass
                let s = s0 * (1.0 + REFINE_FRAC * k as f32);
                let inv = 1.0 / s;
                let tw = ((w as f32 * inv).round() as usize).max(2 * FOLD);
                let th = ((h as f32 * inv).round() as usize).max(2 * FOLD);
                let (dec, prom, ox, oy) = register_decode(y, w, h, tw, th, &tfft, &mut planner);
                progress(Progress::Refine { candidate: ci + 1, scale: s, prominence: prom, verified: dec.verified, errors: dec.errors_corrected });
                if dec.verified {
                    return BlindResult { data: dec.data, verified: true, errors_corrected: dec.errors_corrected, scale: s, offset: (ox, oy), confidence: prom };
                }
                if prom > best.1 { best = (dec, prom, (ox, oy), s); }
            }
        }

        BlindResult { data: best.0.data, verified: best.0.verified, errors_corrected: best.0.errors_corrected, scale: best.3, offset: best.2, confidence: best.1 }
    }
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

    // ── BCH ECC (Phase 1: standalone codec) ──────────────────────────────────

    fn rand_info(state: &mut u64) -> [bool; bch::INFO_BITS] {
        let mut info = [false; bch::INFO_BITS];
        for b in info.iter_mut() { *b = xorshift64(state) & 1 == 1; }
        info
    }

    fn make_codeword(codec: &bch::Bch, info: &[bool; bch::INFO_BITS]) -> [bool; bch::CODE_BITS] {
        let parity = codec.encode_parity(info);
        let mut code = [false; bch::CODE_BITS];
        code[..bch::INFO_BITS].copy_from_slice(info);
        code[bch::INFO_BITS..].copy_from_slice(&parity);
        code
    }

    fn inject_distinct(code: &mut [bool; bch::CODE_BITS], n: usize, state: &mut u64) -> Vec<usize> {
        let mut chosen = Vec::with_capacity(n);
        while chosen.len() < n {
            let p = (xorshift64(state) as usize) % bch::CODE_BITS;
            if !chosen.contains(&p) { chosen.push(p); code[p] ^= true; }
        }
        chosen
    }

    #[test]
    fn bch_clean_codeword_decodes_with_zero_errors() {
        let codec = bch::Bch::new();
        let mut st = 0x1234_5678_9abc_def0u64;
        let info = rand_info(&mut st);
        let mut code = make_codeword(&codec, &info);
        let orig = code;
        // A validly-encoded codeword has zero syndromes → 0 corrections, untouched.
        assert_eq!(codec.correct(&mut code), Some(0), "clean codeword should report 0 errors");
        assert_eq!(code, orig, "clean codeword must be unchanged");
    }

    #[test]
    fn bch_corrects_every_single_bit_error() {
        let codec = bch::Bch::new();
        let mut st = 0xdead_beef_0bad_f00du64;
        let info = rand_info(&mut st);
        let code0 = make_codeword(&codec, &info);
        for pos in 0..bch::CODE_BITS {
            let mut code = code0;
            code[pos] ^= true;
            assert_eq!(codec.correct(&mut code), Some(1), "pos {pos}: not reported as 1 error");
            assert_eq!(code, code0, "pos {pos}: not corrected");
        }
    }

    #[test]
    fn bch_corrects_up_to_four_errors() {
        let codec = bch::Bch::new();
        let mut st = 0x0123_4567_89ab_cdefu64;
        for _ in 0..3000 {
            let info = rand_info(&mut st);
            let code0 = make_codeword(&codec, &info);
            let nerr = 1 + (xorshift64(&mut st) % (bch::T as u64)) as usize; // 1..=4
            let mut code = code0;
            inject_distinct(&mut code, nerr, &mut st);
            assert_eq!(codec.correct(&mut code), Some(nerr), "expected {nerr} errors corrected");
            assert_eq!(code, code0, "≤4 errors not fully corrected");
        }
    }

    #[test]
    fn full_payload_is_a_valid_bch_codeword() {
        // Phase 2: the reserved ECC bytes now carry BCH parity, so the assembled
        // 192-bit payload is a valid codeword — correct() reports 0 errors — and a
        // single induced flip is corrected back to it.
        let data: [u8; DATA_BYTES] = [
            0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE,
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
        ];
        let bits = payload_to_bits(&full_payload(&data));
        let mut code = [false; bch::CODE_BITS];
        code.copy_from_slice(&bits);
        let codec = bch::Bch::new();
        assert_eq!(codec.correct(&mut code), Some(0), "fresh payload must be a valid codeword");
        let clean = code;
        code[42] ^= true;
        assert_eq!(codec.correct(&mut code), Some(1));
        assert_eq!(code, clean, "single flip not corrected back to the codeword");
    }

    #[test]
    fn decode_bits_ecc_rescues_up_to_four_data_errors() {
        // Phase 3: decode_bits applies CRC-first → ECC-on-failure → CRC-recheck.
        let data: [u8; DATA_BYTES] = [
            0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE,
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
        ];
        let clean = payload_to_bits(&full_payload(&data));

        // Clean read → verified, no correction needed.
        let d0 = decode_bits(&clean);
        assert!(d0.verified && d0.errors_corrected == 0 && d0.data == data, "clean decode");

        // 1..=4 flips in the info region (data+CRC) break the CRC, so ECC must run
        // and rescue them — recovering the data and reporting the exact count.
        let mut st = 0xABCD_1234_5678_9999u64;
        for k in 1..=4usize {
            for _ in 0..200 {
                let mut bits = clean;
                let mut chosen = Vec::new();
                while chosen.len() < k {
                    let p = (xorshift64(&mut st) as usize) % bch::INFO_BITS;
                    if !chosen.contains(&p) { chosen.push(p); bits[p] ^= true; }
                }
                let d = decode_bits(&bits);
                assert!(d.verified, "k={k}: ECC failed to rescue");
                assert_eq!(d.data, data, "k={k}: wrong data after correction");
                assert_eq!(d.errors_corrected as usize, k, "k={k}: wrong error count");
            }
        }

        // Errors confined to the parity region: CRC-first short-circuits (the data
        // is already clean), so no correction is reported.
        let mut bits = clean;
        bits[170] ^= true;
        bits[180] ^= true;
        let d = decode_bits(&bits);
        assert!(d.verified && d.errors_corrected == 0, "parity-only errors shouldn't need ECC");
        assert_eq!(d.data, data);
    }

    #[test]
    fn bch_never_restores_original_beyond_t() {
        // 5..=8 errors exceed t=4: the decoder must fail (None) or land on some
        // *other* codeword — never silently restore the original (a false success).
        let codec = bch::Bch::new();
        let mut st = 0xfeed_face_cafe_babeu64;
        for _ in 0..3000 {
            let info = rand_info(&mut st);
            let code0 = make_codeword(&codec, &info);
            let nerr = 5 + (xorshift64(&mut st) % 4) as usize; // 5..=8
            let mut code = code0;
            inject_distinct(&mut code, nerr, &mut st);
            if codec.correct(&mut code).is_some() {
                assert_ne!(code, code0, "decoder falsely restored original from {nerr} errors");
            }
        }
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
    #[ignore]
    fn emit_visual_samples() {
        use image::ColorType;

        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join("tests");
        // test_a = detail-rich; test_e = white-seamless portrait (skin midtones +
        // bright backdrop) — the orange-peel / luminance-masking subject.
        for (stem, file) in [("a", "test_a.jpg"), ("e", "test_e.jpg")] {
            let path = tests_dir.join(file);
            if !path.exists() { continue; }
            let img = image::open(&path).unwrap().into_rgb8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            let pixels = img.into_raw();

            let orig_y = extract_y_rgb(&pixels);
            let mut wm_y = orig_y.clone();
            embed_y(&mut wm_y, w, h, &PHASE3_PAYLOAD);

            let mut pixels_wm = pixels.clone();
            write_y_delta_rgb(&mut pixels_wm, &orig_y, &wm_y);
            let residual = emit_residual(&orig_y, &wm_y, RESIDUAL_AMP);

            image::save_buffer(tests_dir.join(format!("sample_{stem}_original.png")),
                &pixels, w as u32, h as u32, ColorType::Rgb8).unwrap();
            image::save_buffer(tests_dir.join(format!("sample_{stem}_watermarked.png")),
                &pixels_wm, w as u32, h as u32, ColorType::Rgb8).unwrap();
            image::save_buffer(tests_dir.join(format!("sample_{stem}_residual.png")),
                &residual, w as u32, h as u32, ColorType::Rgb8).unwrap();

            let p = psnr(&orig_y, &wm_y);
            let max_d = orig_y.iter().zip(wm_y.iter())
                .map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            println!("visual samples [{file}] → sample_{stem}_*.png  PSNR={p:.1} dB  max|Δ|={max_d:.1} LSB  (alpha={ALPHA}, levels={EMBED_LEVELS:?})");
        }
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

        let errors = crop_errs(&recovered.data);

        println!(
            "resize {}/{}  {}×{} → {}×{} → regrid {}×{}  errors={}/{}  score={:.1}  crc={}  (alpha={})",
            scale_num, scale_den,
            w, h, sw, sh, w, h,
            errors, DATA_BYTES * 8, score, recovered.verified, ALPHA,
        );

        (errors, recovered.data)
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
                crop_errs(&pa.data), sa,
                crop_errs(&pb.data), sb,
                crop_errs(&pc.data), sc,
            ));
            println!("{:<15} A={:>3}/{:>3.0}  B={:>3}/{:>3.0}  C={:>3}/{:>3.0}",
                label, crop_errs(&pa.data), sa, crop_errs(&pb.data), sb, crop_errs(&pc.data), sc);
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
        (crop_errs(&split_payload(&bits_to_payload(&bits)).data), bv / smed, bp % FOLD, bp / FOLD)
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

    // ── Step 2: blind `--auto` sweep across all image fixtures ─────────────────
    //
    // For every `tests/*.jpg`, embed → (optionally JPEG) → crop → rescale, then run
    // the *fully blind* `decode_blind_auto` (recover scale + offset, decode) and
    // record raw (pre-ECC) bit errors + confidence.  Measures the real envelope
    // across varied content (clean vs busy) and sizes the ECC.  Requires the
    // `registration` feature:
    //   cargo test -p glimr --features registration blind_auto_sweep -- --ignored --nocapture
    #[cfg(feature = "registration")]
    #[test]
    #[ignore]
    fn blind_auto_sweep() {
        use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};

        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests");
        let reports = reports_dir();

        let mut fixtures: Vec<std::path::PathBuf> = std::fs::read_dir(&tests_dir).unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "jpg" || x == "jpeg").unwrap_or(false))
            .collect();
        fixtures.sort();

        let scales  = [1.0f32, 0.5];
        let offsets = [("none", 0usize, 0usize), ("crop 130,200", 130, 200)];
        let jpegs   = [("raw", false), ("q80", true)];

        let mut rows = String::new();
        let (mut clean, mut total, mut verified) = (0usize, 0usize, 0usize);

        for path in &fixtures {
            let img = image::open(path).unwrap().into_rgb8();
            let (ow, oh) = (img.width() as usize, img.height() as usize);
            let pixels = img.into_raw();
            let orig_y = extract_y_rgb(&pixels);

            let mut wm_y = orig_y.clone();
            embed_y_masked(&mut wm_y, ow, oh, &PHASE3_PAYLOAD, MASK_STRENGTH);

            // Watermarked-then-JPEG'd Y (realistic compressed channel).
            let wm_jpeg_y = {
                let mut pw = pixels.clone();
                write_y_delta_rgb(&mut pw, &orig_y, &wm_y);
                let mut jpg = Vec::new();
                JpegEncoder::new_with_quality(&mut jpg, 80)
                    .write_image(&pw, ow as u32, oh as u32, ExtendedColorType::Rgb8).unwrap();
                let dec = image::load_from_memory(&jpg).unwrap().into_rgb8();
                extract_y_rgb(dec.as_raw())
            };

            let name = path.file_name().unwrap().to_string_lossy().to_string();
            for &(jlabel, jpeg) in &jpegs {
                let base = if jpeg { &wm_jpeg_y } else { &wm_y };
                for &s in &scales {
                    for &(olabel, ox, oy) in &offsets {
                        let (cw, ch) = (ow - ox, oh - oy);
                        let cropped = crop_rgb_y(base, ow, ox, oy, cw, ch);
                        let (nw, nh) = ((cw as f32 * s).round() as usize, (ch as f32 * s).round() as usize);
                        let suspect = resample_y(&cropped, cw, ch, nw, nh);

                        let r = registration::decode_blind_auto(&suspect, nw, nh);
                        let errs = crop_errs(&r.data);
                        let serr = (r.scale / s - 1.0) * 100.0;
                        total += 1;
                        if errs == 0 { clean += 1; }
                        if r.verified { verified += 1; }
                        rows.push_str(&format!(
                            "| {:<13} | {:<3} | {:>4.2} | {:<12} | {:>+6.1}% | {:>6.1} | {:>3}/128 | {:^3} |\n",
                            name, jlabel, s, olabel, serr, r.confidence, errs, if r.verified { "✓" } else { "·" }));
                        println!("{name} {jlabel} s={s:.2} {olabel}: scale_err={serr:+.1}% conf={:.1} errs={errs} crc={}", r.confidence, r.verified);
                    }
                }
            }
        }

        let report = format!(
"# Blind `--auto` sweep — multi-image, pre-ECC

Embed (CDF 5/3, ALPHA={alpha}, levels {levels:?}, mask {mask}) → optional JPEG q80 → crop →
rescale, then **fully blind** `decode_blind_auto` (recover scale + crop offset, decode).
Raw bit errors are pre-ECC; `confidence` is the phase-peak prominence.  This measures the
real envelope across varied content and sizes the ECC budget.

Fixtures: {fixtures} images via `tests/*.jpg`.  **Clean (0-error) cells: {clean}/{total}; CRC-verified: {verified}/{total}.**

Regenerate: `cargo test -p glimr --features registration blind_auto_sweep -- --ignored --nocapture`

| image         | jpg | scale | crop         | scale err | confidence | bit errors | crc |
|---------------|-----|-------|--------------|-----------|------------|------------|-----|
{rows}
_`crc` ✓ = the embedded CRC-32 verified (definitive). Cells with a few errors are ECC's job;\
 cells near 64/128 are registration failures (need more signal)._
",
            alpha = ALPHA, levels = EMBED_LEVELS, mask = MASK_STRENGTH,
            fixtures = fixtures.len(), clean = clean, total = total, verified = verified, rows = rows,
        );
        let path = reports.join("blind_auto_sweep.md");
        std::fs::write(&path, report).unwrap();
        println!("blind --auto sweep → {}  ({clean}/{total} clean, {verified}/{total} crc-verified)", path.display());
    }

    // ── Phase 5a: channel-quality waterfall (matched decode) ──────────────────
    //
    // Isolates the *channel-noise* axis from registration: decode at the *known*
    // size (registration exact) over a fine JPEG-quality grid (native and 0.5×).
    // Records raw pre-ECC codeword errors, the CRC verdict, and how many bits ECC
    // corrected — so we see whether the 1..4-error band ECC targets actually exists,
    // how steep the waterfall is, and how much quality range t=4 buys.

    // Faithful RGB downscale (per-channel triangle filter) for the quality sweep.
    fn resample_rgb(rgb: &[u8], w: usize, h: usize, nw: usize, nh: usize) -> Vec<u8> {
        let mut pl = [vec![0f32; w * h], vec![0f32; w * h], vec![0f32; w * h]];
        for (i, px) in rgb.chunks(3).enumerate() {
            pl[0][i] = px[0] as f32; pl[1][i] = px[1] as f32; pl[2][i] = px[2] as f32;
        }
        let r: Vec<Vec<f32>> = pl.iter().map(|p| resample_y(p, w, h, nw, nh)).collect();
        let mut out = vec![0u8; nw * nh * 3];
        for i in 0..nw * nh {
            out[i * 3]     = r[0][i].clamp(0.0, 255.0) as u8;
            out[i * 3 + 1] = r[1][i].clamp(0.0, 255.0) as u8;
            out[i * 3 + 2] = r[2][i].clamp(0.0, 255.0) as u8;
        }
        out
    }

    #[test]
    #[ignore]
    fn channel_waterfall() {
        use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests");
        let reports = reports_dir();
        let expected = payload_to_bits(&full_payload(&PHASE3_PAYLOAD));
        let qualities = [90u8, 80, 70, 60, 50, 45, 40, 35, 30, 25, 20, 15, 10];
        let scales = [("native", 1.0f32), ("0.5x", 0.5)];
        let images = ["test_a.jpg", "test_e.jpg"];

        let mut rows = String::new();
        for img_name in images {
            let img = image::open(tests_dir.join(img_name)).unwrap().into_rgb8();
            let (ow, oh) = (img.width() as usize, img.height() as usize);
            let pixels = img.into_raw();
            let orig_y = extract_y_rgb(&pixels);
            let mut wm_y = orig_y.clone();
            embed_y(&mut wm_y, ow, oh, &PHASE3_PAYLOAD);
            let mut wm_rgb = pixels.clone();
            write_y_delta_rgb(&mut wm_rgb, &orig_y, &wm_y);

            for (slabel, s) in scales {
                let (sw, sh) = ((ow as f32 * s).round() as usize, (oh as f32 * s).round() as usize);
                let scaled = if s == 1.0 { wm_rgb.clone() } else { resample_rgb(&wm_rgb, ow, oh, sw, sh) };
                for &q in &qualities {
                    let mut jpg = Vec::new();
                    JpegEncoder::new_with_quality(&mut jpg, q)
                        .write_image(&scaled, sw as u32, sh as u32, ExtendedColorType::Rgb8).unwrap();
                    let suspect = extract_y_rgb(image::load_from_memory(&jpg).unwrap().into_rgb8().as_raw());
                    // Matched decode at the known original size → registration is exact.
                    let regrid = resample_y(&suspect, sw, sh, ow, oh);
                    let total = correlate_embed_levels(&regrid, ow, oh);
                    let mut bits = [false; PAYLOAD_BITS];
                    for (i, b) in bits.iter_mut().enumerate() { *b = total[i] > 0.0; }
                    let raw = bits.iter().zip(expected.iter()).filter(|(a, b)| a != b).count();
                    let d = decode_bits(&bits);
                    rows.push_str(&format!(
                        "| {:<11} | {:<6} | {:>3} | {:>3} | {:>3} | {:^3} |\n",
                        img_name, slabel, q, raw, d.errors_corrected, if d.verified { "✓" } else { "·" }));
                    println!("{img_name} {slabel} q{q}: raw_errs={raw} ecc_fixed={} crc={}", d.errors_corrected, d.verified);
                }
            }
        }
        let report = format!(
"# Phase 5a — channel-quality waterfall (matched decode)

Embed → scale → JPEG q → **decode at the known original size** (registration exact, so
the only error source is channel noise).  `raw` = pre-ECC bit errors over the 192-bit
codeword; `ecc` = bits BCH corrected; `crc` ✓ = verified after correction.  Shows
whether the 1..4-error band exists and how much quality range t=4 buys.

Regenerate: `cargo test -p glimr --release channel_waterfall -- --ignored --nocapture`

| image       | scale  | q | raw | ecc | crc |
|-------------|--------|---|-----|-----|-----|
{rows}
_If `raw` steps 0→1→2→3→4 before climbing, t=4 buys real range; if it jumps 0→≫4 the\
 waterfall is too steep for hard ECC and soft-decision is the lever._
");
        std::fs::write(reports.join("channel_waterfall.md"), report).unwrap();
    }

    // ── Phase 5c: scale-precision "cliff" (matched decode) ────────────────────
    // Decode a cleanly-embedded image at deliberately *wrong* target sizes to trace
    // error-vs-scale: how sharp the registration cliff is, and whether the alignment
    // score (soft metric) tracks the error count — i.e. is it a usable hill-climb
    // objective for the Phase-8 fine search.
    #[test]
    #[ignore]
    fn scale_precision() {
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests");
        let reports = reports_dir();
        let img = image::open(tests_dir.join("test_a.jpg")).unwrap().into_rgb8();
        let (ow, oh) = (img.width() as usize, img.height() as usize);
        let pixels = img.into_raw();
        let orig_y = extract_y_rgb(&pixels);
        let mut wm_y = orig_y.clone();
        embed_y(&mut wm_y, ow, oh, &PHASE3_PAYLOAD);
        let expected = payload_to_bits(&full_payload(&PHASE3_PAYLOAD));

        let mut rows = String::new();
        for k in -12i32..=12 {
            let dfrac = k as f32 * 0.0025; // ±3% in 0.25% steps
            let tw = (ow as f32 * (1.0 + dfrac)).round() as usize;
            let th = (oh as f32 * (1.0 + dfrac)).round() as usize;
            let (d, score) = decode_y_at_size_verbose(&wm_y, ow, oh, tw, th);
            let regrid = resample_y(&wm_y, ow, oh, tw, th);
            let total = correlate_embed_levels(&regrid, tw, th);
            let mut bits = [false; PAYLOAD_BITS];
            for (i, b) in bits.iter_mut().enumerate() { *b = total[i] > 0.0; }
            let raw = bits.iter().zip(expected.iter()).filter(|(a, b)| a != b).count();
            rows.push_str(&format!(
                "| {:>+6.2}% | {:>9} | {:>3} | {:>7.1} | {:^3} |\n",
                dfrac * 100.0, format!("{}×{}", tw, th), raw, score, if d.verified { "✓" } else { "·" }));
            println!("scale {:+.2}%: raw_errs={raw} score={score:.1} crc={}", dfrac * 100.0, d.verified);
        }
        let report = format!(
"# Phase 5c — scale-precision cliff (matched decode)

A cleanly-embedded test_a decoded at deliberately *wrong* target sizes (±3% in 0.25%
steps).  `raw` = pre-ECC codeword errors; `score` = alignment L1 (the candidate soft
metric).  Shows how sharp the registration cliff is and whether `score` tracks the
error count monotonically — i.e. is it a usable objective for fine-scale hill-climbing.

Regenerate: `cargo test -p glimr --release scale_precision -- --ignored --nocapture`

| scale err | target    | raw | score | crc |
|-----------|-----------|-----|-------|-----|
{rows}
_A narrow 0-error notch with `score` peaking there and falling off monotonically =\
 a clean objective for the Phase-8 fine search._
");
        std::fs::write(reports.join("scale_precision.md"), report).unwrap();
    }

    // ── Phase 5b: blind-sync mechanism (white-seamless vs detail-rich) ────────
    // Confirms the test_e gross failures are a *coarse sync* problem, not signal
    // loss: at the failing q80 scales, contrast detail-rich test_a with the
    // white-seamless test_e — recovered blind scale, whether matched `--size` decode
    // still verifies (signal survived), the top autocorr peak, and where the true
    // tile period ranks among the peaks.
    #[cfg(feature = "registration")]
    #[test]
    #[ignore]
    fn sync_mechanism() {
        use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests");
        let reports = reports_dir();
        let ref_period = (4 * TILE_SIDE) as f32; // level-2 tile period at original scale (256)

        let mut rows = String::new();
        for img_name in ["test_a.jpg", "test_e.jpg"] {
            let img = image::open(tests_dir.join(img_name)).unwrap().into_rgb8();
            let (ow, oh) = (img.width() as usize, img.height() as usize);
            let pixels = img.into_raw();
            let orig_y = extract_y_rgb(&pixels);
            let mut wm_y = orig_y.clone();
            embed_y(&mut wm_y, ow, oh, &PHASE3_PAYLOAD);
            let wm_jpeg_y = {
                let mut pw = pixels.clone();
                write_y_delta_rgb(&mut pw, &orig_y, &wm_y);
                let mut jpg = Vec::new();
                JpegEncoder::new_with_quality(&mut jpg, 80)
                    .write_image(&pw, ow as u32, oh as u32, ExtendedColorType::Rgb8).unwrap();
                extract_y_rgb(image::load_from_memory(&jpg).unwrap().into_rgb8().as_raw())
            };

            for (clabel, s) in [("s=1.00", 1.0f32), ("s=0.50", 0.5)] {
                let (nw, nh) = ((ow as f32 * s).round() as usize, (oh as f32 * s).round() as usize);
                let suspect = resample_y(&wm_jpeg_y, ow, oh, nw, nh);

                let blind = registration::decode_blind_auto(&suspect, nw, nh);
                let (matched, _) = decode_y_at_size_verbose(&suspect, nw, nh, ow, oh);
                let peaks = registration::scale_peaks(&suspect, nw, nh, 6);
                let expected_lag = ref_period * s; // true period in the suspect

                let (mut rank, mut nearest) = (0usize, f32::MAX);
                for (i, &(lag, _)) in peaks.iter().enumerate() {
                    let dd = (lag as f32 - expected_lag).abs();
                    if dd < nearest { nearest = dd; rank = i; }
                }
                let true_rank = if nearest <= 6.0 { format!("#{}", rank + 1) } else { "absent".to_string() };
                let top = peaks.first().copied().unwrap_or((0, 0.0));
                rows.push_str(&format!(
                    "| {:<11} | {:<7} | {:>6.3} | {:^5} | {:^7} | {:>5} | {:>6.3} | {:^7} |\n",
                    img_name, clabel, blind.scale,
                    if blind.verified { "✓" } else { "·" },
                    if matched.verified { "✓" } else { "·" },
                    top.0, top.0 as f32 / ref_period, true_rank));
                println!("{img_name} {clabel}: blind_scale={:.3} blind_crc={} matched_crc={} top_lag={} true≈{:.0} true_rank={}",
                    blind.scale, blind.verified, matched.verified, top.0, expected_lag, true_rank);
            }
        }
        let report = format!(
"# Phase 5b — blind-sync mechanism (white-seamless vs detail-rich)

At the q80 scales, for detail-rich test_a and white-seamless test_e: recovered **blind
scale**, whether **matched `--size`** decode still verifies (signal survived ⇒ a sync
problem, not loss), the **top autocorr peak** (lag and implied scale), and **where the
true tile period ranks** among the top peaks.

Regenerate: `cargo test -p glimr --features registration --release sync_mechanism -- --ignored --nocapture`

| image       | config  | blind scale | blind crc | matched crc | top lag | top scale | true rank |
|-------------|---------|-------------|-----------|-------------|---------|-----------|-----------|
{rows}
_`matched crc` ✓ while `blind crc` · and the true period low-ranked/absent ⇒ coarse-sync\
 failure (fixable by detail-aware block selection + harmonic candidates, Phases 6/7) —\
 not ECC or fine search._
");
        std::fs::write(reports.join("sync_mechanism.md"), report).unwrap();
    }

    // ── Phase-7 investigation: brute-force scale sweep on failing crops ────────
    // Bypass the autocorr scale *ranking*: try a fine grid of scales (each with
    // register_decode's own offset recovery), CRC/ECC-gated.  Distinguishes "the true
    // scale was never tried/ranked" (some scale verifies → fixable by better coarse
    // detection) from "signal too weak at any scale" (none verifies → fold-SNR floor).
    // Reads tests/failed_crops/*.jpg.
    //   cargo test -p glimr --features registration --release brute_scale_failed_crops -- --ignored --nocapture
    #[cfg(feature = "registration")]
    #[test]
    #[ignore]
    fn brute_scale_failed_crops() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .join("tests").join("failed_crops");
        if !dir.exists() { eprintln!("no tests/failed_crops dir — skipping"); return; }
        let reports = reports_dir();

        // Fine grid (0.2% step) over the plausible display-scale band.  0.2% < the
        // <0.25% matched notch, so the true scale should land in (or adjacent to) a cell.
        let scales: Vec<f32> = (0..=350).map(|i| 0.30 + i as f32 * 0.002).collect();

        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "jpg" || x == "jpeg").unwrap_or(false))
            .collect();
        files.sort();

        let mut rows = String::new();
        for path in &files {
            let img = image::open(path).unwrap().into_rgb8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            let y = extract_y_rgb(img.as_raw());
            let res = registration::scale_sweep(&y, w, h, &scales);
            let name = path.file_name().unwrap().to_string_lossy().to_string();

            let verified = res.last().filter(|r| r.2).copied();
            let best = res.iter().cloned()
                .fold((0.0f32, f32::MIN, false, 0u8), |b, r| if r.1 > b.1 { r } else { b });

            let s_rep = verified.map(|r| r.0).unwrap_or(best.0);
            let tw = ((w as f32 / s_rep).round() as usize).max(1024);
            let th = ((h as f32 / s_rep).round() as usize).max(1024);
            let tiles = (tw / 512) * (th / 512);

            if let Some((s, prom, _, errs)) = verified {
                rows.push_str(&format!("| {:<14} | {}×{} | **RECOVERED** | {:.3} | {:.1} | {} | {}×{} | {} |\n",
                    name, w, h, s, prom, errs, tw, th, tiles));
                println!("{name} ({w}×{h}): RECOVERED at scale {s:.3} — prominence {prom:.1}, ECC {errs}, rescaled {tw}×{th} ({tiles} fold-tiles)");
            } else {
                rows.push_str(&format!("| {:<14} | {}×{} | not recovered | {:.3} | {:.1} | — | {}×{} | {} |\n",
                    name, w, h, best.0, best.1, tw, th, tiles));
                println!("{name} ({w}×{h}): NOT recovered — best prominence {:.1} at scale {:.3} (rescaled {tw}×{th}, {tiles} fold-tiles)", best.1, best.0);
            }
        }

        let report = format!(
"# Phase-7 investigation — brute-force scale sweep on failing crops

For each `tests/failed_crops/*.jpg`, a fine CRC/ECC-gated scale sweep (0.30–1.00, 0.2%
step) via `register_decode` (its own offset recovery), bypassing the autocorr *ranking*.
**RECOVERED** = some scale CRC-verified ⇒ recoverable; the autocorr just couldn't rank/hit
it (→ better coarse detection). **not recovered** ⇒ no scale verified ⇒ a fold-SNR floor at
that crop size. `fold-tiles` = ⌊tw/512⌋·⌊th/512⌋ at the reported scale (more = stronger fold).

| crop | size | result | scale | prominence | ECC | rescaled | fold-tiles |
|------|------|--------|-------|-----------|-----|----------|------------|
{rows}
");
        std::fs::write(reports.join("brute_scale.md"), report).unwrap();
    }

    // ── Phase-7 investigation: where does the true period rank? ────────────────
    // For each failing crop, dump the top-12 autocorr peaks (lag → implied scale)
    // for the single centre block vs a full-extent multi-block sum, marking the peak
    // nearest the KNOWN true period (from brute_scale.md).  Tells us the cheap fix:
    //   • true period a low-ranked raw peak → widen K
    //   • true period ≈ ½×/⅓× of a strong peak (visible in the scale column) → harmonic siblings
    //   • absent even multi-block → keyed fallback ("hard one")
    //   cargo test -p glimr --features registration --release scale_peak_ranks -- --ignored --nocapture
    #[cfg(feature = "registration")]
    #[test]
    #[ignore]
    fn scale_peak_ranks() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .join("tests").join("failed_crops");
        if !dir.exists() { eprintln!("no tests/failed_crops dir — skipping"); return; }
        let reports = reports_dir();
        // Known true scales from the brute-force sweep (brute_scale.md).
        let truth: &[(&str, f32)] = &[
            ("sstest10.jpg", 0.422), ("sstest13.jpg", 0.506),
            ("sstest15.jpg", 0.560), ("sstest16.jpg", 0.558), ("original.jpg", 1.000),
        ];
        let refp = (4 * TILE_SIDE) as f32; // 256

        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "jpg" || x == "jpeg").unwrap_or(false))
            .collect();
        files.sort();

        let mut rows = String::new();
        for path in &files {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let img = image::open(path).unwrap().into_rgb8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            let y = extract_y_rgb(img.as_raw());
            let single = registration::scale_peaks(&y, w, h, 12);
            let multi  = registration::scale_peaks_multi(&y, w, h, 12);

            let ts = truth.iter().find(|(n, _)| *n == name).map(|(_, s)| *s);
            let true_lag = ts.map(|s| (s * refp).round() as i64);
            let near = |lag: usize| matches!(true_lag, Some(tl) if (lag as i64 - tl).abs() <= 3);
            let rank = |peaks: &[(usize, f32)]| -> String {
                match true_lag {
                    Some(_) => peaks.iter().position(|&(l, _)| near(l)).map(|i| format!("#{}", i + 1)).unwrap_or_else(|| "absent".into()),
                    None => "?".into(),
                }
            };
            let show = |peaks: &[(usize, f32)]| peaks.iter()
                .map(|&(l, _)| format!("{}{}→{:.3}", l, if near(l) { "★" } else { "" }, l as f32 / refp))
                .collect::<Vec<_>>().join("  ");

            let tss = ts.map(|s| format!("{:.3}", s)).unwrap_or_else(|| "?".into());
            println!("{name} ({w}×{h})  true≈{tss}");
            println!("  single [{}]: {}", rank(&single), show(&single));
            println!("  multi  [{}]: {}", rank(&multi), show(&multi));
            rows.push_str(&format!("| {:<14} | {} | {} | {} |\n", name, tss, rank(&single), rank(&multi)));
        }

        let report = format!(
"# Phase-7 — scale-peak ranking (single centre block vs multi-block sum)

Where the *true* tile period (lag = scale·256, from brute_scale.md) ranks among the top-12
whitened-autocorr peaks.  `absent` = not a top-12 peak.  The stdout dump shows each peak as
`lag→scale`; a ½×/⅓× harmonic relationship to a stronger peak is visible in the scale column
(e.g. a strong 1.01 with the true 0.51 = its half).

| crop | true scale | single-block rank | multi-block rank |
|------|-----------|-------------------|------------------|
{rows}
");
        std::fs::write(reports.join("scale_peak_ranks.md"), report).unwrap();
    }
}
