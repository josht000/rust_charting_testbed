use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
const windows = new Map();
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
        header span{flex:1} button{border:0;background:transparent;color:#ddd;width:28px;height:26px;font-size:18px;cursor:pointer}
        button:hover{background:#5c3030} button:focus-visible{outline:1px solid #ddd}
        .plot{position:relative;flex:1;min-height:0}
        canvas{position:absolute;left:0;top:0;width:calc(100% - 56px);height:calc(100% - 18px);object-fit:fill}
        .price,.time{position:absolute;color:#bdbdbd;font:9px sans-serif;overflow:hidden}
        .price{right:0;top:0;width:55px;height:calc(100% - 18px);border-left:1px solid #414141}
        .time{left:0;bottom:0;width:calc(100% - 56px);height:17px;border-top:1px solid #414141}
        .zone{position:absolute;right:0;bottom:0;width:56px;height:18px;text-align:center;line-height:18px;color:#888;font:9px sans-serif}
        .price span,.time span{position:absolute;white-space:nowrap}
        .price span{left:4px;transform:translateY(-50%)}
        .price .current-price{position:absolute;left:0;right:0;height:16px;transform:translateY(-50%);background:#e68a00;color:#141414;font:bold 10px sans-serif;line-height:16px;text-align:center;clip-path:polygon(0 50%,7px 0,100% 0,100% 100%,7px 100%);padding-left:7px;box-sizing:border-box;overflow:hidden;white-space:nowrap}
        .time span{top:3px;transform:translateX(-50%)}</style></head>
        <body><header><span></span><button aria-label="Close chart" title="Close chart">&#215;</button></header><div class="plot"><canvas></canvas><div class="price"></div><div class="time"></div><div class="zone">UTC</div></div></body></html>`);
    popup.document.close();
    popup.document.querySelector("button").addEventListener("click", () => popup.close());
    popup.addEventListener("resize", () => layout_ticks(popup));
    windows.set(id, popup);
    return true;
}

function layout_ticks(popup) {
    for (const [selector, dimension, spacing] of [[".time", "clientWidth", 90], [".price", "clientHeight", 22]]) {
        const axis = popup.document.querySelector(selector);
        const labels = axis.querySelectorAll("span");
        const step = Math.max(1, Math.ceil(labels.length * spacing / Math.max(1, axis[dimension])));
        const marker = selector === ".price" ? axis.querySelector(".current-price") : null;
        const markerY = marker ? parseFloat(marker.style.top) * axis.clientHeight / 100 : null;
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

export function chart_popup_update(id, symbol, pixels, width, height, timeTicks, priceTicks, currentPrice) {
    if (!chart_popup_is_open(id)) return;
    const popup = windows.get(id);
    popup.document.title = `${symbol} - DigiTrader chart`;
    popup.document.querySelector("header span").textContent = `${symbol}  |  Sample chart`;
    const canvas = popup.document.querySelector("canvas");
    canvas.width = width;
    canvas.height = height;
    canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(pixels), width, height), 0, 0);
    for (const [selector, ticks, coordinate] of [
        [".time", JSON.parse(timeTicks), "left"],
        [".price", JSON.parse(priceTicks), "top"]
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
    if (marker) {
        const label = popup.document.createElement("div");
        label.className = "current-price";
        label.textContent = marker.label;
        label.style.top = `${marker.position * 100}%`;
        popup.document.querySelector(".price").appendChild(label);
    }
    layout_ticks(popup);
}

"#)]
extern "C" {
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
    );
}
