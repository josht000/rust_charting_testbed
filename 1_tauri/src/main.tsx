import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App.tsx';
import './index.css';

// Benchmark epoch: earliest measurable point of the page's script execution.
(window as unknown as { __t0: number }).__t0 = performance.now();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
