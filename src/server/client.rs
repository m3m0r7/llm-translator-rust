use anyhow::{Context, Result};
use axum::Router;
use axum::response::Html;
use axum::routing::get;
use std::sync::Arc;

pub async fn run_client(addr: String, api_base: String) -> Result<()> {
    let html = Arc::new(render_client_html(&api_base)?);
    let app = Router::new().route(
        "/",
        get({
            let html = html.clone();
            move || {
                let html = html.clone();
                async move { Html((*html).clone()) }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| "failed to bind client address")?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn render_client_html(api_base: &str) -> Result<String> {
    let api_base_json = serde_json::to_string(api_base)?;
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>LLM Translator</title>
  <style>
    :root {{
      --bg: #f4fbf6;
      --panel: #ffffff;
      --panel-2: #f0f8f2;
      --accent: #2f855a;
      --accent-2: #48bb78;
      --text: #1f2937;
      --muted: #6b7280;
      --border: #d1e7d8;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
      background: var(--bg);
      color: var(--text);
    }}
    header {{
      padding: 24px 32px;
      border-bottom: 1px solid var(--border);
      background: var(--panel);
    }}
    header h1 {{
      margin: 0;
      font-size: 22px;
      letter-spacing: 0.02em;
    }}
    main {{
      display: grid;
      grid-template-columns: 1.1fr 1.6fr;
      gap: 20px;
      padding: 20px 32px 36px;
    }}
    .card {{
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 16px;
      box-shadow: 0 6px 16px rgba(0, 0, 0, 0.04);
    }}
    .card h2 {{
      margin: 0 0 12px;
      font-size: 15px;
      color: var(--accent);
      text-transform: uppercase;
      letter-spacing: 0.08em;
    }}
    .history-list {{
      display: flex;
      flex-direction: column;
      gap: 12px;
      max-height: calc(100vh - 180px);
      overflow: auto;
    }}
    .history-item {{
      border: 1px solid var(--border);
      border-radius: 10px;
      padding: 10px 12px;
      background: var(--panel-2);
      font-size: 12px;
      line-height: 1.5;
    }}
    .history-item strong {{
      color: var(--accent);
    }}
    .history-text {{
      white-space: pre-wrap;
      word-break: break-word;
    }}
    form {{
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 12px;
    }}
    label {{
      font-size: 12px;
      color: var(--muted);
      display: block;
      margin-bottom: 6px;
    }}
    input, select, textarea {{
      width: 100%;
      padding: 8px 10px;
      border-radius: 8px;
      border: 1px solid var(--border);
      background: #fff;
      font-size: 13px;
    }}
    textarea {{
      min-height: 140px;
      resize: vertical;
      grid-column: 1 / -1;
    }}
    .row {{
      grid-column: 1 / -1;
    }}
    .actions {{
      display: flex;
      gap: 10px;
      grid-column: 1 / -1;
    }}
    button {{
      border: none;
      padding: 10px 14px;
      border-radius: 8px;
      background: var(--accent);
      color: #fff;
      font-weight: 600;
      cursor: pointer;
    }}
    button.secondary {{
      background: #e2e8f0;
      color: #1f2937;
    }}
    .toggle {{
      display: flex;
      align-items: center;
      gap: 8px;
      font-size: 12px;
      color: var(--muted);
    }}
    .results {{
      margin-top: 16px;
      display: flex;
      flex-direction: column;
      gap: 12px;
    }}
    .result-item {{
      border: 1px solid var(--border);
      border-radius: 10px;
      padding: 12px;
      background: var(--panel-2);
    }}
    .result-item pre {{
      margin: 0;
      white-space: pre-wrap;
      word-break: break-word;
      font-size: 13px;
    }}
    .result-item img {{
      max-width: 100%;
      border-radius: 8px;
    }}
    .error {{
      color: #b91c1c;
      font-size: 13px;
      margin-top: 8px;
    }}
    @media (max-width: 960px) {{
      main {{
        grid-template-columns: 1fr;
      }}
      .history-list {{
        max-height: none;
      }}
    }}
  </style>
</head>
<body>
  <header>
    <h1>LLM Translator</h1>
  </header>
  <main>
    <section class="card">
      <h2>Histories</h2>
      <div id="histories" class="history-list"></div>
    </section>
    <section class="card">
      <h2>Translate</h2>
      <form id="translate-form">
        <div>
          <label for="source-lang">Source language</label>
          <select id="source-lang"></select>
        </div>
        <div>
          <label for="target-lang">Target language</label>
          <select id="target-lang"></select>
        </div>
        <div>
          <label for="formal">Formality</label>
          <select id="formal"></select>
        </div>
        <div>
          <label for="model">Model (optional)</label>
          <input id="model" placeholder="openai:gpt-5.2" />
        </div>
        <div class="row">
          <label for="text">Text</label>
          <textarea id="text" placeholder="Enter text to translate..."></textarea>
        </div>
        <div>
          <label for="file">File</label>
          <input id="file" type="file" />
        </div>
        <div>
          <label for="mime">Mime override (optional)</label>
          <input id="mime" placeholder="auto" />
        </div>
        <div class="toggle">
          <input id="slang" type="checkbox" />
          <label for="slang">Enable slang</label>
        </div>
        <div class="toggle">
          <input id="force" type="checkbox" />
          <label for="force">Force translation</label>
        </div>
        <div class="toggle">
          <input id="commentout" type="checkbox" />
          <label for="commentout">Translate comments</label>
        </div>
        <div class="actions">
          <button type="submit">Translate</button>
          <button type="button" class="secondary" id="clear">Clear</button>
        </div>
        <div id="error" class="error"></div>
      </form>
      <div class="results" id="results"></div>
    </section>
  </main>
  <script>
    const API_BASE = {api_base_json};

    const historiesEl = document.getElementById('histories');
    const form = document.getElementById('translate-form');
    const resultsEl = document.getElementById('results');
    const errorEl = document.getElementById('error');
    const fileEl = document.getElementById('file');

    async function fetchJson(path, options) {{
      const res = await fetch(`${{API_BASE}}${{path}}`, options);
      const text = await res.text();
      let data = null;
      try {{ data = text ? JSON.parse(text) : null; }} catch (_) {{ data = text; }}
      if (!res.ok) {{
        const message = data && data.error ? data.error : res.statusText;
        throw new Error(message);
      }}
      return data;
    }}

    function formatDate(value) {{
      const num = Number(value);
      if (!Number.isFinite(num)) return value;
      return new Date(num * 1000).toLocaleString();
    }}

    function renderHistories(items) {{
      historiesEl.innerHTML = '';
      if (!items || items.length === 0) {{
        historiesEl.innerHTML = '<div class="history-item">No histories yet.</div>';
        return;
      }}
      items.forEach((item, idx) => {{
        const div = document.createElement('div');
        div.className = 'history-item';
        div.innerHTML = `
          <div><strong>[${{idx + 1}}]</strong></div>
          <div>datetime: ${{formatDate(item.datetime)}}</div>
          <div>type: ${{item.type}}</div>
          <div>model: ${{item.model}}</div>
          ${{item.formal ? `<div>formal: ${{item.formal}}</div>` : ''}}
          <div>mime: ${{item.mime}}</div>
          <div>src:</div>
          <div class="history-text">${{item.src}}</div>
          <div>dest:</div>
          <div class="history-text">${{item.dest}}</div>
        `;
        historiesEl.appendChild(div);
      }});
    }}

    async function loadHistories() {{
      try {{
        const data = await fetchJson('/histories');
        renderHistories(data);
      }} catch (err) {{
        historiesEl.innerHTML = `<div class="history-item">Failed to load histories: ${{err.message}}</div>`;
      }}
    }}

    function populateSelect(select, values, defaultValue) {{
      select.innerHTML = '';
      values.forEach(value => {{
        const opt = document.createElement('option');
        opt.value = value;
        opt.textContent = value;
        if (value === defaultValue) opt.selected = true;
        select.appendChild(opt);
      }});
    }}

    async function loadSettings() {{
      const data = await fetchJson('/settings');
      const sourceSelect = document.getElementById('source-lang');
      const targetSelect = document.getElementById('target-lang');
      const formalSelect = document.getElementById('formal');
      const languages = data.languages || [];
      const sourceValues = ['auto', ...languages];
      populateSelect(sourceSelect, sourceValues, data.default_source_lang || 'auto');
      populateSelect(targetSelect, languages.length ? languages : ['en'], data.default_lang || 'en');
      populateSelect(formalSelect, data.formal_keys || ['formal'], data.default_formal || 'formal');
    }}

    function showError(message) {{
      errorEl.textContent = message || '';
    }}

    function renderResults(contents) {{
      resultsEl.innerHTML = '';
      if (!contents || contents.length === 0) return;
      contents.forEach((content, idx) => {{
        const wrapper = document.createElement('div');
        wrapper.className = 'result-item';
        const title = document.createElement('div');
        title.style.marginBottom = '8px';
        title.style.fontWeight = '600';
        title.textContent = `Result ${{idx + 1}} (${{content.mime}})`;
        wrapper.appendChild(title);

        if (content.format === 'raw') {{
          const pre = document.createElement('pre');
          pre.textContent = content.translated || '';
          wrapper.appendChild(pre);
        }} else if (content.format === 'base64') {{
          const dataUrl = `data:${{content.mime}};base64,${{content.translated}}`;
          if (content.mime.startsWith('image/')) {{
            const img = document.createElement('img');
            img.src = dataUrl;
            wrapper.appendChild(img);
          }} else if (content.mime.startsWith('audio/')) {{
            const audio = document.createElement('audio');
            audio.controls = true;
            audio.src = dataUrl;
            wrapper.appendChild(audio);
          }} else if (content.mime.startsWith('video/')) {{
            const video = document.createElement('video');
            video.controls = true;
            video.src = dataUrl;
            wrapper.appendChild(video);
          }} else {{
            const link = document.createElement('a');
            link.href = dataUrl;
            link.download = `translated-${{idx + 1}}`;
            link.textContent = 'Download translated file';
            wrapper.appendChild(link);
          }}
        }} else {{
          const pre = document.createElement('pre');
          pre.textContent = content.translated || '';
          wrapper.appendChild(pre);
        }}
        resultsEl.appendChild(wrapper);
      }});
    }}

    async function readFileAsDataUrl(file) {{
      return new Promise((resolve, reject) => {{
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result);
        reader.onerror = () => reject(new Error('Failed to read file'));
        reader.readAsDataURL(file);
      }});
    }}

    form.addEventListener('submit', async (event) => {{
      event.preventDefault();
      showError('');
      resultsEl.innerHTML = '';

      const payload = {{
        lang: document.getElementById('target-lang').value,
        source_lang: document.getElementById('source-lang').value,
        formal: document.getElementById('formal').value,
        model: document.getElementById('model').value || null,
        slang: document.getElementById('slang').checked,
        force_translation: document.getElementById('force').checked,
        with_commentout: document.getElementById('commentout').checked,
        response_format: 'base64'
      }};

      const file = fileEl.files[0];
      if (file) {{
        const dataUrl = await readFileAsDataUrl(file);
        payload.data_base64 = dataUrl;
        payload.data_name = file.name;
        const overrideMime = document.getElementById('mime').value.trim();
        payload.data_mime = overrideMime || file.type || 'auto';
      }} else {{
        const text = document.getElementById('text').value.trim();
        if (!text) {{
          showError('Text is empty.');
          return;
        }}
        payload.text = text;
      }}

      try {{
        const data = await fetchJson('/translate', {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json' }},
          body: JSON.stringify(payload)
        }});
        renderResults(data.contents || []);
        await loadHistories();
      }} catch (err) {{
        showError(err.message);
      }}
    }});

    document.getElementById('clear').addEventListener('click', () => {{
      document.getElementById('text').value = '';
      fileEl.value = '';
      document.getElementById('mime').value = '';
      resultsEl.innerHTML = '';
      showError('');
    }});

    loadSettings().then(loadHistories);
  </script>
</body>
</html>"#
    );
    Ok(html)
}
