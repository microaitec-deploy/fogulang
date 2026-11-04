import React, { useEffect, useState } from 'react';
import { REPO } from '../data/examples';

const LOGO =
  'data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCA2NCA2NCIgd2lkdGg9IjY0IiBoZWlnaHQ9IjY0Ij48ZGVmcz48bGluZWFyR3JhZGllbnQgaWQ9ImYiIHgxPSIwIiB5MT0iMCIgeDI9IjAiIHkyPSIxIj48c3RvcCBvZmZzZXQ9IjAiIHN0b3AtY29sb3I9IiNmYmJmMjQiLz48c3RvcCBvZmZzZXQ9IjAuNSIgc3RvcC1jb2xvcj0iI2Y5NzMxNiIvPjxzdG9wIG9mZnNldD0iMSIgc3RvcC1jb2xvcj0iI2MyNDEwYyIvPjwvbGluZWFyR3JhZGllbnQ+PC9kZWZzPjxwYXRoIGZpbGw9InVybCgjZikiIGQ9Ik0zMyAxYzIgOSA4IDEzIDEzIDIwIDQgNSA4IDExIDggMTkgMCAxMi0xMCAyMi0yMiAyMlMxMCA1MiAxMCA0MGMwLTcgMy0xMyA3LTE3IDEgNSAzIDggNyAxMC0xLTggMS0xNCA0LTE5IDItMyA0LTcgNS0xM3oiLz48dGV4dCB4PSIzMiIgeT0iNTMiIHRleHQtYW5jaG9yPSJtaWRkbGUiIGZvbnQtZmFtaWx5PSInSmV0QnJhaW5zIE1vbm8nLCAnRGVqYVZ1IFNhbnMgTW9ubycsIE1lbmxvLCBDb25zb2xhcywgbW9ub3NwYWNlIiBmb250LXdlaWdodD0iNzAwIiBmb250LXNpemU9IjIyIiBmaWxsPSIjZmZmZmZmIiBsZXR0ZXItc3BhY2luZz0iLTIuNSI+LmZnPC90ZXh0Pjwvc3ZnPg==';

/** The header's links: [label, page id, the groups it stands for]. */
const LINKS: [string, string, string[]][] = [
  ['Docs', 'intro', ['Getting started', 'Language']],
  ['Guides', 'guides', ['Guides']],
  ['Reference', 'library', ['Standard library', 'Core modules', 'Data formats', 'Storage', 'Web & network', 'Messaging & events', 'Integrations', 'HTML & browser', 'Images & charts', 'Containers & services', 'Archives & serialization']],
  ['Examples', 'examples', ['Examples']],
  ['Enterprise', 'enterprise-arch', ['Enterprise & Automation']],
  ['CLI', 'cli', ['Tools']],
];

interface DocHeaderProps {
  onToggleMobileMenu: () => void;
  onOpenSearch: () => void;
  activeGroup?: string;
  menuOpen: boolean;
}

export const DocHeader: React.FC<DocHeaderProps> = ({ onToggleMobileMenu, onOpenSearch, activeGroup, menuOpen }) => {
  const [isDark, setIsDark] = useState(() => document.documentElement.classList.contains('dark'));
  const [scrolled, setScrolled] = useState(false);
  const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 4);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  const toggleTheme = () => {
    const next = !isDark;
    setIsDark(next);
    document.documentElement.classList.toggle('dark', next);
    try {
      localStorage.setItem('fogu-theme', next ? 'dark' : 'light');
    } catch {
      /* private mode */
    }
  };

  return (
    <header
      className={`sticky top-0 z-30 border-b bg-[var(--bg)]/80 backdrop-blur-xl backdrop-saturate-150 transition-[border-color,box-shadow] ${
        scrolled ? 'border-[var(--border)] shadow-[0_1px_0_rgba(0,0,0,0.02)]' : 'border-transparent'
      }`}
    >
      <div className="mx-auto flex h-[var(--header-h)] max-w-[90rem] items-center gap-2 px-4 sm:px-6">
        <button
          type="button"
          onClick={onToggleMobileMenu}
          className="icon-btn -ml-1.5 lg:hidden"
          aria-label="Open the navigation"
          aria-controls="menu"
          aria-expanded={menuOpen}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
            <path d="M4 7h16M4 12h16M4 17h10" />
          </svg>
        </button>

        <a href="#intro" className="flex shrink-0 items-center gap-2.5 rounded-lg font-semibold tracking-tight text-[var(--fg)]">
          <img src={LOGO} alt="" width="28" height="28" className="h-7 w-7 drop-shadow-[0_2px_6px_rgba(249,115,22,0.35)]" />
          <span className="text-[1.05rem]">Fogu</span>
          <span className="hidden text-[var(--fg-faint)] sm:inline">docs</span>
        </a>
        <span className="ml-1 hidden rounded-full border border-[var(--border)] bg-[var(--bg-subtle)] px-2 py-0.5 font-mono text-[0.68rem] font-medium text-[var(--fg-muted)] sm:inline">
          v0.7.3
        </span>

        <nav className="ml-5 hidden items-center gap-0.5 text-[0.875rem] xl:flex" aria-label="Main">
          {LINKS.map(([label, id, groups]) => (
            <a key={id} href={`#${id}`} className={`top-link ${activeGroup && groups.includes(activeGroup) ? 'is-current' : ''}`}>
              {label}
            </a>
          ))}
        </nav>

        <div className="flex-1" />

        <button
          type="button"
          onClick={onOpenSearch}
          className="group hidden h-9 w-60 items-center gap-2 rounded-lg border border-[var(--border)] bg-[var(--bg-subtle)] pl-3 pr-1.5 text-left text-sm text-[var(--fg-faint)] transition-colors hover:border-[var(--border-strong)] md:flex"
          aria-label="Search the documentation"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
          <span className="flex-1">Search docs…</span>
          <kbd className="key">{isMac ? '⌘' : 'Ctrl'}</kbd>
          <kbd className="key">K</kbd>
        </button>
        <button type="button" onClick={onOpenSearch} className="icon-btn md:hidden" aria-label="Search the documentation">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
        </button>

        <a href={REPO} className="icon-btn" aria-label="Fogu on GitHub" title="GitHub" target="_blank" rel="noreferrer">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.53-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.71 1.26 3.37.96.1-.75.4-1.26.73-1.55-2.56-.29-5.25-1.28-5.25-5.69 0-1.26.45-2.29 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.8 1.19 1.83 1.19 3.09 0 4.42-2.69 5.39-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z" />
          </svg>
        </a>
        <button
          type="button"
          onClick={toggleTheme}
          className="icon-btn"
          aria-label={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
          title={isDark ? 'Light mode' : 'Dark mode'}
        >
          {isDark ? (
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <circle cx="12" cy="12" r="4" />
              <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
            </svg>
          ) : (
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
            </svg>
          )}
        </button>
      </div>
    </header>
  );
};
