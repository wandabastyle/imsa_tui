import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app';
import './app.css';

const rootElement = document.querySelector('#app');
if (rootElement) {
  createRoot(rootElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
