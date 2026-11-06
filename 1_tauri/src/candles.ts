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

      // Bar spacing from consecutive logical coordinates.
      const xa = this._ts.logicalToCoordinate(i0 as Logical);
      const xb = this._ts.logicalToCoordinate((i0 + 1) as Logical);
      const spacing = xa !== null && xb !== null ? Math.abs(xb - xa) : 4;
      const bodyW = Math.max(1, spacing * 0.7);

      for (let i = i0; i <= i1; i++) {
        const b = bars[i];
        const xc = this._ts.logicalToCoordinate(i as Logical);
        if (xc === null) continue;
        const yh = this._series.priceToCoordinate(b.high);
        const yl = this._series.priceToCoordinate(b.low);
        const yo = this._series.priceToCoordinate(b.open);
        const yc = this._series.priceToCoordinate(b.close);
        if (yh === null || yl === null || yo === null || yc === null) continue;
        ctx.fillStyle = b.close >= b.open ? UP : DOWN;
        // wick (1px, pixel-snapped)
        const xw = Math.round(xc) - 0.5;
        ctx.fillRect(xw, Math.min(yh, yl), 1, Math.max(1, Math.abs(yl - yh)));
        // body
        const yTop = Math.min(yo, yc);
        ctx.fillRect(
          xc - bodyW / 2,
          yTop,
          bodyW,
          Math.max(1, Math.abs(yc - yo)),
        );
      }

      // Indicator polylines over the same visible range; null/NaN breaks a
      // segment (warmup gaps).
      for (const line of this._lines) {
        ctx.strokeStyle = line.color;
        ctx.lineWidth = line.width;
        ctx.beginPath();
        let pen = false;
        for (let i = i0; i <= i1; i++) {
          const v = line.data[i];
          if (v === null || v === undefined || !Number.isFinite(v)) {
            pen = false;
            continue;
          }
          const xc = this._ts.logicalToCoordinate(i as Logical);
          const y = this._series.priceToCoordinate(v);
          if (xc === null || y === null) continue;
          if (pen) ctx.lineTo(xc, y);
          else ctx.moveTo(xc, y);
          pen = true;
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
