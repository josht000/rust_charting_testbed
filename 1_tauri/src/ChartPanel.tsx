import { useEffect, useRef } from 'react';
import type { IDockviewPanelProps } from 'dockview';
import {
  LineSeries,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type UTCTimestamp,
} from 'lightweight-charts';
import type { Bar, Indicators } from './types';
import { computeIndicators, getInitialBars, getStreamRate, streamBars } from './data';
import { IncrementalIndicators } from './stream';
import { VolumeProfilePrimitive } from './volprofile';
import { CandlePrimitive } from './candles';

export interface ChartHandle {
  chartIdx: number;
  chart: IChartApi;
  /** The invisible anchor series carrying the time axis. */
  anchor: ISeriesApi<'Line'>;
  bars: Bar[];
  /** Push a live update (mutates/extends the last bar). */
  pushLive(bar: Bar): void;
  /** Resolves when the post-preload stream completes. */
  streamDone: Promise<void>;
  /** performance.now() when the stream started. */
  streamStartAt: number;
}

/** Registry used by the scripted benchmark (?bench=1). */
export const chartRegistry: (ChartHandle | undefined)[] = new Array(8);
export let firstChartReadyAt = -1;
// Debug/bench hook: puppeteer scripts drive charts through this.
(window as unknown as { __charts?: typeof chartRegistry }).__charts = chartRegistry;

const LINE_DEFS: { key: keyof Indicators; color: string; width: 1 | 2 }[] = [
  { key: 'ema10', color: '#ffffff', width: 1 },
  { key: 'ema20', color: '#ffa500', width: 1 },
  { key: 'ema50', color: '#00e5ff', width: 1 },
  { key: 'sma200', color: '#ffe600', width: 2 },
  { key: 'vwap', color: '#b44dff', width: 1 },
];

export function ChartPanel({ params }: IDockviewPanelProps<{ chart: number }>) {
  const chartIdx = params.chart;
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const chart = createChart(host, {
      layout: {
        background: { color: '#131722' },
        textColor: '#c9d1d9',
        attributionLogo: false,
      },
      grid: {
        vertLines: { color: '#1e2530' },
        horzLines: { color: '#1e2530' },
      },
      timeScale: { timeVisible: true, secondsVisible: true },
      width: host.clientWidth,
      height: host.clientHeight,
    });

    // "Anchor" series: carries data to establish the time axis but draws
    // nothing itself (lineVisible:false). Candles are drawn by
    // CandlePrimitive straight from the bars array — no per-bar
    // update()/setData() on any series during streaming.
    const anchor = chart.addSeries(LineSeries, {
      color: 'rgba(0,0,0,0)',
      lineVisible: false,
      priceLineVisible: false,
      lastValueVisible: false,
      crosshairMarkerVisible: false,
      autoscaleInfoProvider: () => null,
    });
    const candlePrim = new CandlePrimitive();
    anchor.attachPrimitive(candlePrim);
    const profile = new VolumeProfilePrimitive();
    anchor.attachPrimitive(profile);
    // Manual price scale: computed over the visible bars only (indicator
    // lines never stretch it — same behaviour as before).
    const priceApi = chart.priceScale('right');
    priceApi.applyOptions({ autoScale: false });

    let refitPending = false;
    const ro = new ResizeObserver(() => {
      chart.applyOptions({ width: host.clientWidth, height: host.clientHeight });
      if (refitPending && host.clientWidth > 50) {
        refitPending = false;
        chart.timeScale().fitContent();
      }
    });
    ro.observe(host);

    let cancelled = false;
    let lastBar: Bar | null = null;

    (async () => {
      // ---- static preload: first INITIAL_BARS bars ----
      const initBars = await getInitialBars(chartIdx);
      if (cancelled) return;
      lastBar = initBars[initBars.length - 1];
      const bars: Bar[] = [...initBars];
      // Anchor holds close-as-line data purely for the time axis; appended
      // lazily at stream end (logical indices extrapolate meanwhile).
      const anchorData = bars.map((b) => ({
        time: b.ts as UTCTimestamp,
        value: b.close,
      }));
      anchor.setData(anchorData);
      candlePrim.setSource(bars);

      // Manual autoscale over the currently visible bars.
      let priceLo = 0;
      let priceHi = 0;
      const recalcPrice = () => {
        const r = chart.timeScale().getVisibleLogicalRange();
        if (!r || bars.length === 0) return;
        const i0 = Math.max(0, Math.floor(r.from));
        const i1 = Math.min(bars.length - 1, Math.ceil(r.to));
        let lo = Infinity;
        let hi = -Infinity;
        for (let i = i0; i <= i1; i++) {
          const b = bars[i];
          if (b.low < lo) lo = b.low;
          if (b.high > hi) hi = b.high;
        }
        if (!Number.isFinite(lo) || !Number.isFinite(hi)) return;
        const pad = (hi - lo) * 0.05 || 0.01;
        priceLo = lo - pad;
        priceHi = hi + pad;
        priceApi.setVisibleRange({ from: priceLo, to: priceHi });
      };
      // Subscription lives for the chart's lifetime (dies with chart.remove()).
      chart.timeScale().subscribeVisibleLogicalRangeChange(recalcPrice);

      const ind = await computeIndicators(chartIdx, initBars, initBars.length);
      if (cancelled) return;
      // Indicator lines are drawn inside the candle primitive from these
      // per-bar arrays — no LWC line series at all (series updates created
      // sparse time points that broke the primitive's index mapping).
      const lineData: Record<string, (number | null)[]> = {};
      for (const def of LINE_DEFS) {
        lineData[def.key] = [...(ind[def.key] as (number | null)[])];
      }
      candlePrim.setLines(
        LINE_DEFS.map((d) => ({ color: d.color, width: d.width, data: lineData[d.key] })),
      );
      profile.setBins(ind.profile);
      chart.timeScale().fitContent();
      recalcPrice();
      // Panel may not have its final size yet; refit once it does.
      refitPending = true;

      // Incremental indicator state, seeded by replaying the preload so it
      // is identical to the batch computation just rendered.
      const inc = new IncrementalIndicators(initBars);

      let resolveStreamDone!: () => void;
      let rejectStreamDone!: (e: unknown) => void;
      const streamDonePromise = new Promise<void>((res, rej) => {
        resolveStreamDone = res;
        rejectStreamDone = rej;
      });
      const streamStartAt = performance.now();

      chartRegistry[chartIdx] = {
        chartIdx,
        chart,
        anchor,
        bars,
        streamDone: streamDonePromise,
        streamStartAt,
        pushLive: (bar: Bar) => {
          // Primitive draws straight from `bars` — no series calls at all.
          // Indicator state stays live: appends feed inc.pushBar, mutations
          // of the forming bar recompute the last point via inc.updateLast.
          if (lastBar && bar.ts <= lastBar.ts) {
            lastBar.high = Math.max(lastBar.high, bar.high);
            lastBar.low = Math.min(lastBar.low, bar.low);
            lastBar.close = bar.close;
            lastBar.volume = bar.volume;
            const v = inc.updateLast(lastBar);
            const i = bars.length - 1;
            lineData.ema10[i] = v.ema10;
            lineData.ema20[i] = v.ema20;
            lineData.ema50[i] = v.ema50;
            lineData.sma200[i] = v.sma200;
            lineData.vwap[i] = v.vwap;
          } else {
            lastBar = bar;
            bars.push(bar);
            anchorData.push({ time: bar.ts as UTCTimestamp, value: bar.close });
            const v = inc.pushBar(bar);
            lineData.ema10.push(v.ema10);
            lineData.ema20.push(v.ema20);
            lineData.ema50.push(v.ema50);
            lineData.sma200.push(v.sma200);
            lineData.vwap.push(v.vwap);
          }
          candlePrim.requestUpdate();
          if (bar.low < priceLo || bar.high > priceHi) recalcPrice();
        },
      };
      if (firstChartReadyAt < 0) {
        requestAnimationFrame(() => {
          if (firstChartReadyAt < 0) firstChartReadyAt = performance.now();
        });
      }

      // ---- stream the remaining bars ----
      const rate = await getStreamRate();
      // During catch-up keep only the tail visible: repainting thousands of
      // candles x8 charts per frame saturates the main thread and starves
      // the stream timers. A ~500-bar window keeps each repaint cheap;
      // fitContent() runs when the stream ends.
      const TAIL = 500;
      // Display coalescing: bars + indicator values accumulate per stream
      // batch; view work only happens every FLUSH_MS — profile recompute, a
      // price recalc, and a primitive repaint request. The primitive draws
      // candles AND indicator lines straight from `bars`/`lineData`, so no
      // LWC series is touched during the stream (series updates create
      // sparse time points that break the index↔bar alignment).
      const FLUSH_MS = 200;
      let pendingBars = 0;
      let lastFlush = performance.now() - FLUSH_MS + chartIdx * 25;
      const flush = () => {
        if (pendingBars === 0) return;
        pendingBars = 0;
        lastFlush = performance.now();
        profile.setBins(inc.updateProfile());
        candlePrim.requestUpdate();
        if (bars.length > TAIL) {
          // Logical range: index == bar index (no sparse series updates), so
          // this is exact and immune to unmapped-time clamping. The range
          // change triggers the subscribed recalcPrice — no explicit call.
          chart.timeScale().setVisibleLogicalRange({
            from: bars.length - TAIL,
            to: bars.length + 15,
          });
        } else {
          recalcPrice();
        }
      };
      streamBars(chartIdx, rate, (batch) => {
        if (cancelled) return;
        for (const bar of batch) {
          lastBar = bar;
          bars.push(bar);
          pendingBars++;
          anchorData.push({ time: bar.ts as UTCTimestamp, value: bar.close });
          const v = inc.pushBar(bar);
          lineData.ema10.push(v.ema10);
          lineData.ema20.push(v.ema20);
          lineData.ema50.push(v.ema50);
          lineData.sma200.push(v.sma200);
          lineData.vwap.push(v.vwap);
        }
        if (performance.now() - lastFlush >= FLUSH_MS) flush();
      })
        .then(() => {
          if (cancelled) return;
          pendingBars = 0;
          // Exact final state: anchor catches up to the full day for the
          // time axis/crosshair. lineData already holds every incremental
          // point — the primitive draws it as-is.
          anchor.setData(anchorData);
          profile.setBins(inc.updateProfile());
          candlePrim.requestUpdate();
          chart.timeScale().fitContent();
          resolveStreamDone();
        }, rejectStreamDone);
    })().catch((e) => console.error(`chart${chartIdx} load failed`, e));

    return () => {
      cancelled = true;
      ro.disconnect();
      chartRegistry[chartIdx] = undefined;
      chart.remove();
    };
  }, [chartIdx]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', background: '#131722' }}>
      <div style={{ padding: '2px 8px', fontSize: 12, color: '#8b949e' }}>Chart {chartIdx}</div>
      <div ref={hostRef} style={{ flex: 1, minHeight: 0 }} />
    </div>
  );
}
