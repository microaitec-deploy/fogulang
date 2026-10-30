import React, { useState, useEffect } from 'react';

interface DocHeaderProps {
  onToggleMobileMenu: () => void;
  searchQuery: string;
  onSearchChange: (q: string) => void;
}

export const DocHeader: React.FC<DocHeaderProps> = ({
  onToggleMobileMenu,
  searchQuery,
  onSearchChange
}) => {
  const [isDark, setIsDark] = useState(false);

  useEffect(() => {
    // Check initial dark mode status from html element
    const checkDark = () => {
      setIsDark(document.documentElement.classList.contains('dark'));
    };
    checkDark();

    // Observe changes to html class in case changed externally
    const observer = new MutationObserver(checkDark);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => observer.disconnect();
  }, []);

  const toggleTheme = () => {
    const nextDark = !isDark;
    setIsDark(nextDark);
    if (nextDark) {
      document.documentElement.classList.add('dark');
      try {
        localStorage.setItem('fogu-theme', 'dark');
      } catch (e) {}
    } else {
      document.documentElement.classList.remove('dark');
      try {
        localStorage.setItem('fogu-theme', 'light');
      } catch (e) {}
    }
  };

  return (
    <header className="sticky top-0 z-30 border-b border-slate-200 dark:border-slate-800 bg-white/95 dark:bg-[#0b0d12]/95 backdrop-blur transition-colors">
      <div className="mx-auto max-w-7xl px-4 h-14 flex items-center gap-3">
        {/* Mobile menu toggle */}
        <button
          onClick={onToggleMobileMenu}
          className="lg:hidden -ml-1 p-2 rounded-md hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 transition-colors"
          aria-label="Open the table of contents"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M4 6h16M4 12h16M4 18h16" />
          </svg>
        </button>

        {/* Logo and Brand */}
        <a href="#intro" className="flex items-center gap-2 font-bold text-lg text-slate-900 dark:text-white transition-colors">
          <img
            src="data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCA2NCA2NCIgd2lkdGg9IjY0IiBoZWlnaHQ9IjY0Ij48ZGVmcz48bGluZWFyR3JhZGllbnQgaWQ9ImYiIHgxPSIwIiB5MT0iMCIgeDI9IjAiIHkyPSIxIj48c3RvcCBvZmZzZXQ9IjAiIHN0b3AtY29sb3I9IiNmYmJmMjQiLz48c3RvcCBvZmZzZXQ9IjAuNSIgc3RvcC1jb2xvcj0iI2Y5NzMxNiIvPjxzdG9wIG9mZnNldD0iMSIgc3RvcC1jb2xvcj0iI2MyNDEwYyIvPjwvbGluZWFyR3JhZGllbnQ+PC9kZWZzPjxwYXRoIGZpbGw9InVybCgjZikiIGQ9Ik0zMyAxYzIgOSA4IDEzIDEzIDIwIDQgNSA4IDExIDggMTkgMCAxMi0xMCAyMi0yMiAyMlMxMCA1MiAxMCA0MGMwLTcgMy0xMyA3LTE3IDEgNSAzIDggNyAxMC0xLTggMS0xNCA0LTE5IDItMyA0LTcgNS0xM3oiLz48dGV4dCB4PSIzMiIgeT0iNTMiIHRleHQtYW5jaG9yPSJtaWRkbGUiIGZvbnQtZmFtaWx5PSInSmV0QnJhaW5zIE1vbm8nLCAnRGVqYVZ1IFNhbnMgTW9ubycsIE1lbmxvLCBDb25zb2xhcywgbW9ub3NwYWNlIiBmb250LXdlaWdodD0iNzAwIiBmb250LXNpemU9IjIyIiBmaWxsPSIjZmZmZmZmIiBsZXR0ZXItc3BhY2luZz0iLTIuNSI+LmZnPC90ZXh0Pjwvc3ZnPg=="
            alt=""
            width="28"
            height="28"
            className="h-7 w-7"
          />
          <span>Fogu</span>
        </a>
        <span className="hidden sm:inline text-xs rounded-full border border-amber-200 dark:border-amber-900 text-amber-700 dark:text-amber-400 px-2 py-0.5">
          v0.7.2
        </span>

        {/* Quick Nav Anchors for Enterprise & Automation */}
        <nav className="hidden xl:flex items-center gap-1 ml-4 text-xs font-medium text-slate-600 dark:text-slate-400">
          <a href="#intro" className="px-2.5 py-1 rounded-md hover:bg-slate-100 dark:hover:bg-slate-800 hover:text-slate-900 dark:hover:text-white transition-colors">
            Docs
          </a>
          <a href="#enterprise-arch" className="px-2.5 py-1 rounded-md hover:bg-slate-100 dark:hover:bg-slate-800 hover:text-slate-900 dark:hover:text-white transition-colors">
            Architecture
          </a>
          <a href="#automation-devops" className="px-2.5 py-1 rounded-md hover:bg-slate-100 dark:hover:bg-slate-800 hover:text-slate-900 dark:hover:text-white transition-colors text-orange-600 dark:text-orange-400 font-semibold">
            Automation
          </a>
          <a href="#ai-agents" className="px-2.5 py-1 rounded-md hover:bg-slate-100 dark:hover:bg-slate-800 hover:text-slate-900 dark:hover:text-white transition-colors text-purple-600 dark:text-purple-400 font-semibold">
            AI Agents
          </a>
          <a href="#scalability-distributed" className="px-2.5 py-1 rounded-md hover:bg-slate-100 dark:hover:bg-slate-800 hover:text-slate-900 dark:hover:text-white transition-colors">
            Scalability
          </a>
          <a href="#enterprise-playground" className="px-2.5 py-1 rounded-md hover:bg-amber-50 dark:hover:bg-amber-950/40 text-amber-600 dark:text-amber-400 transition-colors font-semibold">
            Interactive Lab
          </a>
        </nav>

        <div className="flex-1" />

        {/* Desktop search */}
        <input
          id="search"
          type="search"
          placeholder="Search documentation…"
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          className="hidden md:block w-56 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50/60 dark:bg-slate-900/60 px-3 py-1.5 text-sm text-slate-900 dark:text-slate-100 placeholder-slate-400 dark:placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-amber-500 transition-all"
        />

        {/* Theme toggle */}
        <button
          onClick={toggleTheme}
          type="button"
          className="flex items-center gap-1.5 p-2 rounded-lg border border-slate-200 dark:border-slate-800 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 transition-colors"
          aria-label={isDark ? "Switch to light mode" : "Switch to dark mode"}
          title={isDark ? "Switch to light mode" : "Switch to dark mode"}
        >
          {isDark ? (
            <>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-amber-400">
                <circle cx="12" cy="12" r="4" />
                <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
              </svg>
              <span className="text-xs font-medium hidden sm:inline text-slate-300">Light</span>
            </>
          ) : (
            <>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-slate-700">
                <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
              </svg>
              <span className="text-xs font-medium hidden sm:inline text-slate-600">Dark</span>
            </>
          )}
        </button>
      </div>
    </header>
  );
};
