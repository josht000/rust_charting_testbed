import { DockviewReact, themeDark, type DockviewReadyEvent, type IDockviewPanelProps } from 'dockview-react';
import 'dockview-react/dist/styles/dockview.css';
import { ChartPanel } from './ChartPanel';
import { NUM_CHARTS } from './types';
import { runBench } from './bench';
import { benchRequested } from './data';

const components: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  chart: ChartPanel as React.FunctionComponent<IDockviewPanelProps>,
};

function onReady(event: DockviewReadyEvent) {
  const api = event.api;
  // 4 columns x 2 rows dockable layout.
  api.addPanel({ id: 'chart0', component: 'chart', title: 'Chart 0', params: { chart: 0 } });
  for (let i = 1; i < 4; i++) {
    api.addPanel({
      id: `chart${i}`,
      component: 'chart',
      title: `Chart ${i}`,
      params: { chart: i },
      position: { referencePanel: `chart${i - 1}`, direction: 'right' },
    });
  }
  // chart4 starts a new full-width row below the whole top row (no
  // referencePanel = split the root, not a single column's space).
  for (let i = 4; i < NUM_CHARTS; i++) {
    api.addPanel({
      id: `chart${i}`,
      component: 'chart',
      title: `Chart ${i}`,
      params: { chart: i },
      position: i === 4
        ? { direction: 'below' }
        : { referencePanel: `chart${i - 1}`, direction: 'right' },
    });
  }

  // Nested splits leave uneven sizes; force an even 4x2 grid.
  const w = Math.floor(api.width / 4);
  const h = Math.floor(api.height / 2);
  for (let i = 0; i < NUM_CHARTS; i++) {
    api.getPanel(`chart${i}`)?.api.setSize({ width: w, height: h });
  }

  api.getPanel('chart0')?.api.setActive();

  // Scripted benchmark: ?bench=1 on web, or `bench_requested` in Tauri.
  const params = new URLSearchParams(window.location.search);
  const benchNow = params.get('bench') === '1';
  (async () => {
    const go = benchNow || (await benchRequested());
    if (go) {
      runBench().catch((e) => console.error('bench failed', e));
    }
  })();
}

export default function App() {
  return (
    <div style={{ height: '100vh', width: '100vw' }}>
      <DockviewReact
        components={components}
        onReady={onReady}
        theme={themeDark}
      />
    </div>
  );
}
