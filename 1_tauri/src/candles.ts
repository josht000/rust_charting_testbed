/**
 * Candle rendering as a Lightweight-Charts series primitive.
 *
 * Draws candles directly from the raw Bar[] array onto the pane canvas each
 * repaint — bypassing the per-bar series.update()/setData() path entirely.
 * Attached to an "anchor" LineSeries with `lineVisible:false`: the anchor
 * carries data to establish the time axis but draws nothing itself.
 *
 * X positions use logicalToCoordinate(i): logical indices are dense per data
 * point, and our bars are contiguous, so bar i maps to logical index i even
 * before the anchor series has been fed that bar.
 */

import type { CanvasRenderingTarget2D } from 'fancy-canvas';
import type {
  IPrimitivePaneRenderer,
  IPrimitivePaneView,
  ISeriesApi,
  ISeriesPrimitive,
  ITimeScaleApi,
  Logical,
  PrimitivePaneViewZOrder,
  SeriesAttachedParameter,
  Time,
} from 'lightweight-charts';
import type { Bar } from './types';

const UP = '#26a69a';
const DOWN = '#ef5350';

/** One indicator line drawn inside the primitive. */
export interface PrimitiveLine {
  color: string;
  width: number;
  data: readonly (number | null)[];
}

class CandleRenderer implements IPrimitivePaneRenderer {
  private readonly _ts: ITimeScaleApi<Time>;
  private readonly _series: ISeriesApi<'Line', Time>;
  private readonly _bars: readonly Bar[];
  private readonly _lines: readonly PrimitiveLine[];

  constructor(
    ts: ITimeScaleApi<Time>,
    series: ISeriesApi<'Line', Time>,
    bars: readonly Bar[],
    lines: readonly PrimitiveLine[],
  ) {
    this._ts = ts;
    this._series = series;
    this._bars = bars;
    this._lines = lines;
  }

  draw(target: CanvasRenderingTarget2D): void {
    target.useMediaCoordinateSpace((scope) => {
      const ctx = scope.context;
      const bars = this._bars;
      if (bars.length === 0) return;
      const range = this._ts.getVisibleLogicalRange();
      if (!range) return;

      const i0 = Math.max(0, Math.floor(range.from));
      const i1 = Math.min(bars.length - 1, Math.ceil(range.to));
      if (i1 < i0) return;

      // Linearized coordinate math: index->x and price->y are affine, so
      // two API conversions each give us O(1) math per bar — per-call
      // coordinate APIs at 23k bars x 8 charts made full-view pan/zoom
      // unusable.
      const xa = this._ts.logicalToCoordinate(i0 as Logical);
      const xb = this._ts.logicalToCoordinate((i0 + 1) as Logical);
      if (xa === null) return;
      const spacing = xb !== null ? Math.abs(xb - xa) : 4;
      const xAt = (i: number) => xa + (i - i0) * spacing;

      const refA = bars[i0].high;
      const refB = bars[i0].low;
      const yA = this._series.priceToCoordinate(refA);
      const yB = this._series.priceToCoordinate(refB);
      const scaleY =
        yA !== null && yB !== null && refA !== refB
          ? (yB - yA) / (refB - refA)
          : null;
      const yAt = (p: number): number | null =>
        scaleY !== null && yA !== null ? yA + (p - refA) * scaleY : null;

      // Per-pixel-column aggregation (same approach as the egui renderer):
      // when several bars land on one column, draw a single column candle —
      // O(pane width) draw calls instead of O(bars).
      const bw = Math.max(1, spacing * 0.7);
      let col = -Infinity;
      let bo = 0;
      let bh = -Infinity;
      let bl = Infinity;
      let bc = 0;
      const emit = () => {
        if (col === -Infinity) return;
        const yh = yAt(bh);
        const yl = yAt(bl);
        const yo = yAt(bo);
        const yc = yAt(bc);
        if (yh === null || yl === null || yo === null || yc === null) return;
        ctx.fillStyle = bc >= bo ? UP : DOWN;
        const xc = col + 0.5;
        ctx.fillRect(Math.round(xc) - 0.5, yh, 1, Math.max(1, yl - yh));
        ctx.fillRect(xc - bw / 2, Math.min(yo, yc), bw, Math.max(1, Math.abs(yc - yo)));
      };
      for (let i = i0; i <= i1; i++) {
        const c = Math.floor(xAt(i));
        const b = bars[i];
        if (c !== col) {
          emit();
          col = c;
          bo = b.open;
          bh = b.high;
          bl = b.low;
          bc = b.close;
        } else {
          if (b.high > bh) bh = b.high;
          if (b.low < bl) bl = b.low;
          bc = b.close;
        }
      }
      emit();

      // Indicator polylines, one vertex per pixel column (last value wins);
      // null/NaN breaks a segment (warmup gaps).
      for (const line of this._lines) {
        ctx.strokeStyle = line.color;
        ctx.lineWidth = line.width;
        ctx.beginPath();
        let pen = false;
        let curCol = -Infinity;
        let curY = 0;
        let penX = 0;
        for (let i = i0; i <= i1; i++) {
          const v = line.data[i];
          const c = Math.floor(xAt(i));
          if (v === null || v === undefined || !Number.isFinite(v)) {
            if (curCol !== -Infinity) {
              pen ? ctx.lineTo(penX, curY) : ctx.moveTo(curCol + 0.5, curY);
              pen = true;
            }
            pen = false;
            curCol = -Infinity;
            continue;
          }
          const y = yAt(v);
          if (y === null) continue;
          if (c !== curCol) {
            if (curCol !== -Infinity) {
              pen ? ctx.lineTo(penX, curY) : ctx.moveTo(curCol + 0.5, curY);
              pen = true;
            }
            curCol = c;
            penX = c + 0.5;
          }
          curY = y;
        }
        if (curCol !== -Infinity) {
          if (pen) ctx.lineTo(penX, curY);
          else ctx.moveTo(penX, curY);
        }
        ctx.stroke();
      }
    });
  }
}

class CandlePaneView implements IPrimitivePaneView {
  private _renderer: CandleRenderer | null = null;
  private readonly _ts: ITimeScaleApi<Time>;
  private readonly _series: ISeriesApi<'Line', Time>;
  private readonly _getBars: () => readonly Bar[];
  private readonly _getLines: () => readonly PrimitiveLine[];

  constructor(
    ts: ITimeScaleApi<Time>,
    series: ISeriesApi<'Line', Time>,
    getBars: () => readonly Bar[],
    getLines: () => readonly PrimitiveLine[],
  ) {
    this._ts = ts;
    this._series = series;
    this._getBars = getBars;
    this._getLines = getLines;
  }

  update(): void {
    this._renderer = new CandleRenderer(
      this._ts,
      this._series,
      this._getBars(),
      this._getLines(),
    );
  }

  renderer(): IPrimitivePaneRenderer | null {
    return this._renderer;
  }

  zOrder(): PrimitivePaneViewZOrder {
    return 'normal';
  }
}

export class CandlePrimitive implements ISeriesPrimitive<Time> {
  private _bars: Bar[] = [];
  private _lines: PrimitiveLine[] = [];
  private _paneViews: CandlePaneView[] = [];
  private _requestUpdate: (() => void) | null = null;

  /** Point the primitive at the (mutated in place) bars array. */
  setSource(bars: Bar[]): void {
    this._bars = bars;
    this.requestUpdate();
  }

  /** Point the primitive at the (mutated in place) indicator arrays. */
  setLines(lines: PrimitiveLine[]): void {
    this._lines = lines;
    this.requestUpdate();
  }

  requestUpdate(): void {
    this._requestUpdate?.();
  }

  attached(param: SeriesAttachedParameter<Time, 'Line'>): void {
    this._requestUpdate = param.requestUpdate;
    this._paneViews = [
      new CandlePaneView(
        param.chart.timeScale(),
        param.series as ISeriesApi<'Line', Time>,
        () => this._bars,
        () => this._lines,
      ),
    ];
    this.updateAllViews();
  }

  detached(): void {
    this._paneViews = [];
    this._requestUpdate = null;
  }

  updateAllViews(): void {
    for (const v of this._paneViews) v.update();
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return this._paneViews;
  }
}
