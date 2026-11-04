import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { DocHeader } from './components/DocHeader';
import { DocSidebar, GROUP_ORDER, TITLES, type NavGroup, type NavPage } from './components/DocSidebar';
import { SearchPalette, type SearchEntry } from './components/SearchPalette';
import { GettingStartedSection } from './components/sections/GettingStartedSection';
import { EnterpriseAutomationSection } from './components/sections/EnterpriseAutomationSection';
import { LanguageSection } from './components/sections/LanguageSection';
import { CoreModulesSection } from './components/sections/CoreModulesSection';
import { DataAndStorageSection } from './components/sections/DataAndStorageSection';
import { WebAndMessagingSection } from './components/sections/WebAndMessagingSection';
import { IntegrationsSection } from './components/sections/IntegrationsSection';
import { AdvancedModulesSection } from './components/sections/AdvancedModulesSection';
import { ToolsAndProjectSection } from './components/sections/ToolsAndProjectSection';
import { GuidesSection } from './components/sections/GuidesSection';
import { ExamplesSection, RelatedExamples } from './components/sections/ExamplesSection';
import { examplesFor } from './data/examples';
import { BenchmarksSection } from './components/sections/BenchmarksSection';
import { InteractiveRunnerModal } from './components/InteractiveRunnerModal';

/*
 * The docs are one bundle of pages: every top-level `<section id data-group
 * data-title>` of <main id="content"> is a page. The URL hash says which one is
 * shown (#mod-http), or a heading inside it (#mod-http-requests): the page that
 * holds the target is shown, the others are hidden, and the window scrolls to
 * the target. Links are plain `href="#id"` anchors, so the history (back,
 * forward), deep links and new tabs work like on any site.
 */

interface Route {
  page: string;
  anchor: string | null; // a heading inside the page, or null for the top
}

interface Heading {
  id: string;
  text: string;
  depth: number;
}

const slug = (s: string) =>
  s
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 60);

const pageSections = () => Array.from(document.querySelectorAll<HTMLElement>('#content > section[id]'));

/** The page that holds an element id (or null). */
function pageOf(id: string): HTMLElement | null {
  if (!id) return null;
  const el = document.getElementById(id);
  return (el?.closest('#content > section[id]') as HTMLElement | null) ?? null;
}

function readRoute(first: string): Route {
  let id = '';
  try {
    id = decodeURIComponent(location.hash.slice(1));
  } catch {
    id = location.hash.slice(1);
  }
  const page = pageOf(id);
  if (!page) return { page: first, anchor: null };
  return { page: page.id, anchor: page.id === id ? null : id };
}

/** Gives every heading of the pages an id and a "#" link (once, after the first render). */
function decorateHeadings() {
  const taken = new Set(Array.from(document.querySelectorAll('[id]')).map((e) => e.id));
  for (const sec of pageSections()) {
    if (sec.hasAttribute('data-no-anchor')) continue;
    const heads = sec.querySelectorAll<HTMLElement>('h1, h2, h3');
    heads.forEach((h, i) => {
      if (h.closest('figure, table, [data-no-anchor], button, a')) return;
      if (h.querySelector('.h-anchor')) return;
      let id = h.id;
      if (!id) {
        if (i === 0 && (h.tagName === 'H1' || h.tagName === 'H2')) id = sec.id;
        else {
          const base = `${sec.id}-${slug(h.textContent || 'section') || 'section'}`;
          id = base;
          for (let n = 2; taken.has(id); n++) id = `${base}-${n}`;
          h.id = id;
          taken.add(id);
        }
      }
      const a = document.createElement('a');
      a.className = 'h-anchor';
      a.href = '#' + id;
      a.setAttribute('aria-label', 'Link to this section');
      a.textContent = '#';
      h.appendChild(a);
    });
  }
}

/** The sidebar: the pages grouped, groups in GROUP_ORDER (unknown ones after, in page order). */
function buildNav(): { groups: NavGroup[]; pages: NavPage[] } {
  const byGroup = new Map<string, NavPage[]>();
  for (const sec of pageSections()) {
    const group = sec.dataset.group || 'More';
    const title = TITLES[sec.id] ?? sec.dataset.title ?? sec.id;
    if (!byGroup.has(group)) byGroup.set(group, []);
    byGroup.get(group)!.push({ id: sec.id, title, group });
  }
  const names = [...byGroup.keys()];
  const rank = (g: string) => (GROUP_ORDER.includes(g) ? GROUP_ORDER.indexOf(g) : GROUP_ORDER.length + names.indexOf(g));
  const groups = names.sort((a, b) => rank(a) - rank(b)).map((g) => ({ group: g, pages: byGroup.get(g)! }));
  return { groups, pages: groups.flatMap((g) => g.pages) };
}

const BLOCK = /^(P|LI|TD|TH|H1|H2|H3|H4|PRE|DIV|FIGCAPTION|SECTION|TR|UL|OL|ARTICLE|BUTTON)$/;

/** The text of an element, with a space between blocks (for the search). */
function textOf(root: HTMLElement): string {
  const parts: string[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let lastBlock: Element | null = null;
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const parent = n.parentElement;
    if (!parent || parent.classList.contains('h-anchor')) continue;
    let block: Element | null = parent;
    while (block && block !== root && !BLOCK.test(block.tagName)) block = block.parentElement;
    if (block !== lastBlock && parts.length) parts.push(' ');
    lastBlock = block;
    parts.push(n.nodeValue || '');
  }
  return parts.join('').replace(/\s+/g, ' ').trim();
}

function buildIndex(pages: NavPage[]): SearchEntry[] {
  const out: SearchEntry[] = [];
  for (const p of pages) {
    const sec = document.getElementById(p.id);
    if (!sec) continue;
    const raw = textOf(sec);
    out.push({ id: p.id, title: p.title, group: p.group, text: raw.toLowerCase(), raw, heading: false });
    sec.querySelectorAll<HTMLElement>('h3[id]').forEach((h) => {
      const title = (h.textContent || '').replace(/#$/, '').trim();
      if (title) out.push({ id: h.id, title, group: p.title, text: '', raw: '', heading: true });
    });
  }
  return out;
}

const Arrow: React.FC<{ left?: boolean }> = ({ left }) => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="transition-transform group-hover:translate-x-0.5">
    {left ? <path d="M19 12H5M11 18l-6-6 6-6" /> : <path d="M5 12h14M13 6l6 6-6 6" />}
  </svg>
);

export default function App() {
  const [nav, setNav] = useState<{ groups: NavGroup[]; pages: NavPage[] }>({ groups: [], pages: [] });
  const [route, setRoute] = useState<Route | null>(null);
  const [headings, setHeadings] = useState<Heading[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [index, setIndex] = useState<SearchEntry[]>([]);
  const [runnerCode, setRunnerCode] = useState<string | null>(null);
  const lastPage = useRef<string | null>(null);

  const onRun = useCallback((code: string) => setRunnerCode(code), []);
  const closeMenu = useCallback(() => setMenuOpen(false), []);
  const openSearch = useCallback(() => setSearchOpen(true), []);

  // the pages are rendered once: route changes only show and hide them
  const content = useMemo(
    () => (
      <>
        <GettingStartedSection onRun={onRun} />
        <EnterpriseAutomationSection onRun={onRun} />
        <LanguageSection onRun={onRun} />
        <GuidesSection onRun={onRun} />
        <ExamplesSection onRun={onRun} />
        <CoreModulesSection onRun={onRun} />
        <DataAndStorageSection onRun={onRun} />
        <WebAndMessagingSection onRun={onRun} />
        <IntegrationsSection onRun={onRun} />
        <AdvancedModulesSection onRun={onRun} />
        <ToolsAndProjectSection onRun={onRun} />
        <BenchmarksSection />
      </>
    ),
    [onRun],
  );

  // after the first render: ids and anchors on headings, the sidebar, the route
  useLayoutEffect(() => {
    decorateHeadings();
    const n = buildNav();
    setNav(n);
    const first = n.pages[0]?.id ?? 'intro';
    setRoute(readRoute(first));
    const onHash = () => setRoute(readRoute(first));
    window.addEventListener('hashchange', onHash);
    // the search index is built when the browser is idle
    const idle = (window as any).requestIdleCallback ?? ((f: () => void) => setTimeout(f, 300));
    idle(() => setIndex(buildIndex(n.pages)));
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  // a new route: show the page, scroll to the target, list its headings
  useLayoutEffect(() => {
    if (!route) return;
    const samePage = lastPage.current === route.page;
    lastPage.current = route.page;
    const sec = document.getElementById(route.page);
    if (route.anchor) {
      const el = document.getElementById(route.anchor);
      el?.scrollIntoView({ behavior: samePage ? 'smooth' : 'instant', block: 'start' });
    } else {
      window.scrollTo({ top: 0, behavior: 'instant' });
    }
    const page = nav.pages.find((p) => p.id === route.page);
    document.title = page && route.page !== 'intro' ? `${page.title} · Fogu docs` : 'Fogu Documentation';
    if (sec) {
      const hs = Array.from(sec.querySelectorAll<HTMLElement>('h3[id]'))
        .filter((h) => !h.closest('figure, [data-toc-skip]') && h.offsetParent !== null)
        .map((h) => ({ id: h.id, text: (h.textContent || '').replace(/#$/, '').trim(), depth: 3 }));
      setHeadings(hs);
    }
  }, [route, nav.pages]);

  // "on this page": the heading the reader is at
  useEffect(() => {
    if (!headings.length) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      const line = 120;
      let cur: string | null = null;
      for (const h of headings) {
        const el = document.getElementById(h.id);
        if (el && el.getBoundingClientRect().top <= line) cur = h.id;
      }
      // at the very bottom, the last heading
      if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 4) cur = headings[headings.length - 1].id;
      setCurrent(cur);
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', onScroll);
    return () => {
      window.removeEventListener('scroll', onScroll);
      window.removeEventListener('resize', onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [headings]);

  // keyboard: Ctrl/Cmd+K or "/" opens the search
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = /^(INPUT|TEXTAREA|SELECT)$/.test((e.target as HTMLElement)?.tagName) || (e.target as HTMLElement)?.isContentEditable;
      if ((e.key === 'k' || e.key === 'K') && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setSearchOpen((o) => !o);
      } else if (e.key === '/' && !typing) {
        e.preventDefault();
        setSearchOpen(true);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  // no page scrolling behind the drawer or the search
  useEffect(() => {
    document.documentElement.style.overflow = menuOpen || searchOpen ? 'hidden' : '';
  }, [menuOpen, searchOpen]);

  const pages = nav.pages;
  const at = route ? pages.findIndex((p) => p.id === route.page) : -1;
  const page = at >= 0 ? pages[at] : null;
  const prev = at > 0 ? pages[at - 1] : null;
  const next = at >= 0 && at + 1 < pages.length ? pages[at + 1] : null;
  const groupFirst = page ? nav.groups.find((g) => g.group === page.group)?.pages[0] : null;
  const exampleCount = route && route.page !== 'examples' ? examplesFor(route.page).length : 0;
  const cssId = route ? route.page.replace(/["\\]/g, '\\$&') : '';

  return (
    <div className="min-h-screen bg-[var(--bg)] text-[var(--fg)]">
      {route && <style>{`#content > section:not([id="${cssId}"]) { display: none; }`}</style>}
      <a href="#content" className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-[70] focus:rounded-lg focus:bg-[var(--bg-elevated)] focus:px-3 focus:py-2 focus:shadow-lg" onClick={(e) => { e.preventDefault(); document.getElementById('content')?.focus(); }}>
        Skip to content
      </a>

      <DocHeader onToggleMobileMenu={() => setMenuOpen((o) => !o)} onOpenSearch={openSearch} activeGroup={page?.group} menuOpen={menuOpen} />

      <div className="mx-auto flex max-w-[90rem] gap-10 px-4 sm:px-6 lg:gap-12">
        <DocSidebar groups={nav.groups} activeId={route?.page ?? ''} isMobileOpen={menuOpen} onCloseMobile={closeMenu} onOpenSearch={openSearch} />

        <div className="min-w-0 flex-1 pb-16 pt-6 lg:pt-8">
          <div className="flex gap-12">
            <div className="min-w-0 flex-1 xl:max-w-[50rem]">
              {/* breadcrumb */}
              {page && (
                <nav aria-label="Breadcrumb" className="mb-5 flex flex-wrap items-center gap-1.5 text-[0.8rem] text-[var(--fg-faint)]">
                  <a href="#intro" className="hover:text-[var(--fg)]">Docs</a>
                  <span aria-hidden="true" className="opacity-60">/</span>
                  {groupFirst && groupFirst.id !== page.id ? (
                    <a href={`#${groupFirst.id}`} className="hover:text-[var(--fg)]">{page.group}</a>
                  ) : (
                    <span>{page.group}</span>
                  )}
                  <span aria-hidden="true" className="opacity-60">/</span>
                  <span aria-current="page" className="font-medium text-[var(--fg-muted)]">{page.title}</span>
                  {exampleCount > 0 && (
                    <a
                      href="#related-examples"
                      onClick={(e) => {
                        e.preventDefault();
                        document.getElementById('related-examples')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
                      }}
                      className="ml-auto inline-flex items-center gap-1.5 rounded-full border border-[var(--border)] bg-[var(--bg-subtle)] px-2.5 py-0.5 text-[0.75rem] font-medium text-[var(--fg-muted)] transition-colors hover:border-[var(--accent-ring)] hover:text-[var(--accent)]"
                    >
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m16 18 6-6-6-6M8 6l-6 6 6 6" /></svg>
                      {exampleCount === 1 ? 'See the example' : `${exampleCount} examples`}
                    </a>
                  )}
                </nav>
              )}

              {/* "on this page" on phones and tablets */}
              {headings.length > 1 && (
                <details className="group mb-6 rounded-xl border border-[var(--border)] bg-[var(--bg-subtle)] xl:hidden">
                  <summary className="flex cursor-pointer list-none items-center justify-between px-4 py-2.5 text-sm font-medium text-[var(--fg-muted)] [&::-webkit-details-marker]:hidden">
                    On this page
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true" className="transition-transform group-open:rotate-180"><path d="m6 9 6 6 6-6" /></svg>
                  </summary>
                  <ul className="border-t border-[var(--border)] px-4 py-2 text-sm">
                    {headings.map((h) => (
                      <li key={h.id}>
                        <a href={`#${h.id}`} onClick={(e) => (e.currentTarget.closest('details') as HTMLDetailsElement).open = false} className="block py-1.5 text-[var(--fg-muted)] hover:text-[var(--fg)]">
                          {h.text}
                        </a>
                      </li>
                    ))}
                  </ul>
                </details>
              )}

              <main id="content" tabIndex={-1} className="prose-fogu text-left outline-none">
                {content}
              </main>

              {route && <RelatedExamples key={route.page} pageId={route.page} />}

              {/* previous / next */}
              {(prev || next) && (
                <nav aria-label="Previous and next page" className="mt-14 grid gap-3 border-t border-[var(--border)] pt-8 sm:grid-cols-2">
                  {prev ? (
                    <a href={`#${prev.id}`} className="frame-card group flex flex-col gap-1 p-4">
                      <span className="flex items-center gap-1.5 text-xs text-[var(--fg-faint)]"><Arrow left /> Previous</span>
                      <span className="font-semibold text-[var(--fg)] group-hover:text-[var(--accent)]">{prev.title}</span>
                      <span className="text-xs text-[var(--fg-faint)]">{prev.group}</span>
                    </a>
                  ) : (
                    <span className="hidden sm:block" />
                  )}
                  {next && (
                    <a href={`#${next.id}`} className="frame-card group flex flex-col items-end gap-1 p-4 text-right">
                      <span className="flex items-center gap-1.5 text-xs text-[var(--fg-faint)]">Next <Arrow /></span>
                      <span className="font-semibold text-[var(--fg)] group-hover:text-[var(--accent)]">{next.title}</span>
                      <span className="text-xs text-[var(--fg-faint)]">{next.group}</span>
                    </a>
                  )}
                </nav>
              )}

              <footer className="mt-14 flex flex-col gap-2 border-t border-[var(--border)] pt-6 text-[0.8rem] text-[var(--fg-faint)] sm:flex-row sm:justify-between">
                <span>Fogu v0.7.3 · MIT License · © 2026 Edmilson Soares</span>
                <span>Documentation for the Rust reference implementation.</span>
              </footer>
            </div>

            {/* "on this page" (wide screens) */}
            <aside className="sticky top-[calc(var(--header-h)+2rem)] hidden max-h-[calc(100vh-var(--header-h)-4rem)] w-56 shrink-0 self-start overflow-y-auto xl:block" aria-label="On this page">
              {headings.length > 0 && (
                <>
                  <p className="mb-3 flex items-center gap-2 text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--fg-faint)]">
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M4 6h16M4 12h10M4 18h13" /></svg>
                    On this page
                  </p>
                  <ul className="border-l border-[var(--border)] text-[0.8125rem]">
                    {headings.map((h) => (
                      <li key={h.id}>
                        <a href={`#${h.id}`} className={`toc-link ${current === h.id ? 'is-current' : ''}`} aria-current={current === h.id ? 'location' : undefined}>
                          {h.text}
                        </a>
                      </li>
                    ))}
                  </ul>
                </>
              )}
              <button
                type="button"
                onClick={() => window.scrollTo({ top: 0, behavior: 'smooth' })}
                className="mt-5 flex items-center gap-1.5 text-xs text-[var(--fg-faint)] hover:text-[var(--fg)]"
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="m18 15-6-6-6 6" /></svg>
                Back to top
              </button>
            </aside>
          </div>
        </div>
      </div>

      <SearchPalette open={searchOpen} onClose={() => setSearchOpen(false)} index={index} />

      {runnerCode && <InteractiveRunnerModal isOpen={!!runnerCode} onClose={() => setRunnerCode(null)} initialCode={runnerCode} />}
    </div>
  );
}
