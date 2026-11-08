use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r##"
const windows = new Map();
let styleHandler = null;
let optionHandler = null;
let zoomHandler = null;
let actionHandler = null;
let pointerHandler = null;
let resizeHandler = null;
export function chart_popup_set_style_handler(handler) { styleHandler = handler; }
export function chart_popup_set_option_handler(handler) { optionHandler = handler; }
export function chart_popup_set_zoom_handler(handler) { zoomHandler = handler; }
export function chart_popup_set_action_handler(handler) { actionHandler = handler; }
export function chart_popup_set_pointer_handler(handler) { pointerHandler = handler; }
export function chart_popup_set_resize_handler(handler) { resizeHandler = handler; }
export function chart_popup_request_resize(id) {
    if (!chart_popup_is_open(id)) return;
    const popup = windows.get(id), rect = popup.document.querySelector("canvas").getBoundingClientRect();
    resizeHandler(id, rect.width, rect.height, popup.devicePixelRatio || 1);
}
window.addEventListener("pagehide", () => {
    for (const popup of windows.values()) if (!popup.closed) popup.close();
    windows.clear();
});

export function chart_popup_open(id) {
    let popup = windows.get(id);
    if (popup && !popup.closed) { popup.focus(); return true; }
    popup = window.open("", `digitrader-chart-${id}`, "popup,width=440,height=300");
    if (!popup) return false;
    popup.document.open();
    popup.document.write(`<!doctype html><html><head><title>DigiTrader chart</title>
        <style>html,body{margin:0;width:100%;height:100%;background:#111;color:#e8e8e8;font:12px sans-serif}
        body{display:flex;flex-direction:column} header{height:30px;padding:0 6px 0 8px;background:#191919;display:flex;align-items:center;gap:6px;flex-shrink:0}
        header input{flex:1;min-width:30px;background:#1d1d1d;border:1px solid #444;color:#eee;font:bold 12px sans-serif}
        header .link{width:42px;font:10px sans-serif;background:#313131}
        header .link.selected{background:#1c3e3a}
        button{border:0;background:transparent;color:#ddd;width:28px;height:26px;font-size:18px;cursor:pointer}
        .tools{height:27px;display:flex;gap:2px;padding:0 4px;flex-shrink:0}
        .tools button{flex:1;min-width:0;width:auto;font:9px sans-serif;background:#252525}
        .tools button.selected{background:#1c3e3a}
        .status{height:16px;flex-shrink:0;color:#bbb;font:9px sans-serif;overflow:hidden}
        button:hover{background:#5c3030} button:focus-visible{outline:1px solid #ddd}
        .plot{position:relative;flex:1;min-height:0}
        canvas{position:absolute;left:0;top:0;width:calc(100% - 56px);height:calc(100% - 18px);object-fit:fill}
        .price,.time{position:absolute;color:#bdbdbd;font:9px sans-serif;overflow:hidden}
        .price{right:0;top:0;width:55px;height:calc(100% - 18px);border-left:1px solid #414141}
        .time{left:0;bottom:0;width:calc(100% - 56px);height:17px;border-top:1px solid #414141}
        .zone{position:absolute;right:0;bottom:0;width:56px;height:18px;text-align:center;line-height:18px;color:#888;font:9px sans-serif}
        .price span,.time span{position:absolute;white-space:nowrap}
        .price span{left:4px;transform:translateY(-50%)}
        .price .current-price{position:absolute;left:0;width:48px;height:14px;transform:translateY(-50%);background:#fff;color:#141414;font:bold 9px sans-serif;line-height:14px;text-align:center;clip-path:polygon(0 50%,6px 0,100% 0,100% 100%,6px 100%);padding-left:6px;box-sizing:border-box;overflow:hidden;white-space:nowrap}
        .price .current-price::after{content:"";position:absolute;z-index:-1;inset:1px;background:var(--marker-color);clip-path:polygon(0 50%,5px 0,100% 0,100% 100%,5px 100%)}
        .price .current-price.offscreen{clip-path:none}
        .price .current-price.offscreen::after{clip-path:none}
        .price .current-price.offscreen::before{content:"";position:absolute;left:5px;top:4px;border-left:3.5px solid transparent;border-right:3.5px solid transparent}
        .price .current-price.offscreen{padding-left:12px}
        .price .current-price.above::before{border-bottom:7px solid #141414}
        .price .current-price.below::before{border-top:7px solid #141414}
        .style-editor{position:absolute;right:56px;top:4px;width:205px;max-height:calc(100% - 26px);overflow:auto;padding:8px;z-index:2;background:#242424;border:1px solid #777;border-radius:4px}
        .style-editor[hidden]{display:none}.style-editor strong{display:block;font-size:10px;margin-bottom:6px}
        .style-editor .colors,.style-editor .patterns{display:flex;gap:4px;margin-bottom:5px}
        .style-editor button{width:auto;height:23px;border:1px solid #777;color:#eee;font:9px sans-serif;padding:0 6px}
        .style-editor .colors button{width:27px}.style-editor button.selected{border:2px solid white}
        .time span{top:3px;transform:translateX(-50%)}</style></head>
        <body><header><input aria-label="Chart symbol"><button class="link">SOLO</button><button class="close" aria-label="Close chart" title="Close chart">&#215;</button></header><div class="tools"></div><div class="plot"><canvas></canvas><div class="price"></div><div class="time"></div><div class="zone">UTC</div><div class="style-editor" hidden><strong>PRICE LINE STYLE</strong><div class="colors"></div><div class="patterns"></div><div class="options"></div></div></div><div class="status">Ready</div></body></html>`);
    popup.document.close();
    popup.document.querySelector(".close").addEventListener("click", () => popup.close());
    popup.addEventListener("resize", () => { layout_ticks(popup); chart_popup_request_resize(id); });
    const status = popup.document.querySelector(".status");
    const input = popup.document.querySelector("input");
    input.addEventListener("focus", () => input.select());
    input.addEventListener("blur", () => { input.value = input.dataset.symbol; });
    input.addEventListener("keydown", event => {
        if (event.key === "Escape") { input.value = input.dataset.symbol; input.blur(); }
        if (event.key === "Enter") { status.textContent = actionHandler(id, "symbol", input.value); input.value = input.dataset.symbol; }
    });
    popup.document.querySelector(".link").addEventListener("click", () => { status.textContent = actionHandler(id, "link", ""); });
    for (const [label, name] of [["CUR","Cursor"],["TREND","Trend line"],["HORIZ","Horizontal line"],["VERT","Vertical line"],["MEAS","Measure"],["CROSS","Crosshair"],["RAY","Ray"],["ERASE","Eraser"]]) {
        const button = popup.document.createElement("button");
        button.textContent = label; button.title = name; button.dataset.tool = name;
        button.addEventListener("click", () => { status.textContent = actionHandler(id, "tool", name); });
        popup.document.querySelector(".tools").appendChild(button);
    }
    const editor = popup.document.querySelector(".style-editor");
    const colors = {orange:"#e68a00", teal:"#26a69a", red:"#ef5350", white:"#dcdcdc"};
    for (const [name, value] of Object.entries(colors)) {
        const button = popup.document.createElement("button");
        button.title = name; button.dataset.color = name; button.style.background = value;
        button.addEventListener("click", () => styleHandler(id, name, editor.dataset.pattern));
        editor.querySelector(".colors").appendChild(button);
    }
    for (const name of ["dashed", "solid", "dotted"]) {
        const button = popup.document.createElement("button");
        button.textContent = name; button.dataset.pattern = name;
        button.addEventListener("click", () => styleHandler(id, editor.dataset.color, name));
        editor.querySelector(".patterns").appendChild(button);
    }
    for (const [title, field, values] of [
        ["Visibility", "visible", ["true", "false"]],
        ["Border", "border", ["white", "color", "none"]],
        ["Thickness", "thickness", ["1", "2", "3"]],
        ["Glow color", "glow_color", ["orange", "teal", "red", "white", "black"]],
        ["Glow intensity", "glow", ["0", "1", "2"]],
        ["Shadow color", "shadow_color", ["orange", "teal", "red", "white", "black"]],
        ["Shadow intensity", "shadow", ["0", "1", "2"]],
        ["Y axis labels", "labels", ["true", "false"]],
        ["Y axis grid", "grid", ["true", "false"]]
    ]) {
        const label = popup.document.createElement("strong");
        label.textContent = title;
        const row = popup.document.createElement("div");
        row.className = "patterns";
        for (const value of values) {
            const button = popup.document.createElement("button");
            button.dataset.field = field; button.dataset.value = value;
            button.textContent = value === "true" ? "on" : value === "false" ? "off" : value;
            button.addEventListener("click", () => optionHandler(id, field, value));
            row.appendChild(button);
        }
        editor.querySelector(".options").append(label, row);
    }
    popup.document.querySelector(".price").addEventListener("contextmenu", event => {
        event.preventDefault(); editor.hidden = false;
    });
    popup.document.querySelector("canvas").addEventListener("contextmenu", event => {
        const marker = popup.document.querySelector(".current-price");
        const rect = event.currentTarget.getBoundingClientRect();
        if (marker && !marker.classList.contains("offscreen") && Math.abs(event.clientY - rect.top - Number(marker.dataset.position) * rect.height) < 6) {
            event.preventDefault(); editor.hidden = false;
        }
    });
    popup.document.querySelector("canvas").addEventListener("wheel", event => {
        const rect = event.currentTarget.getBoundingClientRect();
        if (rect.width <= 0 || !event.deltaY) return;
        event.preventDefault();
        const multiplier = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? rect.height : 1;
        zoomHandler(id, -event.deltaY * multiplier, (event.clientX - rect.left) / rect.width);
    }, {passive: false});
    const canvas = popup.document.querySelector("canvas");
    function pointer(event, pressed) {
        const rect = canvas.getBoundingClientRect();
        if (rect.width > 0 && rect.height > 0) {
            const result = pointerHandler(id, (event.clientX - rect.left) / rect.width, (event.clientY - rect.top) / rect.height, pressed);
            if (result) status.textContent = result;
        }
    }
    canvas.addEventListener("pointerdown", event => {
        if (event.button !== 0) return;
        canvas.setPointerCapture(event.pointerId);
        pointer(event, true);
    });
    canvas.addEventListener("pointermove", event => { if (canvas.hasPointerCapture(event.pointerId)) pointer(event, true); });
    canvas.addEventListener("pointerup", event => { if (event.button === 0) pointer(event, false); });
    canvas.addEventListener("pointercancel", event => pointer(event, false));
    popup.document.addEventListener("pointerdown", event => {
        if (!editor.contains(event.target) && event.button !== 2) editor.hidden = true;
    });
    popup.document.addEventListener("keydown", event => {
        if (event.key === "Escape") editor.hidden = true;
    });
    windows.set(id, popup);
    return true;
}

function layout_ticks(popup) {
    for (const [selector, dimension, spacing] of [[".time", "clientWidth", 90], [".price", "clientHeight", 22]]) {
        const axis = popup.document.querySelector(selector);
        const labels = axis.querySelectorAll("span");
        const step = Math.max(1, Math.ceil(labels.length * spacing / Math.max(1, axis[dimension])));
        const marker = selector === ".price" ? axis.querySelector(".current-price") : null;
        const markerY = marker ? Math.max(Math.min(7, axis.clientHeight / 2), Math.min(axis.clientHeight - Math.min(7, axis.clientHeight / 2), Number(marker.dataset.position) * axis.clientHeight)) : null;
        if (marker) marker.style.top = `${markerY}px`;
        labels.forEach((label, index) => {
            const labelY = parseFloat(label.style.top) * axis.clientHeight / 100;
            label.hidden = index % step !== 0 || (markerY !== null && Math.abs(labelY - markerY) < 15);
        });
    }
}

export function chart_popup_is_open(id) {
    const popup = windows.get(id);
    if (!popup || popup.closed) { windows.delete(id); return false; }
    return true;
}

export function chart_popup_update(id, symbol, pixels, width, height, timeTicks, priceTicks, currentPrice, color, pattern, styleJson, linked, tool) {
    if (!chart_popup_is_open(id)) return;
    const popup = windows.get(id);
    popup.document.title = `${symbol} - DigiTrader chart`;
    const input = popup.document.querySelector("input");
    input.dataset.symbol = symbol;
    if (popup.document.activeElement !== input) input.value = symbol;
    const link = popup.document.querySelector(".link");
    link.textContent = linked ? "LINK" : "SOLO"; link.classList.toggle("selected", linked);
    popup.document.querySelectorAll(".tools button").forEach(button => button.classList.toggle("selected", button.dataset.tool === tool));
    const editor = popup.document.querySelector(".style-editor");
    editor.dataset.color = color; editor.dataset.pattern = pattern;
    editor.querySelectorAll("button").forEach(button => button.classList.toggle("selected", button.dataset.color === color || button.dataset.pattern === pattern));
    const style = JSON.parse(styleJson);
    const options = {visible:style.visible, border:style.border, thickness:style.thickness, glow_color:style.glow_color, glow:style.glow_intensity, shadow_color:style.shadow_color, shadow:style.shadow_intensity, labels:style.labels_visible, grid:style.grid_visible};
    editor.querySelectorAll("button[data-field]").forEach(button => button.classList.toggle("selected", String(options[button.dataset.field]) === button.dataset.value));
    const canvas = popup.document.querySelector("canvas");
    canvas.width = width;
    canvas.height = height;
    canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(pixels), width, height), 0, 0);
    for (const [selector, ticks, coordinate] of [
        [".time", JSON.parse(timeTicks), "left"],
        [".price", style.labels_visible ? JSON.parse(priceTicks) : [], "top"]
    ]) {
        const axis = popup.document.querySelector(selector);
        axis.replaceChildren();
        for (const tick of ticks) {
            const label = popup.document.createElement("span");
            label.textContent = tick.label;
            label.style[coordinate] = `${tick.position * 100}%`;
            axis.appendChild(label);
        }
    }
    const marker = JSON.parse(currentPrice);
    if (marker && style.visible) {
        const label = popup.document.createElement("div");
        label.className = "current-price";
        if (marker.position < 0) label.classList.add("offscreen", "above");
        else if (marker.position >= 1) label.classList.add("offscreen", "below");
        label.textContent = marker.label;
        label.dataset.position = marker.position;
        label.style.setProperty("--marker-color", {orange:"#e68a00",teal:"#26a69a",red:"#ef5350",white:"#dcdcdc",black:"#000000"}[color]);
        label.style.background = style.border === "white" ? "white" : {orange:"#e68a00",teal:"#26a69a",red:"#ef5350",white:"#dcdcdc",black:"#000000"}[color];
        popup.document.querySelector(".price").appendChild(label);
    }
    layout_ticks(popup);
}

"##)]
extern "C" {
    pub fn chart_popup_set_style_handler(handler: &JsValue);
    pub fn chart_popup_set_option_handler(handler: &JsValue);
    pub fn chart_popup_set_zoom_handler(handler: &JsValue);
    pub fn chart_popup_set_action_handler(handler: &JsValue);
    pub fn chart_popup_set_pointer_handler(handler: &JsValue);
    pub fn chart_popup_set_resize_handler(handler: &JsValue);
    pub fn chart_popup_request_resize(id: u32);
    pub fn chart_popup_open(id: u32) -> bool;
    pub fn chart_popup_is_open(id: u32) -> bool;
    pub fn chart_popup_update(
        id: u32,
        symbol: &str,
        pixels: &[u8],
        width: u32,
        height: u32,
        time_ticks: &str,
        price_ticks: &str,
        current_price: &str,
        color: &str,
        pattern: &str,
        style: &str,
        linked: bool,
        tool: &str,
    );
}
