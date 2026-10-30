import React, { useState, useEffect } from 'react';
import { DocHeader } from './components/DocHeader';
import { DocSidebar } from './components/DocSidebar';
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
import { BenchmarksSection } from './components/sections/BenchmarksSection';
import { InteractiveRunnerModal } from './components/InteractiveRunnerModal';

export default function App() {
  const [searchQuery, setSearchQuery] = useState('');
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);
  const [activeSectionId, setActiveSectionId] = useState('intro');
  const [runnerCode, setRunnerCode] = useState<string | null>(null);

  // Smooth navigation to target element
  const handleNavigate = (id: string) => {
    setActiveSectionId(id);
    const element = document.getElementById(id);
    if (element) {
      element.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  };

  // Open interactive test runner for a code snippet
  const handleRunSnippet = (code: string) => {
    setRunnerCode(code);
  };

  // Setup IntersectionObserver for active section tracking
  useEffect(() => {
    const sections = Array.from(document.querySelectorAll('main section[id]'));
    if (sections.length === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            setActiveSectionId(entry.target.id);
          }
        });
      },
      { rootMargin: '-20% 0px -70% 0px' }
    );

    sections.forEach((sec) => observer.observe(sec));
    return () => observer.disconnect();
  }, []);

  // Filter sections when search query changes
  useEffect(() => {
    const q = searchQuery.trim().toLowerCase();
    const sections = Array.from(document.querySelectorAll('main section[id]')) as HTMLElement[];

    sections.forEach((sec) => {
      if (!q) {
        sec.style.display = '';
      } else {
        const text = (sec.textContent || '').toLowerCase();
        const matches = text.includes(q);
        sec.style.display = matches ? '' : 'none';
      }
    });
  }, [searchQuery]);

  return (
    <div className="min-h-screen bg-white dark:bg-[#0b0d12] text-slate-800 dark:text-slate-200">
      {/* Top bar */}
      <DocHeader
        onToggleMobileMenu={() => setIsMobileMenuOpen(!isMobileMenuOpen)}
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
      />

      {/* Main Content Layout with Sidebar */}
      <div className="mx-auto max-w-7xl px-4 lg:flex lg:gap-10">
        {/* Table of Contents Sidebar */}
        <DocSidebar
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
          activeId={activeSectionId}
          onNavigate={handleNavigate}
          isMobileOpen={isMobileMenuOpen}
          onCloseMobile={() => setIsMobileMenuOpen(false)}
        />

        {/* Documentation Content */}
        <main id="content" className="min-w-0 flex-1 py-10 prose-fogu text-left">
          <GettingStartedSection onRun={handleRunSnippet} />
          <EnterpriseAutomationSection onRun={handleRunSnippet} />
          <LanguageSection onRun={handleRunSnippet} />
          <GuidesSection onRun={handleRunSnippet} />
          <CoreModulesSection onRun={handleRunSnippet} />
          <DataAndStorageSection onRun={handleRunSnippet} />
          <WebAndMessagingSection onRun={handleRunSnippet} />
          <IntegrationsSection onRun={handleRunSnippet} />
          <AdvancedModulesSection onRun={handleRunSnippet} />
          <ToolsAndProjectSection onRun={handleRunSnippet} />
          <BenchmarksSection />

          <footer className="mt-20 border-t border-slate-200 dark:border-slate-800 pt-6 pb-10 text-sm text-slate-500">
            Fogu v0.7.0 · MIT License · © 2026 Edmilson Soares · Documentation for the Rust reference implementation.
          </footer>
        </main>
      </div>

      {/* Optional Interactive Snippet Runner Modal */}
      {runnerCode && (
        <InteractiveRunnerModal
          isOpen={!!runnerCode}
          onClose={() => setRunnerCode(null)}
          initialCode={runnerCode}
        />
      )}
    </div>
  );
}
