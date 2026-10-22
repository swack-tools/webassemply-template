//! Embedded Single-Page Web Dashboard (< 200 KiB).

pub const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Cloudflare Worker + KV State Dashboard</title>
  <style>
    :root {
      color-scheme: dark;
      --bg: #0b0f19;
      --panel: #111827;
      --border: #1f2937;
      --text: #f3f4f6;
      --muted: #9ca3af;
      --accent: #38bdf8;
    }
    body {
      margin: 0;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      background: var(--bg);
      color: var(--text);
      padding: 2rem;
    }
    .card {
      max-width: 840px;
      margin: 0 auto;
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 1.5rem;
    }
    h1 { margin-top: 0; font-size: 1.5rem; color: var(--accent); }
    pre {
      background: #030712;
      padding: 1rem;
      border-radius: 8px;
      overflow-x: auto;
      border: 1px solid var(--border);
    }
    .meta { color: var(--muted); font-size: 0.9rem; }
  </style>
</head>
<body>
  <main class="card">
    <h1>Cloudflare Worker + KV State Dashboard</h1>
    <p class="meta">Served from compiled Rust WebAssembly memory in &lt; 15 ms.</p>
    <pre id="state-output">Loading consolidated KV state...</pre>
  </main>
  <script>
    fetch('/api/state')
      .then((r) => r.json())
      .then((data) => {
        document.getElementById('state-output').textContent = JSON.stringify(data, null, 2);
      })
      .catch((err) => {
        document.getElementById('state-output').textContent = 'Error loading state: ' + err;
      });
  </script>
</body>
</html>
"#;
