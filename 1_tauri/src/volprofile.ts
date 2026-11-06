/**
 * Horizontal volume profile as a Lightweight-Charts v5 series primitive
 * (`ISeriesPrimitive`). Draws 40 price bins as horizontal bars anchored to the
 * right edge of the pane at 50% blue opacity, on an independent (normalised)
 * scale — it does not participate in the price autoscale.
 */

import type { CanvasRenderingTarget2D } from 'fancy-canvas';
import type {
  IPrimitivePaneRenderer,
  IPrimitivePaneView,
  ISeriesApi,
  ISeriesPrimitive,
  PrimitivePaneViewZOrder,
  SeriesAttachedParameter,
  Time,
} from 'lightweight-charts';
import type { ProfileBin } from './types';

const FILL = 'rgba(30, 90, 255, 0.5)'; // blue @ 50% opacity
const EDGE_WIDTH_FRACTION = 0.18; // profile occupies right ~18% of the pane

class ProfileRenderer implements IPrimitivePaneRenderer {
  private readonly _bins: readonly ProfileBin[];
  private readonly _series: ISeriesApi<'Candlestick', Time>;

  constructor(bins: readonly ProfileBin[], series: ISeriesApi<'Candlestick', Time>) {
    this._bins = bins;
    this._series = series;
  }

  draw(target: CanvasRenderingTarget2D): void {
    target.useMediaCoordinateSpace((scope) => {
      const ctx = scope.context;
      if (this._bins.length === 0) return;
      let maxVol = 0;
      for (const b of this._bins) if (b.volume > maxVol) maxVol = b.volume;
      if (maxVol <= 0) return;

      const right = scope.mediaSize.width;
      const maxBarWidth = right * EDGE_WIDTH_FRACTION;
      ctx.fillStyle = FILL;
      for (const bin of this._bins) {
        const yTop = this._series.priceToCoordinate(bin.price_high);
        const yBot = this._series.priceToCoordinate(bin.price_low);
        if (yTop === null || yBot === null) continue;
        const y = Math.min(yTop, yBot);
        const h = Math.max(1, Math.abs(yBot - yTop) - 0.5);
        const w = (bin.volume / maxVol) * maxBarWidth;
        if (w <= 0) continue;
        ctx.fillRect(right - w, y, w, h);
      }
    });
  }
}

class ProfilePaneView implements IPrimitivePaneView {
  private _renderer: ProfileRenderer | null = null;
  private readonly _series: ISeriesApi<'Candlestick', Time>;
  private readonly _getBins: () => readonly ProfileBin[];

  constructor(series: ISeriesApi<'Candlestick', Time>, getBins: () => readonly ProfileBin[]) {
    this._series = series;
    this._getBins = getBins;
  }

  update(): void {
    this._renderer = new ProfileRenderer(this._getBins(), this._series);
  }

  renderer(): IPrimitivePaneRenderer | null {
    return this._renderer;
  }

  zOrder(): PrimitivePaneViewZOrder {
    // Above the candles but below the crosshair.
    return 'normal';
  }
}

export class VolumeProfilePrimitive implements ISeriesPrimitive<Time> {
  private _bins: ProfileBin[] = [];
  private _paneViews: ProfilePaneView[] = [];
  private _requestUpdate: (() => void) | null = null;

  setBins(bins: ProfileBin[]): void {
    this._bins = bins;
    this.updateAllViews();
    this._requestUpdate?.();
  }

  attached(param: SeriesAttachedParameter<Time, 'Candlestick'>): void {
    this._requestUpdate = param.requestUpdate;
    this._paneViews = [
      new ProfilePaneView(param.series as ISeriesApi<'Candlestick', Time>, () => this._bins),
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
