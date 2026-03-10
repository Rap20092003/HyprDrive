/**
 * HyprDrive Desktop — Tauri thin client
 *
 * This is NOT the system. This is a UI shell.
 * All intelligence lives in hyprdrive-daemon.
 * This app connects via WebSocket to :7420.
 */

import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';

const root = document.getElementById('root');
if (root) {
    ReactDOM.createRoot(root).render(
        <React.StrictMode>
            <App />
        </React.StrictMode>
    );
}
