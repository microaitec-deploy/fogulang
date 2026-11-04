import React, { useEffect, useMemo, useRef, useState } from 'react';

/** One entry of the search index: a page, or a heading inside a page. */
export interface SearchEntry {
  id: string; // the anchor to go to
  title: string;
  group: string; // the page's group, or the page's title for a heading
  text: string; // lower-case text (pages only)
  raw: string; // the text, for snippets
  heading: boolean;
}

interface Props {
  open: boolean;
  onClose: () => void;
  index: SearchEntry[];
}

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

function snippet(raw: string, q: string): string {
  const i = raw.toLowerCase().indexOf(q);
  if (i < 0) return '';
  const from = Math.max(0, i - 50);
  const part = raw.slice(from, i + q.length + 70);
  const at = i - from;
  return (from > 0 ? '… ' : '') + esc(part.slice(0, at)) + '<mark>' + esc(part.slice(at, at + q.length)) + '</mark>' + esc(part.slice(at + q.length)) + '…';
}

export const SearchPalette: React.FC<Props> = ({ open, onClose, index }) => {
  const [q, setQ] = useState('');
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    if (open) {
      setSel(0);
      setTimeout(() => input.current?.select(), 0);
    }
  }, [open]);

  const results = useMemo(() => {
    const query = q.trim().toLowerCase();
    if (query.length < 2) return [];
    const words = query.split(/\s+/);
    const hits: { e: SearchEntry; score: number }[] = [];
    for (const e of index) {
      const title = e.title.toLowerCase();
      let score = -1;
      if (title === query || title === 'fogu:' + query) score = 0;
      else if (title.startsWith(query) || title.includes(':' + query)) score = 1;
      else if (title.includes(query)) score = 2;
      else if (words.every((w) => title.includes(w))) score = 3;
      else if (!e.heading && e.text.includes(query)) score = 5;
      else if (!e.heading && words.length > 1 && words.every((w) => e.text.includes(w))) score = 6;
      if (score >= 0) hits.push({ e, score: score + (e.heading ? 0.5 : 0) });
    }
    hits.sort((a, b) => a.score - b.score);
    return hits.slice(0, 30).map((h) => h.e);
  }, [q, index]);

  useEffect(() => setSel(0), [q]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-i="${sel}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [sel]);

  if (!open) return null;
  const query = q.trim().toLowerCase();

  const go = (e: SearchEntry) => {
    onClose();
    location.hash = e.id;
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-start justify-center px-3 pt-[8vh] sm:pt-[12vh]" role="dialog" aria-modal="true" aria-label="Search the documentation">
      <div className="absolute inset-0 bg-black/45 backdrop-blur-[3px] [animation:fade-in_.15s_ease-out]" onClick={onClose} aria-hidden="true" />
      <div className="relative flex max-h-[78vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-[var(--border)] bg-[var(--bg-elevated)] shadow-2xl [animation:pop-in_.18s_ease-out]">
        <div className="flex items-center gap-3 border-b border-[var(--border)] px-4">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" className="shrink-0 text-[var(--accent)]" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
          <input
            ref={input}
            type="search"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'ArrowDown') {
                e.preventDefault();
                setSel((s) => Math.min(results.length - 1, s + 1));
              } else if (e.key === 'ArrowUp') {
                e.preventDefault();
                setSel((s) => Math.max(0, s - 1));
              } else if (e.key === 'Enter' && results[sel]) {
                e.preventDefault();
                go(results[sel]);
              } else if (e.key === 'Escape') {
                e.preventDefault();
                onClose();
              }
            }}
            placeholder="Search modules, functions, guides…"
            className="h-14 min-w-0 flex-1 bg-transparent text-base text-[var(--fg)] outline-none placeholder:text-[var(--fg-faint)] [&::-webkit-search-cancel-button]:hidden"
            aria-label="Search"
            aria-controls="search-results"
            aria-activedescendant={results[sel] ? `sr-${sel}` : undefined}
            autoComplete="off"
            spellCheck={false}
          />
          <button type="button" onClick={onClose} className="shrink-0" aria-label="Close the search">
            <kbd className="key">Esc</kbd>
          </button>
        </div>
        <ul ref={list} id="search-results" role="listbox" className="min-h-0 flex-1 overflow-y-auto p-2">
          {query.length < 2 && (
            <li className="px-3 py-8 text-center text-sm text-[var(--fg-faint)]">Type at least two letters — a module (<code className="font-mono">http</code>), a function (<code className="font-mono">readText</code>) or a topic.</li>
          )}
          {query.length >= 2 && results.length === 0 && <li className="px-3 py-8 text-center text-sm text-[var(--fg-faint)]">No results for “{q}”.</li>}
          {results.map((e, i) => (
            <li key={e.id + i} id={`sr-${i}`} role="option" aria-selected={i === sel} data-i={i}>
              <a
                href={`#${e.id}`}
                onClick={(ev) => {
                  ev.preventDefault();
                  go(e);
                }}
                onMouseMove={() => setSel(i)}
                className={`flex items-start gap-3 rounded-xl px-3 py-2.5 transition-colors ${i === sel ? 'bg-[var(--accent-soft)]' : ''}`}
              >
                <span className={`mt-0.5 grid size-7 shrink-0 place-items-center rounded-lg border border-[var(--border)] text-[var(--fg-faint)] ${i === sel ? 'border-[var(--accent-ring)] text-[var(--accent)]' : ''}`} aria-hidden="true">
                  {e.heading ? (
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round"><path d="M4 9h16M4 15h16M10 3 8 21M16 3l-2 18" /></svg>
                  ) : (
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /></svg>
                  )}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block text-[0.7rem] font-medium uppercase tracking-wider text-[var(--fg-faint)]">{e.group}</span>
                  <span className="block truncate font-medium text-[var(--fg)]">{e.title}</span>
                  {!e.heading && !e.title.toLowerCase().includes(query) && (
                    <span className="search-snippet mt-0.5 block truncate text-xs text-[var(--fg-muted)]" dangerouslySetInnerHTML={{ __html: snippet(e.raw, query) }} />
                  )}
                </span>
                {i === sel && <span className="mt-2 hidden shrink-0 text-xs text-[var(--fg-faint)] sm:block">↵</span>}
              </a>
            </li>
          ))}
        </ul>
        <div className="hidden items-center gap-4 border-t border-[var(--border)] px-4 py-2 text-xs text-[var(--fg-faint)] sm:flex">
          <span className="flex items-center gap-1"><kbd className="key">↑</kbd><kbd className="key">↓</kbd> to move</span>
          <span className="flex items-center gap-1"><kbd className="key">↵</kbd> to open</span>
          <span className="flex items-center gap-1"><kbd className="key">Esc</kbd> to close</span>
        </div>
      </div>
    </div>
  );
};
