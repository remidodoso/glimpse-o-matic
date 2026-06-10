'use strict';

// Social Preview Maker — Phases 1–3a.
//   • Phase 1  — drop an image, reposition (drag) and resize (wheel) it inside the frame.
//   • Phase 2  — a logo overlay (chosen from a pool) you can drag and corner-resize.
//   • Phase 3a — one title text box: single-click to select (move/corner-resize),
//                double-click to edit (DOM textarea overlay), word-wrapped and clipped
//                to the box. Plain corner-drag resizes the box (text reflows); Shift +
//                corner-drag scales box AND font, aspect locked.
// Canvas-native, so what you see is exactly what's exported. Serve over HTTP so logo.png
// is same-origin (otherwise the canvas taints and Download fails).

const CARD_W = 1200;
const CARD_H = 630;
const GRAY = '#8c8c8c';      // must match --gray-stage so the letterbox blends into the stage
const ZOOM_OVER_FILL = 2;    // image zoom-in reaches at least 2× the "fill the frame" scale …
const ZOOM_OVER_NATIVE = 2;  // … and at least 2× native pixels, whichever is greater

const LOGOS = ['logo-white.png', 'logo-black.png']; // pool members (extend later)
const DEFAULT_LOGO = 'logo-white.png';
const LOGO_INIT_FRAC = 0.15; // initial logo width as a fraction of the card width
const LOGO_MARGIN = 40;      // inset of the logo from the card edges (card px)
const LOGO_MIN_W = 24;       // smallest the logo may be resized (card px)

const HANDLE_SIZE = 16;      // corner-handle square, drawn (card px)
const HANDLE_HIT = 14;       // half-width of the corner hit region (card px)

// Title text defaults.
const PLACEHOLDER = 'Click to edit';
// Curated font-family stacks: Windows name first, then Mac / Linux / Android substitutes.
const FONTS = [
    { label: 'Palatino',           stack: '"Palatino Linotype", "Book Antiqua", Palatino, "URW Palladio L", P052, "TeX Gyre Pagella", "Noto Serif", serif' },
    { label: 'Cambria',            stack: 'Cambria, Georgia, "PT Serif", "Noto Serif", "Liberation Serif", serif' },
    { label: 'Verdana',            stack: 'Verdana, Geneva, "DejaVu Sans", "Noto Sans", sans-serif' },
    { label: 'Georgia',            stack: 'Georgia, Gelasio, "Noto Serif", "PT Serif", "Times New Roman", serif' },
    { label: 'Gabriola',           stack: 'Gabriola, "Apple Chancery", "URW Chancery L", "TeX Gyre Chorus", cursive' },
    { label: 'Lucida Sans',        stack: '"Lucida Sans", "Lucida Sans Unicode", "Lucida Grande", "DejaVu Sans", "Noto Sans", sans-serif' },
    { label: 'Consolas',           stack: 'Consolas, Menlo, Monaco, "DejaVu Sans Mono", "Liberation Mono", "Noto Sans Mono", monospace' },
    { label: 'Monotype Corsiva',   stack: '"Monotype Corsiva", "Apple Chancery", "URW Chancery L", "TeX Gyre Chorus", cursive' },
    { label: 'Lucida Calligraphy', stack: '"Lucida Calligraphy", "Apple Chancery", "URW Chancery L", cursive' },
];
const TEXT_COLOR = '#ffffff';
const TEXT_ALIGN = 'left';
const TEXT_LINE_HEIGHT = 1.2;              // × font size
const TEXT_PAD = 14;                       // inner padding (card px)
const TEXT_MARGIN = 40;                    // initial inset of the box from the card's top-left (card px)
const TEXT_DEFAULT_PX = 36;                // initial font size (output px)
const TEXT_SHADOW = { color: 'rgba(0,0,0,0.55)', blur: 14, dx: 0, dy: 3 };
const TEXT_MIN_W = 60, TEXT_MIN_H = 30, TEXT_MIN_FONT = 10, TEXT_MAX_FONT = 300;

const LOGO_SHADOW = { color: 'rgba(0,0,0,0.5)', blur: 18, dx: 0, dy: 6 };

const canvas = document.getElementById('card');
const ctx = canvas.getContext('2d');
const stage = document.getElementById('stage');

// Layers.  x,y are top-left in card coordinates.
let img = null;         // background photo: { bitmap, w, h, scale, coverScale, x, y }
let logo = null;        // logo overlay:     { bitmap, w, h, scale, x, y }
let text = null;        // title box:        { str, x, y, w, h, fontPx, fontFamily, weight, color, align, lineHeightMul }

let logoActive = false; // logo handles shown?
let textActive = false; // text handles shown?
let textEditing = false;// textarea overlay open?
let exporting = false;  // true only during a Download render (suppresses screen-only placeholders)

// ── background-image geometry / constraints ─────────────────────────────────

function coverScaleFor(w, h) { return Math.max(CARD_W / w, CARD_H / h); }

function clampScale(s) {
    if (!img) return s;
    const maxS = Math.max(img.coverScale * ZOOM_OVER_FILL, ZOOM_OVER_NATIVE);
    return Math.min(Math.max(s, img.coverScale), maxS);
}

function clampImgPos() {
    if (!img) return;
    const dw = img.w * img.scale, dh = img.h * img.scale;
    img.x = Math.min(0, Math.max(CARD_W - dw, img.x));
    img.y = Math.min(0, Math.max(CARD_H - dh, img.y));
}

// ── shared box helpers (logo + text) ────────────────────────────────────────

function boxCorners(x, y, w, h) {
    return [
        { id: 'tl', x: x,     y: y },
        { id: 'tr', x: x + w, y: y },
        { id: 'br', x: x + w, y: y + h },
        { id: 'bl', x: x,     y: y + h },
    ];
}

function handleAt(corners, p) {
    for (let i = 0; i < corners.length; i++) {
        const c = corners[i];
        if (Math.abs(p.x - c.x) <= HANDLE_HIT && Math.abs(p.y - c.y) <= HANDLE_HIT) return c.id;
    }
    return null;
}

function oppositeCorner(corners, id) {
    const opp = { tl: 'br', tr: 'bl', br: 'tl', bl: 'tr' }[id];
    return corners.find(function (c) { return c.id === opp; });
}

function cursorForHandle(id) {
    return (id === 'tl' || id === 'br') ? 'nwse-resize' : 'nesw-resize';
}

// ── logo geometry ────────────────────────────────────────────────────────────

function logoBox() {
    if (!logo) return null;
    const w = logo.w * logo.scale, h = logo.h * logo.scale;
    return { x: logo.x, y: logo.y, w: w, h: h, corners: boxCorners(logo.x, logo.y, w, h) };
}

function clampLogoPos() {
    if (!logo) return;
    const dw = logo.w * logo.scale, dh = logo.h * logo.scale;
    logo.x = Math.min(Math.max(0, logo.x), CARD_W - dw);
    logo.y = Math.min(Math.max(0, logo.y), CARD_H - dh);
}

function logoMaxScale() { return Math.min(CARD_W / logo.w, CARD_H / logo.h); }

function hitLogo(p) {
    const b = logoBox();
    return b && p.x >= b.x && p.x <= b.x + b.w && p.y >= b.y && p.y <= b.y + b.h;
}

// ── text geometry ──────────────────────────────────────────────────────────

function hitTextBox(p) {
    return text && p.x >= text.x && p.x <= text.x + text.w && p.y >= text.y && p.y <= text.y + text.h;
}

function clampTextPos() {
    if (!text) return;
    text.x = Math.min(Math.max(0, text.x), Math.max(0, CARD_W - text.w));
    text.y = Math.min(Math.max(0, text.y), Math.max(0, CARD_H - text.h));
}

// Greedy word-wrap to maxW, honoring explicit newlines. Assumes ctx.font is set.
function wrapText(str, maxW) {
    const lines = [];
    str.split('\n').forEach(function (para) {
        const words = para.split(/\s+/).filter(function (w) { return w.length; });
        if (!words.length) { lines.push(''); return; }
        let line = words[0];
        for (let i = 1; i < words.length; i++) {
            const test = line + ' ' + words[i];
            if (ctx.measureText(test).width <= maxW) line = test;
            else { lines.push(line); line = words[i]; }
        }
        lines.push(line);
    });
    return lines;
}

// ── rendering ────────────────────────────────────────────────────────────────
// drawContent() is exactly what gets exported (placeholders suppressed). drawChrome()
// is screen-only editing UI (selection box + handles).

function drawContent() {
    ctx.clearRect(0, 0, CARD_W, CARD_H);
    ctx.fillStyle = GRAY;
    ctx.fillRect(0, 0, CARD_W, CARD_H);

    if (img) {
        ctx.drawImage(img.bitmap, img.x, img.y, img.w * img.scale, img.h * img.scale);
    } else if (!exporting) {
        ctx.fillStyle = 'rgba(255, 255, 255, 0.7)';
        ctx.font = '600 36px system-ui, sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText('Drag an image here', CARD_W / 2, CARD_H / 2);
    }

    if (logo) {
        ctx.save();
        ctx.shadowColor = LOGO_SHADOW.color;
        ctx.shadowBlur = LOGO_SHADOW.blur;
        ctx.shadowOffsetX = LOGO_SHADOW.dx;
        ctx.shadowOffsetY = LOGO_SHADOW.dy;
        ctx.drawImage(logo.bitmap, logo.x, logo.y, logo.w * logo.scale, logo.h * logo.scale);
        ctx.restore();
    }

    drawText();
}

// Canvas font shorthand: [italic] <weight> <px> <family-stack>.
function fontString(t) {
    return (t.italic ? 'italic ' : '') + (t.bold ? '700' : '400') + ' ' + t.fontPx + 'px ' + t.fontFamily;
}

function drawText() {
    if (!text || textEditing) return;            // while editing, the overlay shows the text
    const empty = text.str.trim() === '';
    if (empty && exporting) return;              // the placeholder is never exported

    ctx.save();
    ctx.beginPath();
    ctx.rect(text.x, text.y, text.w, text.h);
    ctx.clip();                                  // overflow past the box is clipped
    ctx.font = fontString(text);
    ctx.textBaseline = 'top';
    ctx.textAlign = text.align;
    ctx.shadowColor = TEXT_SHADOW.color;
    ctx.shadowBlur = TEXT_SHADOW.blur;
    ctx.shadowOffsetX = TEXT_SHADOW.dx;
    ctx.shadowOffsetY = TEXT_SHADOW.dy;
    ctx.fillStyle = empty ? 'rgba(255,255,255,0.5)' : text.color;

    const content = empty ? PLACEHOLDER : text.str;
    const lines = wrapText(content, text.w - 2 * TEXT_PAD);
    const lh = text.fontPx * text.lineHeightMul;
    const tx = text.align === 'center' ? text.x + text.w / 2
             : text.align === 'right' ? text.x + text.w - TEXT_PAD
             : text.x + TEXT_PAD;
    let ty = text.y + TEXT_PAD;
    lines.forEach(function (ln) { ctx.fillText(ln, tx, ty); ty += lh; });
    ctx.restore();
}

function drawBoxChrome(x, y, w, h) {
    ctx.save();
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.9)';
    ctx.lineWidth = 1.5;
    ctx.setLineDash([6, 4]);
    ctx.strokeRect(x, y, w, h);
    ctx.setLineDash([]);
    boxCorners(x, y, w, h).forEach(function (c) {
        ctx.fillStyle = 'white';
        ctx.strokeStyle = 'rgba(0, 0, 0, 0.6)';
        ctx.lineWidth = 1;
        ctx.fillRect(c.x - HANDLE_SIZE / 2, c.y - HANDLE_SIZE / 2, HANDLE_SIZE, HANDLE_SIZE);
        ctx.strokeRect(c.x - HANDLE_SIZE / 2, c.y - HANDLE_SIZE / 2, HANDLE_SIZE, HANDLE_SIZE);
    });
    ctx.restore();
}

function drawChrome() {
    if (logoActive && logo) { const b = logoBox(); drawBoxChrome(b.x, b.y, b.w, b.h); }
    if (textActive && text && !textEditing) drawBoxChrome(text.x, text.y, text.w, text.h);
}

function render() {
    drawContent();
    drawChrome();
}

// ── selection (one object at a time) ──────────────────────────────────────────

function setLogoActive(v) { if (logoActive !== v) { logoActive = v; render(); } }
function setTextActive(v) { if (textActive !== v) { textActive = v; render(); } }
function selectLogo() { setTextActive(false); setLogoActive(true); }
function selectText() { setLogoActive(false); setTextActive(true); syncAlignButtons(); syncColorInput(); syncSizeInput(); syncFontSelect(); syncStyleButtons(); }
function selectNone() { setLogoActive(false); setTextActive(false); }

// ── loading ──────────────────────────────────────────────────────────────────

function loadFile(file) {
    if (!file || !file.type.startsWith('image/')) return;
    createImageBitmap(file).then(function (bmp) {
        const cs = coverScaleFor(bmp.width, bmp.height);
        img = { bitmap: bmp, w: bmp.width, h: bmp.height, scale: cs, coverScale: cs, x: 0, y: 0 };
        img.x = (CARD_W - img.w * img.scale) / 2;
        img.y = (CARD_H - img.h * img.scale) / 2;
        clampImgPos();
        render();
    });
}

function placeLogo(imgEl) {
    const w = imgEl.naturalWidth, h = imgEl.naturalHeight;
    if (!w || !h) return;
    const scale = (CARD_W * LOGO_INIT_FRAC) / w;  // per-logo, so unequal sizes land at the same width
    logo = { bitmap: imgEl, w: w, h: h, scale: scale, x: LOGO_MARGIN, y: CARD_H - h * scale - LOGO_MARGIN };
    render();
}

function whenReady(imgEl, fn) {
    if (imgEl.complete && imgEl.naturalWidth) fn();
    else imgEl.addEventListener('load', fn, { once: true });
}

(function buildLogoPool() {
    const pool = document.getElementById('logo-pool');
    LOGOS.forEach(function (src) {
        const sw = document.createElement('img');
        sw.className = 'logo-swatch';
        sw.src = src;
        sw.alt = src;
        sw.draggable = false;

        sw.addEventListener('click', function () {
            if (sw.classList.contains('selected')) { selectLogo(); return; } // reveal handles only
            const prev = pool.querySelector('.logo-swatch.selected');
            if (prev) prev.classList.remove('selected');
            sw.classList.add('selected');
            whenReady(sw, function () { placeLogo(sw); selectLogo(); });
        });

        if (src === DEFAULT_LOGO) {
            sw.classList.add('selected');
            whenReady(sw, function () { placeLogo(sw); }); // placed, but not selected (handles hidden)
        }
        pool.appendChild(sw);
    });
})();

// Default title box: centered, ~70% width, x-height = 1/20 card height.
(function initText() {
    const fontPx = TEXT_DEFAULT_PX;
    const w = CARD_W * 0.7;
    const h = Math.round(fontPx * TEXT_LINE_HEIGHT * 2 + TEXT_PAD * 2); // room for ~2 lines
    text = {
        str: '', x: TEXT_MARGIN, y: TEXT_MARGIN, w: w, h: h,
        fontPx: fontPx, fontFamily: FONTS[0].stack, bold: false, italic: false,
        color: TEXT_COLOR, align: TEXT_ALIGN, lineHeightMul: TEXT_LINE_HEIGHT,
    };
    render();
})();

// ── text editor overlay ────────────────────────────────────────────────────

let textareaEl = null;

function ensureTextarea() {
    if (textareaEl) return textareaEl;
    const ta = document.createElement('textarea');
    ta.id = 'text-editor';
    ta.placeholder = PLACEHOLDER;
    ta.spellcheck = false;
    document.body.appendChild(ta);
    ta.addEventListener('blur', commitEdit);
    ta.addEventListener('keydown', function (e) {
        if (e.key === 'Escape') { e.preventDefault(); ta.blur(); }                    // Esc commits
        else if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); ta.blur(); } // Enter commits; Shift+Enter = newline
    });
    textareaEl = ta;
    return ta;
}

function enterEdit() {
    if (!text) return;
    textEditing = true;
    const r = canvas.getBoundingClientRect();
    const sx = r.width / canvas.width;   // CSS px per card px (aspect preserved → sx ≈ sy)
    const ta = ensureTextarea();
    Object.assign(ta.style, {
        display: 'block',
        left: (r.left + text.x * sx) + 'px',
        top: (r.top + text.y * sx) + 'px',
        width: (text.w * sx) + 'px',
        height: (text.h * sx) + 'px',
        fontFamily: text.fontFamily,
        fontWeight: text.bold ? '700' : '400',
        fontStyle: text.italic ? 'italic' : 'normal',
        fontSize: (text.fontPx * sx) + 'px',
        lineHeight: String(text.lineHeightMul),
        color: text.color,
        textAlign: text.align,
        padding: (TEXT_PAD * sx) + 'px',
        textShadow: (TEXT_SHADOW.dx * sx) + 'px ' + (TEXT_SHADOW.dy * sx) + 'px ' +
                    (TEXT_SHADOW.blur * sx) + 'px ' + TEXT_SHADOW.color,
    });
    ta.value = text.str;
    render();           // canvas hides the text (drawText returns while editing)
    ta.focus();
    ta.setSelectionRange(ta.value.length, ta.value.length);
}

function commitEdit() {
    if (!textEditing) return;
    text.str = textareaEl.value;
    textEditing = false;
    textareaEl.style.display = 'none';
    render();
}

// ── mouse → card coordinate mapping (card is displayed scaled-to-fit) ──────────

function toCard(e) {
    const r = canvas.getBoundingClientRect();
    return {
        x: (e.clientX - r.left) * (canvas.width / r.width),
        y: (e.clientY - r.top) * (canvas.height / r.height),
    };
}

// ── interaction state machine ──────────────────────────────────────────────
//   'pan' | 'move-logo' | 'resize-logo' | 'move-text' | 'resize-text'

let mode = null;
let lastX = 0, lastY = 0;          // previous cursor (pan / move)
let resizeAnchor = null;           // logo: fixed opposite corner
let textResizeAnchor = null;       // text: fixed opposite corner
let textResizeStart = null;        // text: { w, h, fontPx } at resize start (for Shift scaling)

canvas.addEventListener('mousedown', function (e) {
    if (textEditing) commitEdit();             // a click on the canvas (outside the editor) commits
    const p = toCard(e);

    // Text is topmost.
    if (textActive) {
        const h = handleAt(boxCorners(text.x, text.y, text.w, text.h), p);
        if (h) {
            mode = 'resize-text';
            const oc = oppositeCorner(boxCorners(text.x, text.y, text.w, text.h), h);
            textResizeAnchor = { x: oc.x, y: oc.y };
            textResizeStart = { w: text.w, h: text.h, fontPx: text.fontPx };
            canvas.style.cursor = cursorForHandle(h);
            return;
        }
    }
    if (hitTextBox(p)) {
        selectText();
        mode = 'move-text'; lastX = p.x; lastY = p.y; canvas.style.cursor = 'move';
        return;
    }

    // Then the logo.
    if (logoActive) {
        const h = handleAt(logoBox().corners, p);
        if (h) {
            mode = 'resize-logo';
            const oc = oppositeCorner(logoBox().corners, h);
            resizeAnchor = { x: oc.x, y: oc.y };
            canvas.style.cursor = cursorForHandle(h);
            return;
        }
    }
    if (hitLogo(p)) {
        selectLogo();
        mode = 'move-logo'; lastX = p.x; lastY = p.y; canvas.style.cursor = 'move';
        return;
    }

    // Otherwise the photo.
    selectNone();
    if (img) { mode = 'pan'; lastX = p.x; lastY = p.y; canvas.style.cursor = 'grabbing'; }
});

window.addEventListener('mousemove', function (e) {
    if (!mode) return;
    const p = toCard(e);

    if (mode === 'pan') {
        img.x += p.x - lastX; img.y += p.y - lastY; lastX = p.x; lastY = p.y;
        clampImgPos();

    } else if (mode === 'move-logo') {
        logo.x += p.x - lastX; logo.y += p.y - lastY; lastX = p.x; lastY = p.y;
        clampLogoPos();

    } else if (mode === 'resize-logo') {
        const ax = resizeAnchor.x, ay = resizeAnchor.y;
        let s = Math.max(Math.abs(p.x - ax) / logo.w, Math.abs(p.y - ay) / logo.h);
        s = Math.min(Math.max(s, LOGO_MIN_W / logo.w), logoMaxScale());
        logo.scale = s;
        const dw = logo.w * s, dh = logo.h * s;
        logo.x = (p.x >= ax) ? ax : ax - dw;
        logo.y = (p.y >= ay) ? ay : ay - dh;
        clampLogoPos();

    } else if (mode === 'move-text') {
        text.x += p.x - lastX; text.y += p.y - lastY; lastX = p.x; lastY = p.y;
        clampTextPos();

    } else if (mode === 'resize-text') {
        const ax = textResizeAnchor.x, ay = textResizeAnchor.y;
        if (e.shiftKey) {
            // Uniform: scale box AND font together, aspect locked to the original box.
            let s = Math.max(Math.abs(p.x - ax) / textResizeStart.w, Math.abs(p.y - ay) / textResizeStart.h);
            s = Math.max(s, TEXT_MIN_FONT / textResizeStart.fontPx, TEXT_MIN_W / textResizeStart.w);
            text.w = textResizeStart.w * s;
            text.h = textResizeStart.h * s;
            text.fontPx = textResizeStart.fontPx * s;
            syncSizeInput();
        } else {
            // Box only: width/height follow the cursor; font fixed → text reflows + clips.
            text.w = Math.max(TEXT_MIN_W, Math.abs(p.x - ax));
            text.h = Math.max(TEXT_MIN_H, Math.abs(p.y - ay));
        }
        text.x = (p.x >= ax) ? ax : ax - text.w;
        text.y = (p.y >= ay) ? ay : ay - text.h;
        clampTextPos();
    }
    render();
});

window.addEventListener('mouseup', function () {
    mode = null;
    resizeAnchor = null;
    textResizeAnchor = null;
    textResizeStart = null;
    canvas.style.cursor = '';
});

canvas.addEventListener('dblclick', function (e) {
    const p = toCard(e);
    if (hitTextBox(p)) { selectText(); enterEdit(); }
});

// Hover cursor (only when not mid-gesture).
canvas.addEventListener('mousemove', function (e) {
    if (mode) return;
    const p = toCard(e);
    if (textActive && handleAt(boxCorners(text.x, text.y, text.w, text.h), p)) {
        canvas.style.cursor = cursorForHandle(handleAt(boxCorners(text.x, text.y, text.w, text.h), p));
    } else if (hitTextBox(p)) {
        canvas.style.cursor = 'move';
    } else if (logoActive && handleAt(logoBox().corners, p)) {
        canvas.style.cursor = cursorForHandle(handleAt(logoBox().corners, p));
    } else if (hitLogo(p)) {
        canvas.style.cursor = 'move';
    } else {
        canvas.style.cursor = img ? 'grab' : 'default';
    }
});

// A mousedown that isn't the canvas, a swatch, or the editor commits any edit and
// deselects. (The canvas and swatches manage their own selection.)
document.addEventListener('mousedown', function (e) {
    const t = e.target;
    if (t === textareaEl) return;
    if (textEditing) commitEdit();
    if (t === canvas) return;
    if (t && t.closest && t.closest('#text-toolbar')) return;   // align controls keep the text selected
    if (t && t.classList && t.classList.contains('logo-swatch')) return;
    selectNone();
});

// ── resize (wheel) zooms the background image, toward the cursor ───────────────

canvas.addEventListener('wheel', function (e) {
    if (!img) return;
    e.preventDefault();
    const p = toCard(e);
    const ix = (p.x - img.x) / img.scale;
    const iy = (p.y - img.y) / img.scale;
    const ns = clampScale(img.scale * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
    img.scale = ns;
    img.x = p.x - ix * ns;
    img.y = p.y - iy * ns;
    clampImgPos();
    render();
}, { passive: false });

// ── drag & drop ────────────────────────────────────────────────────────────────

['dragenter', 'dragover'].forEach(function (ev) {
    stage.addEventListener(ev, function (e) { e.preventDefault(); stage.classList.add('dropping'); });
});

['dragleave', 'dragend'].forEach(function (ev) {
    stage.addEventListener(ev, function (e) {
        if (ev === 'dragleave' && stage.contains(e.relatedTarget)) return;
        stage.classList.remove('dropping');
    });
});

stage.addEventListener('drop', function (e) {
    e.preventDefault();
    stage.classList.remove('dropping');
    const files = e.dataTransfer && e.dataTransfer.files;
    if (files && files.length) loadFile(files[0]);
});

window.addEventListener('dragover', function (e) { e.preventDefault(); });
window.addEventListener('drop', function (e) { e.preventDefault(); });

// ── text settings toolbar (justification; more controls added one at a time) ───

function syncAlignButtons() {
    document.querySelectorAll('.align-btn').forEach(function (b) {
        b.classList.toggle('active', !!text && b.dataset.align === text.align);
    });
}

function setTextAlign(align) {
    if (!text) return;
    text.align = align;
    if (textEditing && textareaEl) textareaEl.style.textAlign = align;
    syncAlignButtons();
    render();
}

document.querySelectorAll('.align-btn').forEach(function (b) {
    b.addEventListener('click', function () { setTextAlign(b.dataset.align); });
});
syncAlignButtons();

const textColorInput = document.getElementById('text-color');

function syncColorInput() {
    if (textColorInput && text) textColorInput.value = text.color;
}

function setTextColor(c) {
    if (!text) return;
    text.color = c;
    if (textEditing && textareaEl) textareaEl.style.color = c;
    render();
}

textColorInput.addEventListener('input', function () { setTextColor(textColorInput.value); });
syncColorInput();

const textSizeInput = document.getElementById('text-size');

// 1-decimal display; drop the decimal when the size is a whole number ("42", "54.6").
function formatSize(px) {
    const r = Math.round(px * 10) / 10;
    return Number.isInteger(r) ? String(r) : r.toFixed(1);
}

function syncSizeInput() {
    if (textSizeInput && text) textSizeInput.value = formatSize(text.fontPx);
}

// Set font size (font only — the box is unchanged; text reflows/clips inside it).
function applyFontSize(px, normalizeField) {
    if (!text || isNaN(px)) return;
    let v = Math.round(px * 10) / 10;                                   // round to 1 decimal on accept
    v = Math.min(Math.max(v, TEXT_MIN_FONT), TEXT_MAX_FONT);
    text.fontPx = v;
    if (textEditing && textareaEl) {
        const sx = canvas.getBoundingClientRect().width / canvas.width;
        textareaEl.style.fontSize = (v * sx) + 'px';
    }
    if (normalizeField) syncSizeInput();   // reformat only on commit, so typing isn't disrupted
    render();
}

textSizeInput.addEventListener('input', function () { applyFontSize(parseFloat(textSizeInput.value), false); });
textSizeInput.addEventListener('change', function () { applyFontSize(parseFloat(textSizeInput.value), true); });

function stepFontSize(delta) { if (text) applyFontSize(text.fontPx + delta, true); }
document.getElementById('size-dec').addEventListener('click', function () { stepFontSize(-1); });
document.getElementById('size-inc').addEventListener('click', function () { stepFontSize(1); });
syncSizeInput();

const textFontSelect = document.getElementById('text-font');
FONTS.forEach(function (f) {
    const opt = document.createElement('option');
    opt.value = f.stack;
    opt.textContent = f.label;
    opt.style.fontFamily = f.stack;   // preview each option in its own face
    textFontSelect.appendChild(opt);
});

function syncFontSelect() {
    if (textFontSelect && text) textFontSelect.value = text.fontFamily;
}

function setTextFont(stack) {
    if (!text) return;
    text.fontFamily = stack;
    if (textEditing && textareaEl) textareaEl.style.fontFamily = stack;
    render();
}

textFontSelect.addEventListener('change', function () { setTextFont(textFontSelect.value); });
syncFontSelect();

const btnBold = document.getElementById('btn-bold');
const btnItalic = document.getElementById('btn-italic');

function syncStyleButtons() {
    if (btnBold) btnBold.classList.toggle('active', !!text && text.bold);
    if (btnItalic) btnItalic.classList.toggle('active', !!text && text.italic);
}

function applyTextStyle() {
    if (textEditing && textareaEl) {
        textareaEl.style.fontWeight = text.bold ? '700' : '400';
        textareaEl.style.fontStyle = text.italic ? 'italic' : 'normal';
    }
    syncStyleButtons();
    render();
}

btnBold.addEventListener('click', function () { if (text) { text.bold = !text.bold; applyTextStyle(); } });
btnItalic.addEventListener('click', function () { if (text) { text.italic = !text.italic; applyTextStyle(); } });
syncStyleButtons();

// ── download — content only, placeholders and chrome suppressed ────────────────

document.getElementById('btn-download').addEventListener('click', function () {
    if (textEditing) commitEdit();
    exporting = true;
    drawContent();
    // JPEG, not PNG: small + correctly typed for social-card crawlers (e.g. Signal honors
    // Content-Type and caps image size). The card is fully opaque (gray fill), so no alpha lost.
    canvas.toBlob(function (blob) {
        if (blob) {
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = 'social_preview.jpg';
            document.body.appendChild(a);
            a.click();
            document.body.removeChild(a);
            setTimeout(function () { URL.revokeObjectURL(url); }, 1000);
        }
        exporting = false;
        render();
    }, 'image/jpeg', 0.92);
});

render();
