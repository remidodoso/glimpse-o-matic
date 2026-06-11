var info_visible  = false;
var about_visible = false;
var about_html_cache = null;

// ---------------------------------------------------------------------------
// Logging — [HH:MM:SS.SSS] <func> msg  (mirrors the Rust glog format)
// ---------------------------------------------------------------------------
function glimr_log(func, msg) {
    var d = new Date();
    var hh = String(d.getHours()).padStart(2, '0');
    var mm = String(d.getMinutes()).padStart(2, '0');
    var ss = String(d.getSeconds()).padStart(2, '0');
    var ms = String(d.getMilliseconds()).padStart(3, '0');
    console.log('[' + hh + ':' + mm + ':' + ss + '.' + ms + '] <' + func + '> ' + msg);
}

const G_CAROUSEL_SIZE_MAX = 160.0;
var landscape_mq = window.matchMedia('(orientation: landscape)');

var thumbs = [];
var current_index = null;
var thumb_gen      = 0;     // incremented on each load; cancels stale thumbnail fills
var load_gen       = 0;     // incremented on each load; cancels stale extraction rAF loops
var stream_loading = false; // true while a zip stream is in progress

// Slide drag state
var is_dragging = false;
var drag_start_x = 0;
var drag_start_y = 0;
var drag_offset = 0;
var drag_moved = false;
var animation_id = null;

// Carousel drag state
var carousel_is_dragging = false;
var carousel_drag_start_x = 0;
var carousel_drag_start_y = 0;
var carousel_scroll_start = 0;
var carousel_drag_moved = false;

// Carousel kinetic-scroll ("throw") state
var carousel_pointer_id   = null;  // pointer captured for the active drag
var carousel_vel          = 0;     // smoothed pointer velocity, active axis (px/ms)
var carousel_last_pos     = 0;     // last pointer coord (active axis)
var carousel_last_t       = 0;     // last pointer-move timestamp (ms)
var carousel_inertia_raf  = 0;     // momentum rAF handle (0 = idle)
var carousel_captured     = false; // did we setPointerCapture for this drag? (captured lazily)
var CAROUSEL_FRICTION_TAU = 650;   // ms; larger = glidier — decays to a stop, never infinite
var CAROUSEL_MIN_THROW_V  = 0.05;  // px/ms; below this, a release doesn't throw
var CAROUSEL_STOP_V       = 0.015; // px/ms; momentum ends below this
var CAROUSEL_IDLE_MS      = 60;    // held still this long before release ⇒ no throw

// Hover indicator state
var hover_zone = null;
var hover_opacity = 0.0;
var hover_target = 0.0;
var hover_anim_id = null;
var hover_anim_from = 0.0;
var hover_anim_start = 0;
var hover_idle_timer = null;
var current_draw_offset = 0;

// ---------------------------------------------------------------------------
// Watermarking — browser fingerprint + payload assembly
// ---------------------------------------------------------------------------

function fnv32a(str) {
    var h = 0x811c9dc5;
    for (var i = 0; i < str.length; i++) {
        h ^= str.charCodeAt(i);
        h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h >>> 0;
}

// Computed once at page load — stable for all images in a session.
var g_browser_fp = (function() {
    try {
        var tz = '';
        try { tz = Intl.DateTimeFormat().resolvedOptions().timeZone; } catch(e) {}
        return fnv32a([
            navigator.userAgent                                      || '',
            screen.width + 'x' + screen.height + '@' + (screen.colorDepth || 0),
            navigator.language                                       || '',
            tz,
            String(navigator.hardwareConcurrency || 0),
            String(navigator.deviceMemory        || 0),
        ].join('|'));
    } catch(e) { return 0; }
})();

var g_referrer_hash = (function() {
    try {
        var ref = document.referrer;
        if (!ref) return 0;
        return fnv32a(new URL(ref).hostname) & 0xFFFF;
    } catch(e) { return 0; }
})();

// Assembles the 16-byte watermark payload.  Called per decode_image so the
// timestamp reflects when the image was actually decoded and cached.
function build_payload() {
    var buf  = new ArrayBuffer(16);
    var view = new DataView(buf);
    view.setUint32( 0, (Date.now() / 1000) >>> 0,  true); // Unix timestamp (u32 LE)
    view.setUint32( 4, 0,                           true); // IPv4 — deferred
    view.setUint32( 8, g_browser_fp,                true); // browser fingerprint (u32 LE)
    view.setUint16(12, g_referrer_hash & 0xFFFF,    true); // referrer hash (u16 LE)
    view.setUint8 (14, g_referrer_hash !== 0 ? 1 : 0);    // flags: bit 0 = has referrer
    view.setUint8 (15, 1);                                 // version = 1
    return new Uint8Array(buf);
}

// Zoom state
var zoom_mode  = false;
var zoom_scale = 1.0;
var zoom_pan_x = 0;
var zoom_pan_y = 0;

const ZOOM_MAX      = 2.0;
const ZOOM_KEY_STEP = 1.25;
const ARROW_PAN_PX  = 80;

// Pinch state
var pinch_active      = false;
var pinch_start_dist  = 0;
var pinch_start_scale = 1.0;
var pinch_start_pan_x = 0;
var pinch_start_pan_y = 0;
var pinch_mid_x       = 0;
var pinch_mid_y       = 0;

// ---------------------------------------------------------------------------
// Button actions
// ---------------------------------------------------------------------------

function format_size(bytes) {
    if (bytes >= 1048576) return (bytes / 1048576).toFixed(1) + ' MB';
    if (bytes >= 1024)    return (bytes / 1024).toFixed(1) + ' KB';
    return bytes + ' B';
}

function show_info() {
    if (current_index === null) return;
    var name = renderer.image_name(current_index).replace(/\.dat$/i, '.jpg');
    var img_w = renderer.image_width(current_index);
    var img_h = renderer.image_height(current_index);
    var size  = renderer.image_file_size(current_index);

    document.getElementById('info-filename').textContent = name;

    var content = document.getElementById('info-content');
    content.innerHTML = '';
    [img_w && img_h ? img_w + ' × ' + img_h : null,
     format_size(size)
    ].forEach(function(text) {
        if (!text) return;
        var p = document.createElement('p');
        p.textContent = text;
        content.appendChild(p);
    });

    document.getElementById('info-overlay').style.display = 'flex';
    info_visible = true;
}

function hide_info() {
    document.getElementById('info-overlay').style.display = 'none';
    info_visible = false;
}

function show_about() {
    var content = document.getElementById('about-content');
    if (about_html_cache !== null) {
        content.innerHTML = about_html_cache;
        document.getElementById('about-overlay').style.display = 'flex';
        about_visible = true;
        return;
    }
    fetch('about.html').then(function(r) {
        if (!r.ok) throw new Error('HTTP ' + r.status);
        return r.text();
    }).then(function(html) {
        about_html_cache = html;
        content.innerHTML = html;
        document.getElementById('about-overlay').style.display = 'flex';
        about_visible = true;
    }).catch(function() {
        about_html_cache = '<p style="color:#555;font-size:1.1em;">No about information available.</p>';
        content.innerHTML = about_html_cache;
        document.getElementById('about-overlay').style.display = 'flex';
        about_visible = true;
    });
}

function hide_about() {
    document.getElementById('about-overlay').style.display = 'none';
    about_visible = false;
}

function flash_button(btn, entering) {
    var cls = entering ? 'tapped-active' : 'tapped';
    btn.classList.remove('tapped', 'tapped-active');
    void btn.offsetWidth;
    btn.classList.add(cls);
    btn.addEventListener('animationend', function handler() {
        btn.classList.remove('tapped', 'tapped-active');
        btn.removeEventListener('animationend', handler);
    });
}

function toggle_fullscreen() {
    if (document.fullscreenElement) {
        document.exitFullscreen();
    } else {
        document.documentElement.requestFullscreen();
    }
}

// JPEG quality for downloads (browser-native encoder). High quality; the
// luminance watermark survives re-encoding comfortably at this level.
var G_DOWNLOAD_JPEG_QUALITY = 0.92;

// Download the *watermarked* current image as a high-quality JPEG.  The displayed
// pixels are already watermarked and cached in WASM; we pull them at native
// resolution, encode JPEG in-browser, and save.  The un-watermarked source bytes
// are never exported.
function download_current() {
    if (current_index === null) return;
    var index = current_index;
    // Ensure the watermarked pixels exist (instant if already displayed/cached).
    decode_image(index, function() { export_watermarked_jpeg(index); });
}

// Strip any extension and force .jpg (download is always JPEG now).
function jpeg_name(index) {
    return renderer.image_name(index).replace(/\.[^./\\]+$/, '') + '.jpg';
}

function export_watermarked_jpeg(index) {
    var w = renderer.image_width(index);
    var h = renderer.image_height(index);
    var rgba = renderer.watermarked_pixels(index);
    if (!w || !h || rgba.length !== w * h * 4) {
        glimr_log('download_current', 'no watermarked pixels for index ' + index);
        return;
    }
    var oc  = new OffscreenCanvas(w, h);
    var ctx = oc.getContext('2d');
    // Zero-copy view of the WASM-returned buffer as clamped RGBA.
    var clamped = new Uint8ClampedArray(rgba.buffer, rgba.byteOffset, rgba.length);
    ctx.putImageData(new ImageData(clamped, w, h), 0, 0);

    oc.convertToBlob({ type: 'image/jpeg', quality: G_DOWNLOAD_JPEG_QUALITY })
        .then(function(blob) { save_blob(blob, jpeg_name(index)); })
        .catch(function(e) { glimr_log('download_current', 'encode error: ' + e); });
}

function save_blob(blob, name) {
    if (!window.showSaveFilePicker) {
        if (navigator.maxTouchPoints === 0 && !window.confirm(name + '\n' + format_size(blob.size))) return;
        var url = URL.createObjectURL(blob);
        var a = document.createElement('a');
        a.href = url;
        a.download = name;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        return;
    }

    window.showSaveFilePicker({
        suggestedName: name,
        types: [{ description: 'JPEG Image', accept: {'image/jpeg': ['.jpg', '.jpeg']} }]
    }).then(function(handle) {
        return handle.createWritable();
    }).then(function(writable) {
        return writable.write(blob).then(function() { return writable.close(); });
    }).catch(function(e) {
        if (e.name !== 'AbortError') glimr_log('download_current', 'save error: ' + e);
    });
}

function hide_stream_progress() {
    stream_loading = false;
    var bar = document.getElementById('stream-progress');
    if (!bar || bar.style.display === 'none') return;
    bar.style.opacity = '0';
    setTimeout(function() {
        bar.style.display = 'none';
        bar.style.opacity = '';
    }, 450);
}

// load_zip(stream, content_length) — drives the WASM streaming zip parser.
// `stream` is a ReadableStream (fetch response.body or File.stream()).
// `content_length` is the total byte count (for progress %; pass 0 if unknown).
function load_zip(stream, content_length) {
    if (animation_id     !== null) { cancelAnimationFrame(animation_id);    animation_id     = null; }
    if (hover_anim_id    !== null) { cancelAnimationFrame(hover_anim_id);   hover_anim_id    = null; }
    if (hover_idle_timer !== null) { clearTimeout(hover_idle_timer);        hover_idle_timer = null; }

    var loading = document.getElementById('loading');
    if (loading) loading.style.display = '';

    if (info_visible)  hide_info();
    if (about_visible) hide_about();
    zoom_mode     = false;
    zoom_scale    = 1.0;
    zoom_pan_x    = 0;
    zoom_pan_y    = 0;
    current_index = null;

    var header = document.getElementById('header_container');
    header.innerHTML = '';
    thumbs = [];

    var bar    = document.getElementById('stream-progress');
    var fill   = document.getElementById('stream-fill');
    var errDiv = document.getElementById('progress-error');
    stream_loading = true;
    if (bar)    { bar.style.opacity = ''; bar.style.display = 'block'; }
    if (fill)   fill.style.width = '0%';
    if (errDiv) errDiv.style.display = 'none';

    renderer.begin_zip_stream();
    var gen       = ++load_gen;
    ++thumb_gen;

    var reader      = stream.getReader();
    var known_count = 0;
    var bytes_recv  = 0;
    var first_shown = false;

    function pump() {
        reader.read().then(function(result) {
            if (gen !== load_gen) { reader.cancel(); return; }

            if (!result.done) {
                bytes_recv += result.value.length;
                if (fill && content_length > 0)
                    fill.style.width = Math.round(bytes_recv / content_length * 100) + '%';

                try {
                    var new_count = renderer.feed_bytes(result.value);

                    for (var i = known_count; i < new_count; i++) {
                        add_thumbnail(i);
                    }

                    // Show image 0 as soon as its entry arrives.
                    if (!first_shown && new_count > 0) {
                        first_shown = true;
                        set_current_index(0);
                        decode_image(0, function() {
                            if (gen !== load_gen) return;
                            draw(0);
                            if (loading) loading.style.display = 'none';
                            if (renderer.image_count() > 1) decode_image(1, null);
                        });
                    }

                    // Prefetch newly arrived neighbours of the current image.
                    if (current_index !== null) {
                        for (var j = known_count; j < new_count; j++) {
                            if (j === current_index - 1 || j === current_index + 1) {
                                decode_image(j, null);
                            }
                        }
                    }

                    known_count = new_count;
                } catch(e) {
                    glimr_log('load_zip', 'error: ' + e);
                    hide_stream_progress();
                    if (errDiv) { errDiv.textContent = String(e); errDiv.style.display = ''; }
                    return;
                }
            }

            if (result.done || renderer.is_stream_done()) {
                glimr_log('load_zip', 'stream done, ' + known_count + ' images');
                hide_stream_progress();
                if (known_count === 0 && errDiv) {
                    errDiv.textContent = 'No images found in archive.';
                    errDiv.style.display = '';
                }
                return;
            }

            pump();
        }).catch(function(e) {
            if (gen !== load_gen) return;
            glimr_log('load_zip', 'fetch error: ' + e);
            hide_stream_progress();
            if (errDiv) { errDiv.textContent = 'Error: ' + String(e); errDiv.style.display = ''; }
        });
    }
    pump();
}

// ---------------------------------------------------------------------------
// Loading screen
// ---------------------------------------------------------------------------

function create_loading_screen() {
    var div = document.getElementById('loading');
    ['Welcome to Glimpse-o-Matic!', 'Loading now ....'].forEach(function(text) {
        var line = document.createElement('div');
        line.className = 'loading-line';
        var delay = 0;
        text.split('').forEach(function(ch) {
            var span = document.createElement('span');
            span.textContent = ch === ' ' ? ' ' : ch;
            span.style.animationDelay = delay.toFixed(2) + 's';
            if (ch !== ' ') delay += 0.05;
            line.appendChild(span);
        });
        div.appendChild(line);
    });

    var errDiv = document.createElement('div');
    errDiv.id = 'progress-error';
    div.appendChild(errDiv);
}

// ---------------------------------------------------------------------------
// Carousel / thumbnails
// ---------------------------------------------------------------------------

// Creates a single carousel thumbnail for image i and appends it to the header.
// Safe to call incrementally as images arrive from the stream.
function add_thumbnail(i) {
    var carousel_size = Math.min(
        (landscape_mq.matches ? window.innerWidth : window.innerHeight) * 0.18,
        G_CAROUSEL_SIZE_MAX
    );
    var header_container = document.getElementById('header_container');
    var gen = thumb_gen;
    let canvas = document.createElement('canvas');
    canvas.width  = 0;
    canvas.height = 0;
    let divbox = document.createElement('div');
    divbox.appendChild(canvas);
    header_container.appendChild(divbox);
    thumbs.push(canvas);
    canvas.style.boxShadow = '5px 5px 4px #888';
    canvas.style.border = '1px solid #bbb';
    canvas.style.borderRadius = '8px';
    canvas.style.margin = '4px';
    canvas.addEventListener('click', function() {
        if (carousel_drag_moved) { carousel_drag_moved = false; return; }
        navigate_to(i);
    });
    var bytes = renderer.get_image_bytes(i);
    createImageBitmap(new Blob([bytes])).then(function(bitmap) {
        if (gen !== thumb_gen) { bitmap.close(); return; }
        var vert = landscape_mq.matches;
        var scale = vert ? carousel_size / bitmap.width : carousel_size / bitmap.height;
        var tw = Math.round(bitmap.width  * scale);
        var th = Math.round(bitmap.height * scale);
        canvas.width  = tw;
        canvas.height = th;
        canvas.getContext('2d').drawImage(bitmap, 0, 0, tw, th);
        bitmap.close();
        if (i === current_index) scroll_carousel_to(i);
    }).catch(function(e) {
        glimr_log('add_thumbnail', 'thumb ' + i + ' error: ' + e);
    });
}

// Rebuilds all thumbnails for the currently loaded gallery (used by resize handler).
function create_thumbnails() {
    thumb_gen++;
    var count = renderer.image_count();
    for (var i = 0; i < count; i++) {
        add_thumbnail(i);
    }
}

// Stop any in-flight carousel momentum.
function carousel_stop_inertia() {
    if (carousel_inertia_raf) { cancelAnimationFrame(carousel_inertia_raf); carousel_inertia_raf = 0; }
    carousel_vel = 0;
}

// "Throw" the carousel: keep scrolling from the release velocity, decaying to a stop.
function carousel_start_inertia() {
    if (Math.abs(carousel_vel) < CAROUSEL_MIN_THROW_V) { carousel_vel = 0; return; }
    var header = document.getElementById('header_container');
    var vert = landscape_mq.matches;
    var last = performance.now();
    function step(now) {
        var dt = now - last; last = now;
        if (dt > 0) {
            var max = vert ? header.scrollHeight - header.clientHeight
                           : header.scrollWidth  - header.clientWidth;
            var next = (vert ? header.scrollTop : header.scrollLeft) - carousel_vel * dt; // scroll moves opposite the pointer
            if (next <= 0)        { next = 0;   carousel_vel = 0; }
            else if (next >= max) { next = max; carousel_vel = 0; }
            if (vert) header.scrollTop = next; else header.scrollLeft = next;
            carousel_vel *= Math.exp(-dt / CAROUSEL_FRICTION_TAU);
        }
        if (Math.abs(carousel_vel) < CAROUSEL_STOP_V) { carousel_inertia_raf = 0; return; }
        carousel_inertia_raf = requestAnimationFrame(step);
    }
    carousel_inertia_raf = requestAnimationFrame(step);
}

function scroll_carousel_to(index) {
    carousel_stop_inertia();
    var header = document.getElementById('header_container');
    var hr   = header.getBoundingClientRect();
    var vert = landscape_mq.matches;

    if (index < renderer.image_count() - 1) {
        var nr = thumbs[index + 1].getBoundingClientRect();
        if (vert ? nr.bottom > hr.bottom + 1 : nr.right > hr.right + 1) {
            thumbs[index + 1].scrollIntoView({behavior: 'smooth', inline: 'nearest', block: 'nearest'});
            return;
        }
    }
    if (index > 0) {
        var pr = thumbs[index - 1].getBoundingClientRect();
        if (vert ? pr.top < hr.top - 1 : pr.left < hr.left - 1) {
            thumbs[index - 1].scrollIntoView({behavior: 'smooth', inline: 'nearest', block: 'nearest'});
            return;
        }
    }
    thumbs[index].scrollIntoView({behavior: 'smooth', inline: 'nearest', block: 'nearest'});
}

function set_current_index(new_index) {
    zoom_mode = false;
    if (stream_loading) { var _bar = document.getElementById('stream-progress'); if (_bar) _bar.style.display = 'block'; }
    if (new_index === current_index) return;
    if (current_index !== null) {
        var old = thumbs[current_index];
        old.style.boxShadow = '5px 5px 4px #888';
        old.style.border = '1px solid #bbb';
        old.style.borderRadius = '8px';
        old.style.margin = '4px';
        old.style.opacity = '100%';
        old.style.filter = '';
    }
    current_index = new_index;
    thumbs[current_index].style.border = '1px solid red';
    thumbs[current_index].style.opacity = '75%';
    thumbs[current_index].style.filter = 'brightness(75%)';
    scroll_carousel_to(new_index);
}

// Decode image `index` via browser createImageBitmap → WASM receive_pixels.
// Calls callback() when done, or immediately if already cached.
function decode_image(index, callback) {
    if (renderer.is_decoded(index)) {
        if (callback) callback();
        return;
    }
    var bytes = renderer.get_image_bytes(index);
    createImageBitmap(new Blob([bytes])).then(function(bitmap) {
        var oc = new OffscreenCanvas(bitmap.width, bitmap.height);
        var ctx = oc.getContext('2d');
        ctx.drawImage(bitmap, 0, 0);
        var pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
        renderer.receive_pixels(index, bitmap.width, bitmap.height, pixels.data, build_payload());
        bitmap.close();
        // Cap the watermarked-RGBA cache, anchored on the displayed image so the
        // current view and its near neighbours are kept and far images evicted.
        var anchor = (current_index !== null) ? current_index : index;
        renderer.enforce_cache_budget(anchor);
        if (callback) callback();
    }).catch(function(e) {
        glimr_log('decode_image', 'error ' + index + ': ' + e);
    });
}

// Select image `index`: update thumbnail highlight, decode if needed, draw,
// then prefetch immediate neighbours for snappy swipe transitions.
function navigate_to(index) {
    set_current_index(index);
    decode_image(index, function() {
        draw(0);
        var count = renderer.image_count();
        if (index > 0)         decode_image(index - 1, null);
        if (index + 1 < count) decode_image(index + 1, null);
    });
}

// ---------------------------------------------------------------------------
// Hover indicator
// ---------------------------------------------------------------------------

function fade_out_indicator() {
    if (hover_anim_id !== null) { cancelAnimationFrame(hover_anim_id); hover_anim_id = null; }
    hover_anim_from = hover_opacity;
    hover_target = 0.0;
    hover_anim_start = performance.now();
    if (hover_opacity > 0) hover_anim_id = requestAnimationFrame(hover_tick);
}

function reset_hover_idle_timer() {
    if (hover_idle_timer !== null) { clearTimeout(hover_idle_timer); hover_idle_timer = null; }
    if (hover_zone !== null) {
        hover_idle_timer = setTimeout(function() {
            hover_idle_timer = null;
            fade_out_indicator();
        }, 1000);
    }
}

function hover_tick(now) {
    var t = Math.min((now - hover_anim_start) / 200, 1.0);
    hover_opacity = hover_anim_from + (hover_target - hover_anim_from) * t;
    draw(current_draw_offset);
    if (t < 1.0) {
        hover_anim_id = requestAnimationFrame(hover_tick);
    } else {
        hover_anim_id = null;
        hover_opacity = hover_target;
    }
}

function set_hover(new_zone) {
    var target = new_zone !== null ? 1.0 : 0.0;
    if (new_zone === hover_zone && target === hover_target) return;
    hover_zone = new_zone;
    if (hover_anim_id !== null) { cancelAnimationFrame(hover_anim_id); hover_anim_id = null; }
    hover_anim_from = hover_opacity;
    hover_target = target;
    hover_anim_start = performance.now();
    hover_anim_id = requestAnimationFrame(hover_tick);
}

function refresh_hover() {
    if (hover_zone === null) return;
    if (hover_anim_id !== null) { cancelAnimationFrame(hover_anim_id); hover_anim_id = null; }
    hover_anim_from = hover_opacity;
    hover_target = 1.0;
    hover_anim_start = performance.now();
    hover_anim_id = requestAnimationFrame(hover_tick);
    reset_hover_idle_timer();
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

function draw(offset) {
    if (offset === undefined) offset = 0;
    if (current_index === null) return;
    current_draw_offset = offset;

    if (zoom_mode) {
        renderer.draw_zoomed(current_index, zoom_scale, zoom_pan_x, zoom_pan_y);
    } else {
        renderer.draw(current_index, offset);
    }
    renderer.draw_hover_indicator(current_index, hover_zone || '', hover_opacity);
}

// ---------------------------------------------------------------------------
// Zoom entry / exit
// ---------------------------------------------------------------------------

function clamp_pan() {
    if (current_index === null) return;
    var img_w = renderer.image_width(current_index);
    var img_h = renderer.image_height(current_index);
    if (!img_w || !img_h) return;
    var photo_box = document.getElementById('lobjet_pane');
    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;
    zoom_pan_x = Math.max(0, Math.min(zoom_pan_x, Math.max(0, img_w - W / zoom_scale)));
    zoom_pan_y = Math.max(0, Math.min(zoom_pan_y, Math.max(0, img_h - H / zoom_scale)));
}

function enter_zoom(tap_x, tap_y) {
    var img_w = renderer.image_width(current_index);
    var img_h = renderer.image_height(current_index);
    if (!img_w || !img_h) return;

    var photo_box = document.getElementById('lobjet_pane');
    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;

    // Reverse the fit-scale to find which image pixel was tapped.
    var scale = Math.min(W / img_w, H / img_h);
    var h_pad = (W - img_w * scale) / 2;
    var v_pad = (H - img_h * scale) / 2;
    var img_x = (tap_x - h_pad) / scale;
    var img_y = (tap_y - v_pad) / scale;

    zoom_scale = 1.0;

    // Pan so the tapped pixel stays at the same screen position at 1:1.
    zoom_pan_x = img_x - tap_x;
    zoom_pan_y = img_y - tap_y;
    clamp_pan();

    zoom_mode = true;
    if (stream_loading) { var _bar = document.getElementById('stream-progress'); if (_bar) _bar.style.display = 'none'; }
    draw(0);
}

function exit_zoom() {
    zoom_mode  = false;
    zoom_scale = 1.0;
    zoom_pan_x = 0;
    zoom_pan_y = 0;
    if (stream_loading) { var _bar = document.getElementById('stream-progress'); if (_bar) _bar.style.display = 'block'; }
    draw(0);
}

function enter_zoom_at_fit() {
    if (zoom_mode || current_index === null) return;
    var img_w = renderer.image_width(current_index);
    var img_h = renderer.image_height(current_index);
    if (!img_w || !img_h) return;
    var photo_box = document.getElementById('lobjet_pane');
    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;
    zoom_scale = Math.min(W / img_w, H / img_h);
    zoom_pan_x = 0;
    zoom_pan_y = 0;
    zoom_mode  = true;
    if (stream_loading) { var _bar = document.getElementById('stream-progress'); if (_bar) _bar.style.display = 'none'; }
}

function apply_zoom(factor, pivot_x, pivot_y) {
    if (current_index === null) return;
    var img_w = renderer.image_width(current_index);
    var img_h = renderer.image_height(current_index);
    if (!img_w || !img_h) return;
    var photo_box = document.getElementById('lobjet_pane');
    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;
    var fit_scale = Math.min(W / img_w, H / img_h);
    var new_scale = zoom_scale * factor;
    if (new_scale <= fit_scale) { exit_zoom(); return; }
    new_scale = Math.min(ZOOM_MAX, new_scale);
    if (new_scale === zoom_scale) return;
    zoom_pan_x += pivot_x / zoom_scale - pivot_x / new_scale;
    zoom_pan_y += pivot_y / zoom_scale - pivot_y / new_scale;
    zoom_scale  = new_scale;
    clamp_pan();
    draw(0);
}

// ---------------------------------------------------------------------------
// Slide animation
// ---------------------------------------------------------------------------

function animate_slide(from_offset, to_offset, on_complete) {
    if (animation_id !== null) {
        cancelAnimationFrame(animation_id);
        animation_id = null;
    }
    var duration = 250;
    var start_time = performance.now();

    function step(now) {
        var t = Math.min((now - start_time) / duration, 1.0);
        var eased = 1 - (1 - t) * (1 - t);
        var offset = from_offset + (to_offset - from_offset) * eased;
        if (t < 1.0) {
            draw(offset);
            animation_id = requestAnimationFrame(step);
        } else {
            animation_id = null;
            if (on_complete) {
                on_complete();
            } else {
                draw(0);
            }
        }
    }
    animation_id = requestAnimationFrame(step);
}

// ---------------------------------------------------------------------------
// Pointer handling (mouse + touch unified)
// ---------------------------------------------------------------------------

function pointer_start(x, y) {
    if (animation_id !== null) {
        cancelAnimationFrame(animation_id);
        animation_id = null;
    }
    is_dragging = true;
    drag_start_x = x;
    drag_start_y = y;
    drag_offset = 0;
    drag_moved = false;
}

function pointer_move(x, y) {
    if (!is_dragging) return;

    if (zoom_mode) {
        var dx = x - drag_start_x;
        var dy = y - drag_start_y;
        if (Math.abs(dx) > 5 || Math.abs(dy) > 5) drag_moved = true;
        zoom_pan_x -= dx / zoom_scale;
        zoom_pan_y -= dy / zoom_scale;
        clamp_pan();
        drag_start_x = x;
        drag_start_y = y;
        draw(0);
        return;
    }

    var raw = x - drag_start_x;
    if (raw > 0 && current_index === 0) raw = 0;
    if (raw < 0 && current_index === renderer.image_count() - 1) raw = 0;
    drag_offset = raw;
    if (Math.abs(drag_offset) > 5) drag_moved = true;
    draw(drag_offset);
}

function pointer_end() {
    if (!is_dragging) return;
    is_dragging = false;

    if (zoom_mode) {
        if (!drag_moved) exit_zoom();
        return;
    }

    if (!drag_moved) {
        var rect = document.getElementById('photo').getBoundingClientRect();
        var tap_x = drag_start_x - rect.left;
        var tap_y = drag_start_y - rect.top;
        var W = rect.width;
        if (tap_x < W / 3) retreat();
        else if (tap_x > W * 2 / 3) advance();
        else enter_zoom(tap_x, tap_y);
        return;
    }

    var W = document.getElementById('photo').width;
    var threshold = W * 0.25;
    var saved = drag_offset;
    drag_offset = 0;

    if (saved > threshold && current_index > 0) {
        animate_slide(saved, W, function() {
            navigate_to(current_index - 1);
        });
    } else if (saved < -threshold && current_index < renderer.image_count() - 1) {
        animate_slide(saved, -W, function() {
            navigate_to(current_index + 1);
        });
    } else {
        animate_slide(saved, 0, null);
    }
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

function advance() {
    zoom_mode = false;
    var i = current_index + 1;
    if (i >= renderer.image_count()) i = 0;
    navigate_to(i);
    refresh_hover();
}

function retreat() {
    zoom_mode = false;
    var i = current_index - 1;
    if (i < 0) i = renderer.image_count() - 1;
    navigate_to(i);
    refresh_hover();
}

function wheel(event) {
    event.preventDefault();
    if (event.deltaY > 0) advance();
    else retreat();
}

function keydown(event) {
    if (about_visible) { hide_about(); return; }
    if (info_visible) {
        if (event.key === 'i' || event.key === 'I') hide_info();
        return;
    }
    if (event.key === 'i' || event.key === 'I') { show_info(); return; }
    if (event.key === 'ArrowRight' || event.key === 'Right') {
        if (zoom_mode) { zoom_pan_x += ARROW_PAN_PX / zoom_scale; clamp_pan(); draw(0); }
        else advance();
    }
    else if (event.key === 'ArrowLeft' || event.key === 'Left') {
        if (zoom_mode) { zoom_pan_x -= ARROW_PAN_PX / zoom_scale; clamp_pan(); draw(0); }
        else retreat();
    }
    else if (event.key === 'ArrowDown' || event.key === 'Down') {
        if (zoom_mode) { zoom_pan_y += ARROW_PAN_PX / zoom_scale; clamp_pan(); draw(0); }
    }
    else if (event.key === 'ArrowUp' || event.key === 'Up') {
        if (zoom_mode) { zoom_pan_y -= ARROW_PAN_PX / zoom_scale; clamp_pan(); draw(0); }
    }
    else if (event.key === '0') {
        if (zoom_mode) exit_zoom();
    }
    else if (event.key === 'f' || event.key === 'F') {
        flash_button(document.getElementById('btn-fullscreen'), !document.fullscreenElement);
        toggle_fullscreen();
    }
    else if (event.ctrlKey && (event.key === '+' || event.key === '=')) {
        event.preventDefault();
        enter_zoom_at_fit();
        var pb = document.getElementById('lobjet_pane');
        apply_zoom(ZOOM_KEY_STEP, pb.clientWidth / 2, pb.clientHeight / 2);
    }
    else if (zoom_mode && event.ctrlKey && event.key === '-') {
        event.preventDefault();
        var pb = document.getElementById('lobjet_pane');
        apply_zoom(1 / ZOOM_KEY_STEP, pb.clientWidth / 2, pb.clientHeight / 2);
    }
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

function init() {
    var photo = document.getElementById('photo');
    document.onkeydown = keydown;

    var pane = document.getElementById('lobjet_pane');

    pane.addEventListener('mousedown', function(e) {
        pointer_start(e.clientX, e.clientY);
    }, false);
    pane.addEventListener('mousemove', function(e) {
        pointer_move(e.clientX, e.clientY);
        if (!zoom_mode) {
            var rect = document.getElementById('photo').getBoundingClientRect();
            var x = e.clientX - rect.left;
            var W = rect.width;
            set_hover(x < W / 3 ? 'left' : x > W * 2 / 3 ? 'right' : null);
            reset_hover_idle_timer();
        }
    }, false);
    pane.addEventListener('mouseup',    function() { pointer_end(); }, false);
    pane.addEventListener('mouseleave', function() {
        pointer_end();
        set_hover(null);
        reset_hover_idle_timer();
    }, false);
    pane.addEventListener('wheel', function(e) {
        e.preventDefault();
        enter_zoom_at_fit();
        if (!zoom_mode) return;
        var rect = document.getElementById('photo').getBoundingClientRect();
        var raw = e.deltaY;
        if (e.deltaMode === 1) raw *= 30;
        if (e.deltaMode === 2) raw *= 300;
        apply_zoom(Math.pow(1.001, -raw), e.clientX - rect.left, e.clientY - rect.top);
    }, {passive: false});

    pane.addEventListener('touchstart', function(e) {
        e.preventDefault();
        if (e.touches.length === 2) {
            enter_zoom_at_fit();
            var t0 = e.touches[0], t1 = e.touches[1];
            var dx = t1.clientX - t0.clientX;
            var dy = t1.clientY - t0.clientY;
            pinch_active      = true;
            pinch_start_dist  = Math.sqrt(dx*dx + dy*dy);
            pinch_start_scale = zoom_scale;
            pinch_start_pan_x = zoom_pan_x;
            pinch_start_pan_y = zoom_pan_y;
            var rect = document.getElementById('photo').getBoundingClientRect();
            pinch_mid_x = (t0.clientX + t1.clientX) / 2 - rect.left;
            pinch_mid_y = (t0.clientY + t1.clientY) / 2 - rect.top;
            is_dragging = false;
        } else {
            pinch_active = false;
            pointer_start(e.touches[0].clientX, e.touches[0].clientY);
        }
    }, false);
    pane.addEventListener('touchmove', function(e) {
        e.preventDefault();
        if (pinch_active && e.touches.length === 2) {
            var t0 = e.touches[0], t1 = e.touches[1];
            var dx = t1.clientX - t0.clientX;
            var dy = t1.clientY - t0.clientY;
            var dist = Math.sqrt(dx*dx + dy*dy);
            if (pinch_start_dist > 0) {
                var img_w = renderer.image_width(current_index);
                var img_h = renderer.image_height(current_index);
                if (img_w && img_h) {
                    var photo_box = document.getElementById('lobjet_pane');
                    var W = photo_box.clientWidth;
                    var H = photo_box.clientHeight;
                    var fit_scale = Math.min(W / img_w, H / img_h);
                    var raw_scale = pinch_start_scale * dist / pinch_start_dist;
                    if (raw_scale <= fit_scale) { exit_zoom(); pinch_active = false; return; }
                    var new_scale = Math.min(ZOOM_MAX, raw_scale);
                    var img_mid_x = pinch_start_pan_x + pinch_mid_x / pinch_start_scale;
                    var img_mid_y = pinch_start_pan_y + pinch_mid_y / pinch_start_scale;
                    zoom_pan_x = img_mid_x - pinch_mid_x / new_scale;
                    zoom_pan_y = img_mid_y - pinch_mid_y / new_scale;
                    zoom_scale = new_scale;
                    clamp_pan();
                    draw(0);
                }
            }
        } else {
            pointer_move(e.touches[0].clientX, e.touches[0].clientY);
        }
    }, false);
    pane.addEventListener('touchend', function(e) {
        e.preventDefault();
        if (e.touches.length < 2) pinch_active = false;
        if (!pinch_active) pointer_end();
    }, false);
    pane.addEventListener('touchcancel', function() {
        pinch_active = false;
        pointer_end();
    }, false);

    var header = document.getElementById('header_container');

    // Unified pointer drag-to-scroll with kinetic "throw" (mouse + touch + pen).
    // Pointer capture (taken lazily, only once it's a real drag — see pointermove) keeps the
    // drag alive past the strip edges without retargeting a stationary click away from the
    // thumbnail; touch-action:none (CSS) hands touch panning to us for consistent momentum.
    header.addEventListener('pointerdown', function(e) {
        var was_gliding = carousel_inertia_raf !== 0;
        carousel_stop_inertia();
        carousel_is_dragging = true;
        carousel_pointer_id = e.pointerId;
        carousel_captured = false;
        var vert = landscape_mq.matches;
        carousel_drag_start_x = e.clientX;
        carousel_drag_start_y = e.clientY;
        carousel_scroll_start = vert ? header.scrollTop : header.scrollLeft;
        carousel_last_pos = vert ? e.clientY : e.clientX;
        carousel_last_t = performance.now();
        carousel_vel = 0;
        // A press that catches a moving carousel counts as a "move" so the tap stops it
        // (catch) rather than selecting a thumbnail.
        carousel_drag_moved = was_gliding;
    }, false);
    header.addEventListener('pointermove', function(e) {
        if (!carousel_is_dragging || e.pointerId !== carousel_pointer_id) return;
        var vert = landscape_mq.matches;
        var d = vert ? e.clientY - carousel_drag_start_y : e.clientX - carousel_drag_start_x;
        if (Math.abs(d) > 4) {
            carousel_drag_moved = true;
            if (!carousel_captured) { // capture only now → a non-moving click still selects
                try { header.setPointerCapture(carousel_pointer_id); carousel_captured = true; } catch (_) {}
            }
        }
        var pos = vert ? e.clientY : e.clientX;
        var now = performance.now();
        var dt = now - carousel_last_t;
        if (dt > 100) carousel_vel = 0;                            // long gap → treat as paused
        else if (dt > 0) carousel_vel = 0.8 * carousel_vel + 0.2 * (pos - carousel_last_pos) / dt;
        carousel_last_pos = pos;
        carousel_last_t = now;
        if (vert) header.scrollTop  = carousel_scroll_start - d;
        else      header.scrollLeft = carousel_scroll_start - d;
    }, false);
    function carousel_pointer_end() {
        if (!carousel_is_dragging) return;
        carousel_is_dragging = false;
        if (carousel_captured && carousel_pointer_id !== null) {
            try { header.releasePointerCapture(carousel_pointer_id); } catch (_) {}
        }
        carousel_captured = false;
        carousel_pointer_id = null;
        // Held still before release (no moves fired during the pause) ⇒ stale velocity; don't throw.
        if (performance.now() - carousel_last_t > CAROUSEL_IDLE_MS) carousel_vel = 0;
        if (carousel_drag_moved) carousel_start_inertia();         // throw only after a real drag
    }
    header.addEventListener('pointerup', carousel_pointer_end, false);
    header.addEventListener('pointercancel', function() {
        carousel_is_dragging = false;
        carousel_captured = false;
        carousel_pointer_id = null;
        carousel_vel = 0;
    }, false);
    header.addEventListener('wheel', function(e) {
        e.preventDefault();
        carousel_stop_inertia();
        if (landscape_mq.matches) header.scrollTop  += e.deltaY;
        else                       header.scrollLeft += e.deltaY;
    }, {passive: false});

    var last_landscape = landscape_mq.matches;
    window.addEventListener('resize', function() {
        draw();
        var now_landscape = landscape_mq.matches;
        if (now_landscape !== last_landscape) {
            last_landscape = now_landscape;
            if (renderer.image_count() === 0) return;
            var saved = current_index;
            current_index = null;
            var header = document.getElementById('header_container');
            header.innerHTML = '';
            thumbs = [];
            create_thumbnails();
            set_current_index(saved);
        }
    });

    document.getElementById('btn-info').addEventListener('click', function() {
        flash_button(this);
        if (info_visible) hide_info(); else show_info();
    });

    document.getElementById('info-overlay').addEventListener('click', function(e) {
        if (e.target === this) hide_info();
    });
    document.getElementById('info-close').addEventListener('click', hide_info);

    document.getElementById('btn-about').addEventListener('click', function() {
        flash_button(this);
        if (about_visible) hide_about(); else show_about();
    });
    document.getElementById('about-overlay').addEventListener('click', function(e) {
        if (e.target === this) hide_about();
    });
    document.getElementById('about-close').addEventListener('click', hide_about);

    document.getElementById('btn-load').addEventListener('click', function() {
        flash_button(this);
        var input = document.createElement('input');
        input.type = 'file';
        input.accept = '.zip';
        input.style.display = 'none';
        document.body.appendChild(input);
        input.addEventListener('change', function() {
            document.body.removeChild(input);
            if (!input.files || !input.files[0]) return;
            var file = input.files[0];
            load_zip(file.stream(), file.size);
        });
        input.click();
    });

    document.getElementById('btn-fullscreen').addEventListener('click', function() {
        flash_button(this, !document.fullscreenElement);
        toggle_fullscreen();
    });

    document.getElementById('btn-download').addEventListener('click', function() {
        flash_button(this);
        download_current();
    });

    document.addEventListener('fullscreenchange', function() {
        var btn = document.getElementById('btn-fullscreen');
        if (document.fullscreenElement) {
            btn.classList.add('active');
        } else {
            btn.classList.remove('active');
        }
    });

    create_loading_screen();
    fetch('Demo.zip').then(function(r) {
        var len = parseInt(r.headers.get('Content-Length'), 10) || 0;
        load_zip(r.body, len);
    });
}
