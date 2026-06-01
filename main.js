var images = [];
var blob_urls  = {};
var file_sizes = {};
var info_visible = false;

const G_CAROUSEL_SIZE_MAX = 160.0;
var landscape_mq = window.matchMedia('(orientation: landscape)');

var thumbs = [];
var image_cache = {};
var current_index = null;

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

// Hover indicator state
var hover_zone = null;
var hover_opacity = 0.0;
var hover_target = 0.0;
var hover_anim_id = null;
var hover_anim_from = 0.0;
var hover_anim_start = 0;
var hover_idle_timer = null;
var current_draw_offset = 0;

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
    var name = images[current_index].replace(/\.dat$/i, '.jpg');
    var img  = image_cache[images[current_index]];
    var size = file_sizes[images[current_index]] || 0;

    document.getElementById('info-filename').textContent = name;

    var content = document.getElementById('info-content');
    content.innerHTML = '';
    [img && img.naturalWidth ? img.naturalWidth + ' × ' + img.naturalHeight : null,
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

function download_current() {
    if (current_index === null) return;
    var a = document.createElement('a');
    a.href = blob_urls[images[current_index]];
    a.download = images[current_index].replace(/\.dat$/i, '.jpg');
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
}

function load_zip(buf) {
    if (animation_id     !== null) { cancelAnimationFrame(animation_id);    animation_id     = null; }
    if (hover_anim_id    !== null) { cancelAnimationFrame(hover_anim_id);   hover_anim_id    = null; }
    if (hover_idle_timer !== null) { clearTimeout(hover_idle_timer);        hover_idle_timer = null; }

    var loading = document.getElementById('loading');
    if (loading) loading.style.display = '';

    if (info_visible) hide_info();
    Object.keys(blob_urls).forEach(function(k) { URL.revokeObjectURL(blob_urls[k]); });
    images        = [];
    blob_urls     = {};
    file_sizes    = {};
    image_cache   = {};
    zoom_mode     = false;
    zoom_scale    = 1.0;
    zoom_pan_x    = 0;
    zoom_pan_y    = 0;
    current_index = null;

    var header = document.getElementById('header_container');
    header.innerHTML = '';
    thumbs = [];

    var archive = new window.glimr.GlimrZip(new Uint8Array(buf));
    var count = archive.entry_count();
    for (var i = 0; i < count; i++) {
        var name = archive.entry_name(i);
        var blob = new Blob([archive.entry_data(i)], {type: 'image/jpeg'});
        file_sizes[name] = blob.size;
        images.push(name);
        blob_urls[name] = URL.createObjectURL(blob);
    }
    archive.free();
    create_thumbnails();
    preload_images();
    set_current_index(0);
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
}

// ---------------------------------------------------------------------------
// Image loading
// ---------------------------------------------------------------------------

function preload_images() {
    images.forEach(function(src, i) {
        var img = new Image();
        img.onload = function() {
            image_cache[src] = img;
            if (i === 0) {
                var loading = document.getElementById('loading');
                if (loading) loading.style.display = 'none';
                draw(0);
            }
        };
        img.src = blob_urls[src];
    });
}


// ---------------------------------------------------------------------------
// Carousel / thumbnails
// ---------------------------------------------------------------------------

function create_thumbnails() {
    var carousel_size = Math.min(
        (landscape_mq.matches ? window.innerWidth : window.innerHeight) * 0.18,
        G_CAROUSEL_SIZE_MAX
    );
    var header_container = document.getElementById('header_container');
    for (let i = 0; i < images.length; i++) {
        var canvas = document.createElement("canvas");
        canvas.width  = 0;
        canvas.height = 0;
        canvas.dataset.imageNumber = i;
        canvas.dataset.imageSrc = blob_urls[images[i]];
        var divbox = document.createElement('DIV');
        divbox.appendChild(canvas);
        header_container.appendChild(divbox);
        thumbs.push(canvas);
    }
    var l = Array.from(header_container.getElementsByTagName('CANVAS'));
    l.forEach(function(canvas) {
        var image = new Image();
        var ctx = canvas.getContext('2d');
        image.onload = function() {
            var scale = landscape_mq.matches
                ? carousel_size / image.width
                : carousel_size / image.height;
            canvas.width = image.width * scale;
            canvas.height = image.height * scale;
            canvas.style.boxShadow = '5px 5px 4px #888';
            canvas.style.border = '1px solid #bbb';
            canvas.style.borderRadius = '8px';
            canvas.style.margin = '4px';
            canvas.addEventListener('click', function() {
                if (carousel_drag_moved) { carousel_drag_moved = false; return; }
                set_current_index(parseInt(canvas.dataset.imageNumber));
                draw(0);
            });
            ctx.scale(scale, scale);
            ctx.drawImage(image, 0, 0);
        };
        image.src = canvas.dataset.imageSrc;
    });
}

function scroll_carousel_to(index) {
    var header = document.getElementById('header_container');
    var hr   = header.getBoundingClientRect();
    var vert = landscape_mq.matches;

    if (index < images.length - 1) {
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

// ---------------------------------------------------------------------------
// Hover indicator
// ---------------------------------------------------------------------------

function hover_symbol() {
    if (hover_zone === 'left')  return current_index === 0                 ? '>>' : '<';
    if (hover_zone === 'right') return current_index === images.length - 1 ? '<<' : '>';
    return '';
}

function draw_hover_indicator(canvas, W, H) {
    if (hover_opacity <= 0 || hover_zone === null) return;
    var symbol = hover_symbol();
    var cx = hover_zone === 'left' ? W / 6 : W * 5 / 6;
    var ctx = canvas.getContext('2d');
    ctx.save();
    ctx.globalAlpha = hover_opacity * 0.6;
    ctx.fillStyle = '#ddd';
    ctx.shadowColor = '#555';
    ctx.shadowBlur = 4;
    ctx.shadowOffsetX = 3;
    ctx.shadowOffsetY = 3;
    ctx.font = 'bold ' + Math.round(H * 0.10) + 'px sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(symbol, cx, H / 2);
    ctx.restore();
}

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

function draw_image_in_column(ctx, image, col_x, col_w, col_h) {
    var scale = Math.min(col_w / image.naturalWidth, col_h / image.naturalHeight);
    var img_w = image.naturalWidth * scale;
    var img_h = image.naturalHeight * scale;
    var h_pad = (col_w - img_w) / 2.0;
    var v_pad = (col_h - img_h) / 2.0;
    ctx.drawImage(image, col_x + h_pad, v_pad, img_w, img_h);
}

function steg(canvas) {
    var x = canvas.width / 2;
    var y = canvas.height / 2;
    var ctx = canvas.getContext('2d');
    var image_data = ctx.getImageData(x, y, 100, 1);
    for (let i = 0; i < 100; i++) {
        image_data.data[i * 4]     &= 240;
        image_data.data[i * 4 + 1] &= 240;
        image_data.data[i * 4 + 2] &= 240;
        image_data.data[i * 4 + 3] = 255;
    }
    ctx.putImageData(image_data, x, y);
}

function draw_zoomed() {
    var backing_canvas = document.getElementById('backing');
    var canvas = document.getElementById('photo');
    var photo_box = document.getElementById('lobjet_pane');

    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;

    backing_canvas.width = W;
    backing_canvas.height = H;

    var ctx = backing_canvas.getContext('2d');
    ctx.fillStyle = '#777';
    ctx.fillRect(0, 0, W, H);

    var img = image_cache[images[current_index]];
    if (img && img.complete && img.naturalWidth > 0) {
        var src_w = Math.min(img.naturalWidth  - zoom_pan_x, W / zoom_scale);
        var src_h = Math.min(img.naturalHeight - zoom_pan_y, H / zoom_scale);
        var dst_w = src_w * zoom_scale;
        var dst_h = src_h * zoom_scale;
        ctx.drawImage(img,
            zoom_pan_x, zoom_pan_y, src_w, src_h,
            (W - dst_w) / 2, (H - dst_h) / 2, dst_w, dst_h);
    }

    canvas.width = W;
    canvas.height = H;
    canvas.getContext('2d').drawImage(backing_canvas, 0, 0);
}

function draw(offset) {
    if (offset === undefined) offset = 0;
    if (current_index === null) return;
    current_draw_offset = offset;

    if (zoom_mode) {
        draw_zoomed();
        return;
    }

    var backing_canvas = document.getElementById('backing');
    var canvas = document.getElementById('photo');
    var photo_box = document.getElementById('lobjet_pane');

    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;

    backing_canvas.width = W;
    backing_canvas.height = H;

    var ctx = backing_canvas.getContext('2d');
    ctx.fillStyle = '#777';
    ctx.fillRect(0, 0, W, H);

    var cur_img = image_cache[images[current_index]];
    if (cur_img && cur_img.complete && cur_img.naturalWidth > 0) {
        draw_image_in_column(ctx, cur_img, offset, W, H);
    }

    if (offset > 0 && current_index > 0) {
        var prev_img = image_cache[images[current_index - 1]];
        if (prev_img && prev_img.complete && prev_img.naturalWidth > 0) {
            draw_image_in_column(ctx, prev_img, offset - W, W, H);
        }
    } else if (offset < 0 && current_index < images.length - 1) {
        var next_img = image_cache[images[current_index + 1]];
        if (next_img && next_img.complete && next_img.naturalWidth > 0) {
            draw_image_in_column(ctx, next_img, offset + W, W, H);
        }
    }

    canvas.width = W;
    canvas.height = H;
    canvas.getContext('2d').drawImage(backing_canvas, 0, 0);
    steg(canvas);
    draw_hover_indicator(canvas, W, H);
}

// ---------------------------------------------------------------------------
// Zoom entry / exit
// ---------------------------------------------------------------------------

function clamp_pan() {
    var img = image_cache[images[current_index]];
    if (!img) return;
    var photo_box = document.getElementById('lobjet_pane');
    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;
    zoom_pan_x = Math.max(0, Math.min(zoom_pan_x, Math.max(0, img.naturalWidth  - W / zoom_scale)));
    zoom_pan_y = Math.max(0, Math.min(zoom_pan_y, Math.max(0, img.naturalHeight - H / zoom_scale)));
}

function enter_zoom(tap_x, tap_y) {
    var img = image_cache[images[current_index]];
    if (!img || !img.complete || img.naturalWidth === 0) return;

    var photo_box = document.getElementById('lobjet_pane');
    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;

    // Reverse the fit-scale to find which image pixel was tapped.
    var scale = Math.min(W / img.naturalWidth, H / img.naturalHeight);
    var h_pad = (W - img.naturalWidth  * scale) / 2;
    var v_pad = (H - img.naturalHeight * scale) / 2;
    var img_x = (tap_x - h_pad) / scale;
    var img_y = (tap_y - v_pad) / scale;

    zoom_scale = 1.0;

    // Pan so the tapped pixel stays at the same screen position at 1:1.
    zoom_pan_x = img_x - tap_x;
    zoom_pan_y = img_y - tap_y;
    clamp_pan();

    zoom_mode = true;
    draw(0);
}

function exit_zoom() {
    zoom_mode  = false;
    zoom_scale = 1.0;
    zoom_pan_x = 0;
    zoom_pan_y = 0;
    draw(0);
}

function enter_zoom_at_fit() {
    if (zoom_mode || current_index === null) return;
    var img = image_cache[images[current_index]];
    if (!img || !img.complete || img.naturalWidth === 0) return;
    var photo_box = document.getElementById('lobjet_pane');
    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;
    zoom_scale = Math.min(W / img.naturalWidth, H / img.naturalHeight);
    zoom_pan_x = 0;
    zoom_pan_y = 0;
    zoom_mode  = true;
}

function apply_zoom(factor, pivot_x, pivot_y) {
    var img = image_cache[images[current_index]];
    if (!img) return;
    var photo_box = document.getElementById('lobjet_pane');
    var W = photo_box.clientWidth;
    var H = photo_box.clientHeight;
    var fit_scale = Math.min(W / img.naturalWidth, H / img.naturalHeight);
    var new_scale = zoom_scale * factor;
    if (new_scale <= fit_scale) { exit_zoom(); return; }
    new_scale = Math.min(ZOOM_MAX, new_scale);
    if (new_scale === zoom_scale) return;
    zoom_pan_x += pivot_x / zoom_scale - pivot_x / new_scale;
    zoom_pan_y += pivot_y / zoom_scale - pivot_y / new_scale;
    zoom_scale  = new_scale;
    clamp_pan();
    draw_zoomed();
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
    if (raw < 0 && current_index === images.length - 1) raw = 0;
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
            set_current_index(current_index - 1);
            draw(0);
        });
    } else if (saved < -threshold && current_index < images.length - 1) {
        animate_slide(saved, -W, function() {
            set_current_index(current_index + 1);
            draw(0);
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
    if (i >= images.length) i = 0;
    set_current_index(i);
    draw(0);
    refresh_hover();
}

function retreat() {
    zoom_mode = false;
    var i = current_index - 1;
    if (i < 0) i = images.length - 1;
    set_current_index(i);
    draw(0);
    refresh_hover();
}

function wheel(event) {
    event.preventDefault();
    if (event.deltaY > 0) advance();
    else retreat();
}

function keydown(event) {
    if (info_visible) {
        if (event.key === 'i' || event.key === 'I') hide_info();
        return;
    }
    if (event.key === 'i' || event.key === 'I') { show_info(); return; }
    if (event.key === 'ArrowRight' || event.key === 'Right') {
        if (zoom_mode) { zoom_pan_x += ARROW_PAN_PX / zoom_scale; clamp_pan(); draw_zoomed(); }
        else advance();
    }
    else if (event.key === 'ArrowLeft' || event.key === 'Left') {
        if (zoom_mode) { zoom_pan_x -= ARROW_PAN_PX / zoom_scale; clamp_pan(); draw_zoomed(); }
        else retreat();
    }
    else if (event.key === 'ArrowDown' || event.key === 'Down') {
        if (zoom_mode) { zoom_pan_y += ARROW_PAN_PX / zoom_scale; clamp_pan(); draw_zoomed(); }
    }
    else if (event.key === 'ArrowUp' || event.key === 'Up') {
        if (zoom_mode) { zoom_pan_y -= ARROW_PAN_PX / zoom_scale; clamp_pan(); draw_zoomed(); }
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
                var img = image_cache[images[current_index]];
                if (img) {
                    var photo_box = document.getElementById('lobjet_pane');
                    var W = photo_box.clientWidth;
                    var H = photo_box.clientHeight;
                    var fit_scale = Math.min(W / img.naturalWidth, H / img.naturalHeight);
                    var raw_scale = pinch_start_scale * dist / pinch_start_dist;
                    if (raw_scale <= fit_scale) { exit_zoom(); pinch_active = false; return; }
                    var new_scale = Math.min(ZOOM_MAX, raw_scale);
                    var img_mid_x = pinch_start_pan_x + pinch_mid_x / pinch_start_scale;
                    var img_mid_y = pinch_start_pan_y + pinch_mid_y / pinch_start_scale;
                    zoom_pan_x = img_mid_x - pinch_mid_x / new_scale;
                    zoom_pan_y = img_mid_y - pinch_mid_y / new_scale;
                    zoom_scale = new_scale;
                    clamp_pan();
                    draw_zoomed();
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

    header.addEventListener('mousedown', function(e) {
        carousel_is_dragging = true;
        carousel_drag_start_x = e.clientX;
        carousel_drag_start_y = e.clientY;
        carousel_scroll_start = landscape_mq.matches ? header.scrollTop : header.scrollLeft;
        carousel_drag_moved = false;
    }, false);
    header.addEventListener('mousemove', function(e) {
        if (!carousel_is_dragging) return;
        var d = landscape_mq.matches
            ? e.clientY - carousel_drag_start_y
            : e.clientX - carousel_drag_start_x;
        if (Math.abs(d) > 4) carousel_drag_moved = true;
        if (landscape_mq.matches) header.scrollTop  = carousel_scroll_start - d;
        else                       header.scrollLeft = carousel_scroll_start - d;
    }, false);
    header.addEventListener('mouseup',    function() { carousel_is_dragging = false; }, false);
    header.addEventListener('mouseleave', function() { carousel_is_dragging = false; }, false);
    header.addEventListener('wheel', function(e) {
        e.preventDefault();
        if (landscape_mq.matches) header.scrollTop  += e.deltaY;
        else                       header.scrollLeft += e.deltaY;
    }, {passive: false});

    header.addEventListener('touchstart', function(e) {
        carousel_is_dragging = true;
        carousel_drag_start_x = e.touches[0].clientX;
        carousel_drag_start_y = e.touches[0].clientY;
        carousel_scroll_start = landscape_mq.matches ? header.scrollTop : header.scrollLeft;
        carousel_drag_moved = false;
    }, false);
    header.addEventListener('touchmove', function(e) {
        if (!carousel_is_dragging) return;
        var d = landscape_mq.matches
            ? e.touches[0].clientY - carousel_drag_start_y
            : e.touches[0].clientX - carousel_drag_start_x;
        if (Math.abs(d) > 4) { carousel_drag_moved = true; e.preventDefault(); }
        if (landscape_mq.matches) header.scrollTop  = carousel_scroll_start - d;
        else                       header.scrollLeft = carousel_scroll_start - d;
    }, {passive: false});
    header.addEventListener('touchend',    function() { carousel_is_dragging = false; }, false);
    header.addEventListener('touchcancel', function() { carousel_is_dragging = false; }, false);

    var last_landscape = landscape_mq.matches;
    window.addEventListener('resize', function() {
        draw();
        var now_landscape = landscape_mq.matches;
        if (now_landscape !== last_landscape) {
            last_landscape = now_landscape;
            if (images.length === 0) return;
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
            input.files[0].arrayBuffer().then(function(buf) { load_zip(buf); });
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
    fetch('Demo.zip')
        .then(function(r) { return r.arrayBuffer(); })
        .then(function(buf) { load_zip(buf); });
}
