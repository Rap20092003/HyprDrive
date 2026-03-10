import React from 'react';
import ReactDOM from 'react-dom/client';

function App() {
    return (
        <div style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            height: '100vh',
            fontFamily: 'system-ui, sans-serif',
            background: '#0a0a0a',
            color: '#fafafa',
        }}>
            <div style={{ textAlign: 'center' }}>
                <h1 style={{ fontSize: '2rem', marginBottom: '0.5rem' }}>⚡ HyprDrive Web</h1>
                <p style={{ color: '#888' }}>Connects to daemon via WebSocket</p>
            </div>
        </div>
    );
}

const root = document.getElementById('root');
if (root) {
    ReactDOM.createRoot(root).render(
        <React.StrictMode>
            <App />
        </React.StrictMode>
    );
}
