import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import { PlatformProvider } from './platform';
import './styles.css';
import { initializeTheme } from './theme';

initializeTheme();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PlatformProvider><App /></PlatformProvider>
  </StrictMode>,
);
