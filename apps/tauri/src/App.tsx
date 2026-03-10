/**
 * Root application component — placeholder until Phase 11
 */
export default function App() {
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
                <h1 style={{ fontSize: '2rem', marginBottom: '0.5rem' }}>⚡ HyprDrive</h1>
                <p style={{ color: '#888' }}>Desktop client connected to daemon</p>
                <p style={{ color: '#555', fontSize: '0.8rem', marginTop: '1rem' }}>
                    Phase 0 scaffold — UI coming in Phase 11
                </p>
            </div>
        </div>
    );
}
