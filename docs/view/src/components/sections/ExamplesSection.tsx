import React, { useEffect, useState } from 'react';
import { DocCodeBlock } from '../DocCodeBlock';
import { EXAMPLES, TOPICS, REPO, type Example, langOf, loadCode, sourceUrl, blobUrl, examplesFor } from '../../data/examples';

const slug = (s: string) => s.toLowerCase().replace(/&/g, 'and').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

/** Inline Markdown of a description: `code`. */
export const Desc: React.FC<{ text: string; className?: string }> = ({ text, className }) => (
  <p className={className}>
    {text.split(/(`[^`]+`)/).map((part, i) => (part.startsWith('`') && part.endsWith('`') ? <code key={i}>{part.slice(1, -1)}</code> : part))}
  </p>
);

const ExternalIcon = () => (
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M7 17 17 7M8 7h9v9" /></svg>
);

const FileIcon: React.FC<{ folder?: boolean }> = ({ folder }) =>
  folder ? (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
  ) : (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M10 13l-2 2 2 2M14 13l2 2-2 2" /></svg>
  );

/** The source of an example (with tabs for a folder), loaded on demand. */
const ExampleCode: React.FC<{ ex: Example; onRun?: (code: string) => void }> = ({ ex, onRun }) => {
  const [code, setCode] = useState<Record<string, string> | null>(null);
  const [file, setFile] = useState(ex.files[0]?.path);
  useEffect(() => {
    let live = true;
    loadCode().then((c) => live && setCode(c));
    return () => {
      live = false;
    };
  }, []);
  if (!code) return <div className="mt-4 h-24 animate-pulse rounded-xl bg-[var(--bg-subtle)]" aria-label="Loading the source" />;
  return (
    <div className="mt-4">
      {ex.files.length > 1 && (
        <div className="-mb-px flex gap-1 overflow-x-auto" role="tablist" aria-label={`Files of ${ex.title}`}>
          {ex.files.map((f) => (
            <button
              key={f.path}
              type="button"
              role="tab"
              aria-selected={f.path === file}
              onClick={() => setFile(f.path)}
              className={`shrink-0 rounded-t-lg border border-b-0 px-3 py-1.5 font-mono text-xs transition-colors ${
                f.path === file ? 'border-[var(--border)] bg-[var(--code-head)] text-[var(--fg)]' : 'border-transparent text-[var(--fg-faint)] hover:text-[var(--fg)]'
              }`}
            >
              {f.path.slice(ex.path.length + 1)}
            </button>
          ))}
        </div>
      )}
      <DocCodeBlock
        className="!mt-0"
        lang={langOf(file)}
        title={file}
        code={code[file] ?? ''}
        onRun={onRun}
        actions={
          <a href={blobUrl(file)} target="_blank" rel="noreferrer" className="btn-reset code-btn" title="Open on GitHub">
            <span>GitHub</span>
            <ExternalIcon />
          </a>
        }
      />
    </div>
  );
};

const ExampleCard: React.FC<{ ex: Example; pages: Map<string, string>; onRun?: (code: string) => void }> = ({ ex, pages, onRun }) => {
  const [open, setOpen] = useState(false);
  const [targeted, setTargeted] = useState(false);

  // #ex-<name> opens the card (links from the other pages)
  useEffect(() => {
    const check = () => {
      const hit = location.hash === '#' + ex.id;
      setTargeted(hit);
      if (hit) setOpen(true);
    };
    check();
    window.addEventListener('hashchange', check);
    return () => window.removeEventListener('hashchange', check);
  }, [ex.id]);

  const docs = [...new Set([...ex.imports.map((m) => 'mod-' + m), ...ex.pages])].filter((p) => pages.has(p));

  return (
    <article
      id={ex.id}
      className={`example-card frame-card scroll-mt-24 p-4 sm:p-5 ${open ? 'sm:col-span-2' : ''} ${targeted ? 'is-targeted' : ''}`}
    >
      <div className="flex items-start gap-3">
        <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg bg-[var(--accent-soft)] text-[var(--accent)]">
          <FileIcon folder={ex.folder} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-3">
            <h4 className="!mt-0 truncate font-mono text-[0.92rem] font-semibold text-[var(--fg)]">{ex.title}</h4>
            <span className="shrink-0 font-mono text-[0.68rem] text-[var(--fg-faint)]">
              {ex.files.length > 1 ? `${ex.files.length} files` : `${ex.files[0]?.lines ?? 0} lines`}
            </span>
          </div>
          {ex.desc && <Desc text={ex.desc} className="!my-1 text-[0.875rem] leading-relaxed text-[var(--fg-muted)]" />}
        </div>
      </div>
      {docs.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-1.5 pl-11">
          {docs.slice(0, 6).map((p) => (
            <a key={p} href={`#${p}`} className="btn-reset chip" title={`Docs: ${pages.get(p)}`}>
              {pages.get(p)}
            </a>
          ))}
        </div>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-2 pl-11">
        <button type="button" onClick={() => setOpen(!open)} aria-expanded={open} className="ex-btn ex-btn-primary">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className={`transition-transform ${open ? 'rotate-90' : ''}`}><path d="m9 6 6 6-6 6" /></svg>
          {open ? 'Hide code' : 'Show code'}
        </button>
        <a href={sourceUrl(ex)} target="_blank" rel="noreferrer" className="btn-reset ex-btn">
          {ex.folder ? 'Folder on GitHub' : 'Source on GitHub'}
          <ExternalIcon />
        </a>
      </div>
      {open && <ExampleCode ex={ex} onRun={onRun} />}
    </article>
  );
};

interface Props {
  onRun: (code: string) => void;
}

export const ExamplesSection: React.FC<Props> = ({ onRun }) => {
  // the docs pages that exist (to link the modules an example imports)
  const [pages, setPages] = useState<Map<string, string>>(new Map());
  useEffect(() => {
    const secs = [...document.querySelectorAll<HTMLElement>('#content > section[id]')];
    setPages(new Map(secs.map((s) => [s.id, s.dataset.title || s.id])));
  }, []);
  const topics = [...TOPICS.map((t) => t[0]), 'More examples'].filter((t) => EXAMPLES.some((e) => e.topic === t));
  const blurb = Object.fromEntries(TOPICS);

  return (
    <section id="examples" data-group="Examples" data-title="All examples" data-no-anchor className="scroll-mt-20 mt-16">
      <h2 className="text-2xl font-bold">Examples</h2>
      <p className="mt-3 text-[1.02rem] text-[var(--fg-muted)]">
        {EXAMPLES.length} runnable programs from the repository's{' '}
        <a href={`${REPO}/tree/main/examples`} target="_blank" rel="noreferrer">
          <code>examples/</code>
        </a>{' '}
        folder, by topic. Open one to read it here, or run it from a clone with <code>fogu examples/&lt;file&gt;.fg</code>. Servers keep
        running until Ctrl+C; run their clients in another terminal.
      </p>
      <nav className="mt-6 flex flex-wrap gap-2" aria-label="Topics">
        {topics.map((t) => (
          <a key={t} href={`#examples-${slug(t)}`} className="btn-reset chip chip-lg">
            {t}
            <span className="text-[var(--fg-faint)]">{EXAMPLES.filter((e) => e.topic === t).length}</span>
          </a>
        ))}
      </nav>
      {topics.map((t) => (
        <React.Fragment key={t}>
          <h3 id={`examples-${slug(t)}`}>
            {t}
            <a className="h-anchor" href={`#examples-${slug(t)}`} aria-label={`Link to ${t}`}>
              #
            </a>
          </h3>
          {blurb[t] && <p className="!mt-1 text-[var(--fg-muted)]">{blurb[t]}</p>}
          <div className="mt-4 grid gap-3 sm:grid-cols-2">
            {EXAMPLES.filter((e) => e.topic === t).map((e) => (
              <ExampleCard key={e.id} ex={e} pages={pages} onRun={onRun} />
            ))}
          </div>
        </React.Fragment>
      ))}
    </section>
  );
};

/** "Examples" at the end of a docs page: the example files that show its subject. */
export const RelatedExamples: React.FC<{ pageId: string }> = ({ pageId }) => {
  const list = examplesFor(pageId);
  if (pageId === 'examples' || list.length === 0) return null;
  const shown = list.slice(0, 6);
  return (
    <aside id="related-examples" className="related-examples mt-14 scroll-mt-24" aria-labelledby="related-examples-title">
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <h2 id="related-examples-title" className="flex items-center gap-2 text-[0.8rem] font-semibold uppercase tracking-wider text-[var(--fg-faint)]">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-[var(--accent)]" aria-hidden="true"><path d="m16 18 6-6-6-6M8 6l-6 6 6 6" /></svg>
          See the example{shown.length > 1 ? 's' : ''}
        </h2>
        <a href="#examples" className="text-xs font-medium text-[var(--fg-faint)] hover:text-[var(--accent)]">
          All examples →
        </a>
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        {shown.map((e) => (
          <div key={e.id} className="frame-card group flex flex-col p-4">
            <a href={`#${e.id}`} className="flex items-center gap-2 font-mono text-[0.88rem] font-semibold text-[var(--fg)] hover:text-[var(--accent)]">
              <span className="text-[var(--accent)]">
                <FileIcon folder={e.folder} />
              </span>
              examples/{e.path.replace(/^examples\//, '')}
              {e.folder ? '/' : ''}
            </a>
            {e.desc && <Desc text={e.desc} className="mt-1.5 line-clamp-2 flex-1 text-[0.82rem] leading-relaxed text-[var(--fg-muted)]" />}
            <div className="mt-3 flex gap-3 text-xs font-medium">
              <a href={`#${e.id}`} className="text-[var(--accent-strong)] hover:underline dark:text-[var(--accent)]">
                Read it here
              </a>
              <a href={sourceUrl(e)} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-[var(--fg-faint)] hover:text-[var(--fg)]">
                GitHub <ExternalIcon />
              </a>
            </div>
          </div>
        ))}
      </div>
      {list.length > shown.length && (
        <p className="mt-3 text-sm text-[var(--fg-faint)]">
          and {list.length - shown.length} more on the <a className="text-[var(--accent-strong)] underline dark:text-[var(--accent)]" href="#examples">Examples</a> page.
        </p>
      )}
    </aside>
  );
};
