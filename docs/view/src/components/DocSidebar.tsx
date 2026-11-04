import React, { useEffect, useRef, useState } from 'react';

export interface NavPage {
  id: string;
  title: string;
  group: string;
}

export interface NavGroup {
  group: string;
  pages: NavPage[];
}

/**
 * The order of the sidebar's groups, and the sidebar titles of some pages.
 * Every page of the docs is a top-level `<section id data-group data-title>`;
 * the sidebar is built from them (App.tsx), so a new section shows up by itself.
 */
export const GROUP_ORDER = [
  'Getting started',
  'Enterprise & Automation',
  'Language',
  'Guides',
  'Examples',
  'Standard library',
  'Core modules',
  'Data formats',
  'Storage',
  'Web & network',
  'Messaging & events',
  'Integrations',
  'HTML & browser',
  'Images & charts',
  'Containers & services',
  'Archives & serialization',
  'Tools',
  'Performance',
  'Project',
];

export const TITLES: Record<string, string> = {
  library: 'Overview',
  guides: 'About the guides',
  benchmarks: 'Benchmarks vs Python & Bun',
};

const Chevron: React.FC<{ open: boolean }> = ({ open }) => (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
    className={`shrink-0 text-[var(--fg-faint)] transition-transform duration-200 ${open ? 'rotate-90' : ''}`}
  >
    <path d="m9 6 6 6-6 6" />
  </svg>
);

interface DocSidebarProps {
  groups: NavGroup[];
  activeId: string;
  isMobileOpen: boolean;
  onCloseMobile: () => void;
  onOpenSearch: () => void;
}

export const DocSidebar: React.FC<DocSidebarProps> = ({ groups, activeId, isMobileOpen, onCloseMobile, onOpenSearch }) => {
  const activeGroup = groups.find((g) => g.pages.some((p) => p.id === activeId))?.group;
  const [open, setOpen] = useState<Set<string>>(() => new Set(['Getting started']));
  const scroller = useRef<HTMLDivElement>(null);
  const closeBtn = useRef<HTMLButtonElement>(null);

  // the group of the current page is always open
  useEffect(() => {
    if (activeGroup) setOpen((s) => (s.has(activeGroup) ? s : new Set(s).add(activeGroup)));
  }, [activeGroup]);

  // keep the current link in view inside the sidebar (without moving the page)
  useEffect(() => {
    const box = scroller.current;
    const link = box?.querySelector<HTMLElement>('a[aria-current="page"]');
    if (!box || !link) return;
    const t = setTimeout(() => {
      const top = link.offsetTop - box.offsetTop;
      if (top < box.scrollTop + 40 || top > box.scrollTop + box.clientHeight - 60) {
        box.scrollTo({ top: Math.max(0, top - box.clientHeight / 3) });
      }
    }, 60);
    return () => clearTimeout(t);
  }, [activeId, groups.length]);

  // the drawer: Escape closes it, focus moves into it
  useEffect(() => {
    if (!isMobileOpen) return;
    closeBtn.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onCloseMobile();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [isMobileOpen, onCloseMobile]);

  const toggle = (g: string) =>
    setOpen((s) => {
      const n = new Set(s);
      if (n.has(g)) n.delete(g);
      else n.add(g);
      return n;
    });

  return (
    <>
      {/* phones and tablets: a backdrop behind the drawer */}
      <div
        className={`fixed inset-0 z-40 bg-black/40 backdrop-blur-[2px] transition-opacity duration-200 lg:hidden ${isMobileOpen ? 'opacity-100' : 'pointer-events-none opacity-0'}`}
        onClick={onCloseMobile}
        aria-hidden="true"
      />
      <aside
        id="menu"
        aria-label="Documentation"
        className={`sidebar fixed inset-y-0 left-0 z-50 flex w-[19rem] max-w-[86vw] flex-col border-r border-[var(--border)] bg-[var(--bg)] shadow-2xl transition-[transform,visibility] duration-250 ease-out lg:sticky lg:top-[var(--header-h)] lg:z-auto lg:h-[calc(100vh-var(--header-h))] lg:w-64 lg:max-w-none lg:shrink-0 lg:translate-x-0 lg:border-r-0 lg:bg-transparent lg:shadow-none lg:visible ${
          isMobileOpen ? 'translate-x-0 visible' : '-translate-x-full invisible'
        }`}
      >
        {/* drawer header (phones) */}
        <div className="flex h-[var(--header-h)] items-center justify-between border-b border-[var(--border)] px-4 lg:hidden">
          <span className="font-semibold">Documentation</span>
          <button
            ref={closeBtn}
            type="button"
            onClick={onCloseMobile}
            className="grid size-9 place-items-center rounded-lg text-[var(--fg-muted)] hover:bg-[var(--bg-hover)]"
            aria-label="Close the menu"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        </div>
        <div className="px-4 pt-4 lg:hidden">
          <button
            type="button"
            onClick={() => {
              onCloseMobile();
              onOpenSearch();
            }}
            className="flex w-full items-center gap-2 rounded-lg border border-[var(--border)] bg-[var(--bg-subtle)] px-3 py-2 text-sm text-[var(--fg-faint)]"
          >
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
            Search the docs…
          </button>
        </div>

        <div ref={scroller} className="sidebar-scroll min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-10 pt-4 lg:pl-0 lg:pr-4 lg:pt-7">
          <nav id="toc" className="space-y-0.5 text-[0.875rem]">
            {groups.map((g) => {
              const isOpen = open.has(g.group);
              const hasActive = g.group === activeGroup;
              const listId = 'nav-' + g.group.toLowerCase().replace(/[^a-z0-9]+/g, '-');
              return (
                <div key={g.group} className="pb-1">
                  <button
                    type="button"
                    onClick={() => toggle(g.group)}
                    aria-expanded={isOpen}
                    aria-controls={listId}
                    className={`side-group flex w-full items-center justify-between gap-2 rounded-md px-2.5 py-1.5 text-left text-[0.8125rem] font-semibold transition-colors hover:bg-[var(--bg-hover)] ${
                      hasActive ? 'text-[var(--fg)]' : 'text-[var(--fg-muted)]'
                    }`}
                  >
                    <span className="truncate">{g.group}</span>
                    <Chevron open={isOpen} />
                  </button>
                  <div className={`grid transition-[grid-template-rows] duration-200 ease-out ${isOpen ? 'grid-rows-[1fr]' : 'grid-rows-[0fr]'}`}>
                    <ul id={listId} className="overflow-hidden" inert={!isOpen}>
                      <li className="h-1" aria-hidden="true" />
                      {g.pages.map((p) => {
                        const active = p.id === activeId;
                        return (
                          <li key={p.id}>
                            <a
                              href={`#${p.id}`}
                              aria-current={active ? 'page' : undefined}
                              onClick={() => {
                                // the same page again: back to its top (no hashchange)
                                if (location.hash === '#' + p.id) window.scrollTo({ top: 0, behavior: 'smooth' });
                                onCloseMobile();
                              }}
                              className={`side-link ${active ? 'active' : ''}`}
                            >
                              {p.title.startsWith('fogu:') ? (
                                <>
                                  <span className="opacity-55">fogu:</span>
                                  {p.title.slice(5)}
                                </>
                              ) : (
                                p.title
                              )}
                            </a>
                          </li>
                        );
                      })}
                      <li className="h-1.5" aria-hidden="true" />
                    </ul>
                  </div>
                </div>
              );
            })}
          </nav>
        </div>
      </aside>
    </>
  );
};
