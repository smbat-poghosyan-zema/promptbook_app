const BUILD_INFO = `v${__APP_VERSION__} (${import.meta.env.MODE})`;

export function App() {
  return (
    <main className="app-shell">
      <h1>Promptbook Runner</h1>
      <p data-testid="build-info">Build info: {BUILD_INFO}</p>
    </main>
  );
}
