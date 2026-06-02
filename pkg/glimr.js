/* @ts-self-types="./glimr.d.ts" */

export class GlimrRenderer {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        GlimrRendererFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_glimrrenderer_free(ptr, 0);
    }
    /**
     * Draw image at `index` onto the photo canvas.
     * `offset` is the slide drag offset in CSS pixels:
     *   > 0 → dragging right (prev image enters from left)
     *   < 0 → dragging left  (next image enters from right)
     * @param {number} index
     * @param {number} offset
     */
    draw(index, offset) {
        const ret = wasm.glimrrenderer_draw(this.__wbg_ptr, index, offset);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Draws the `<` / `>` hover arrow directly onto `self.canvas` (on top of the blitted image).
     * `zone`    — "left", "right", or "" (no-op).
     * `opacity` — current animation opacity (0.0–1.0); no-op if ≤ 0.
     * `index`   — current image index; used to show `>>` / `<<` at gallery boundaries.
     * @param {number} index
     * @param {string} zone
     * @param {number} opacity
     */
    draw_hover_indicator(index, zone, opacity) {
        const ptr0 = passStringToWasm0(zone, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.glimrrenderer_draw_hover_indicator(this.__wbg_ptr, index, ptr0, len0, opacity);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Draws a thumbnail for image `index` into a caller-supplied canvas element.
     * Sets canvas width/height to match the scaled dimensions, then blits.
     * `carousel_size` — target size in CSS px on the constrained axis.
     * `fit_to_width`  — true in landscape (vertical strip); false in portrait (horizontal strip).
     * @param {HTMLCanvasElement} canvas
     * @param {number} index
     * @param {number} carousel_size
     * @param {boolean} fit_to_width
     */
    draw_thumbnail(canvas, index, carousel_size, fit_to_width) {
        const ret = wasm.glimrrenderer_draw_thumbnail(this.__wbg_ptr, canvas, index, carousel_size, fit_to_width);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Renders image `index` in zoom/pan mode.
     * `scale`  — zoom factor (1.0 = 1:1 pixels, fit_scale = fully zoomed out)
     * `pan_x/y` — top-left corner of the viewport window in image-space pixels
     * @param {number} index
     * @param {number} scale
     * @param {number} pan_x
     * @param {number} pan_y
     */
    draw_zoomed(index, scale, pan_x, pan_y) {
        const ret = wasm.glimrrenderer_draw_zoomed(this.__wbg_ptr, index, scale, pan_x, pan_y);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {number}
     */
    image_count() {
        const ret = wasm.glimrrenderer_image_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Size of the stored (XOR-decoded) JPEG/PNG bytes for image i.
     * @param {number} i
     * @returns {number}
     */
    image_file_size(i) {
        const ret = wasm.glimrrenderer_image_file_size(this.__wbg_ptr, i);
        return ret >>> 0;
    }
    /**
     * Decoded pixel height; 0 if image i has not been drawn yet.
     * @param {number} i
     * @returns {number}
     */
    image_height(i) {
        const ret = wasm.glimrrenderer_image_height(this.__wbg_ptr, i);
        return ret >>> 0;
    }
    /**
     * @param {number} i
     * @returns {string}
     */
    image_name(i) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.glimrrenderer_image_name(this.__wbg_ptr, i);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Decoded pixel width; 0 if image i has not been drawn yet.
     * @param {number} i
     * @returns {number}
     */
    image_width(i) {
        const ret = wasm.glimrrenderer_image_width(this.__wbg_ptr, i);
        return ret >>> 0;
    }
    /**
     * @param {Uint8Array} zip_bytes
     */
    load_zip(zip_bytes) {
        const ptr0 = passArray8ToWasm0(zip_bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.glimrrenderer_load_zip(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {HTMLCanvasElement} canvas
     * @param {HTMLCanvasElement} backing
     */
    constructor(canvas, backing) {
        const ret = wasm.glimrrenderer_new(canvas, backing);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        GlimrRendererFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Returns the XOR-decoded JPEG/PNG bytes for image i.
     * JS uses this for the download button (one-shot blob URL, revoked immediately after click).
     * @param {number} i
     * @returns {Uint8Array}
     */
    raw_bytes(i) {
        const ret = wasm.glimrrenderer_raw_bytes(this.__wbg_ptr, i);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
}
if (Symbol.dispose) GlimrRenderer.prototype[Symbol.dispose] = GlimrRenderer.prototype.free;

/**
 * Exported so JS can emit timestamped log lines in the same format.
 * @param {string} func
 * @param {string} msg
 */
export function glimr_log(func, msg) {
    const ptr0 = passStringToWasm0(func, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(msg, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    wasm.glimr_log(ptr0, len0, ptr1, len1);
}

/**
 * @param {Uint8Array} input
 * @returns {Uint8Array}
 */
export function xor_decode(input) {
    const ptr0 = passArray8ToWasm0(input, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.xor_decode(ptr0, len0);
    var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v2;
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_is_undefined_67b456be8673d3d7: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_throw_1506f2235d1bdba0: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_clientHeight_a7262e398342b986: function(arg0) {
            const ret = arg0.clientHeight;
            return ret;
        },
        __wbg_clientWidth_df70d49cc7ae3e15: function(arg0) {
            const ret = arg0.clientWidth;
            return ret;
        },
        __wbg_createElement_c3c16a9aa7f5cc74: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.createElement(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_document_aceb08cd6489baf5: function(arg0) {
            const ret = arg0.document;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_drawImage_32f6ef2b99e34e96: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.drawImage(arg1, arg2, arg3, arg4, arg5);
        }, arguments); },
        __wbg_drawImage_5672faddb3fa4b97: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9) {
            arg0.drawImage(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9);
        }, arguments); },
        __wbg_drawImage_f2104e9f31510175: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            arg0.drawImage(arg1, arg2, arg3);
        }, arguments); },
        __wbg_fillRect_3916c35e6834cd1b: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.fillRect(arg1, arg2, arg3, arg4);
        },
        __wbg_fillText_9a0aa2acf11b7d03: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
        }, arguments); },
        __wbg_getContext_469d34698d869fc1: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getHours_91ac680ae491b8ea: function(arg0) {
            const ret = arg0.getHours();
            return ret;
        },
        __wbg_getMilliseconds_5fa104a44b95037e: function(arg0) {
            const ret = arg0.getMilliseconds();
            return ret;
        },
        __wbg_getMinutes_c1c2573becc0c7b5: function(arg0) {
            const ret = arg0.getMinutes();
            return ret;
        },
        __wbg_getSeconds_2716239745eecd0a: function(arg0) {
            const ret = arg0.getSeconds();
            return ret;
        },
        __wbg_instanceof_CanvasRenderingContext2d_6f2951bc60fb6d97: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_HtmlCanvasElement_8325b7578cc1684c: function(arg0) {
            let result;
            try {
                result = arg0 instanceof HTMLCanvasElement;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Window_e093be59ee9a8e14: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_log_c7cfed94ca1b778f: function(arg0, arg1) {
            console.log(getStringFromWasm0(arg0, arg1));
        },
        __wbg_new_0_445c13a750296eb6: function() {
            const ret = new Date();
            return ret;
        },
        __wbg_new_with_u8_clamped_array_and_sh_e3609225f4ad3a74: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = new ImageData(getClampedArrayU8FromWasm0(arg0, arg1), arg2 >>> 0, arg3 >>> 0);
            return ret;
        }, arguments); },
        __wbg_putImageData_9118a61bc5ed588d: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            arg0.putImageData(arg1, arg2, arg3);
        }, arguments); },
        __wbg_restore_35d33f272777a212: function(arg0) {
            arg0.restore();
        },
        __wbg_save_e8d837ef098cd6fc: function(arg0) {
            arg0.save();
        },
        __wbg_set_fillStyle_e96ce0e2e6d0bd13: function(arg0, arg1, arg2) {
            arg0.fillStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_font_3700823e473a6204: function(arg0, arg1, arg2) {
            arg0.font = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_globalAlpha_8ed66eb71dbee04d: function(arg0, arg1) {
            arg0.globalAlpha = arg1;
        },
        __wbg_set_height_0739170de8653cc4: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_shadowBlur_9a876dbc0f0e0e7d: function(arg0, arg1) {
            arg0.shadowBlur = arg1;
        },
        __wbg_set_shadowColor_1b8fb4690ca58666: function(arg0, arg1, arg2) {
            arg0.shadowColor = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_shadowOffsetX_d2689cb0ee49a06f: function(arg0, arg1) {
            arg0.shadowOffsetX = arg1;
        },
        __wbg_set_shadowOffsetY_21d7b928ae23beec: function(arg0, arg1) {
            arg0.shadowOffsetY = arg1;
        },
        __wbg_set_textAlign_2cf46493669911fa: function(arg0, arg1, arg2) {
            arg0.textAlign = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_textBaseline_5a3fa8559d95dd95: function(arg0, arg1, arg2) {
            arg0.textBaseline = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_width_87301412247f3343: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_static_accessor_GLOBAL_9d53f2689e622ca1: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_a1a35cec07001a8a: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_4c59f6c7ea29a144: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_e70ae9f2eb052253: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./glimr_bg.js": import0,
    };
}

const GlimrRendererFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_glimrrenderer_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

function getClampedArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ClampedArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

let cachedUint8ClampedArrayMemory0 = null;
function getUint8ClampedArrayMemory0() {
    if (cachedUint8ClampedArrayMemory0 === null || cachedUint8ClampedArrayMemory0.byteLength === 0) {
        cachedUint8ClampedArrayMemory0 = new Uint8ClampedArray(wasm.memory.buffer);
    }
    return cachedUint8ClampedArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedUint8ArrayMemory0 = null;
    cachedUint8ClampedArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('glimr_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
