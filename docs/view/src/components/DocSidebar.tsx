import React from 'react';

export interface TocItem {
  id: string;
  title: string;
}

export interface TocGroup {
  group: string;
  items: TocItem[];
}

export const TOC_GROUPS: TocGroup[] = [
  {
    group: 'Getting started',
    items: [
      { id: 'intro', title: 'Introduction' },
      { id: 'install', title: 'Installation' },
      { id: 'first-steps', title: 'First steps' }
    ]
  },
  {
    group: 'Enterprise & Automation',
    items: [
      { id: 'enterprise-arch', title: 'System Architecture' },
      { id: 'automation-devops', title: 'Automation & Orchestration' },
      { id: 'ai-agents', title: 'AI & Autonomous Agents' },
      { id: 'scalability-distributed', title: 'Distributed Scalability' },
      { id: 'enterprise-playground', title: 'Enterprise Scenarios Lab' }
    ]
  },
  {
    group: 'Language',
    items: [
      { id: 'variables', title: 'Variables and types' },
      { id: 'types', title: 'Type annotations' },
      { id: 'strings', title: 'Strings and templates' },
      { id: 'durations', title: 'Durations' },
      { id: 'operators', title: 'Operators' },
      { id: 'control', title: 'Control flow' },
      { id: 'functions', title: 'Functions' },
      { id: 'collections', title: 'Lists and maps' },
      { id: 'errors', title: 'Errors as values' },
      { id: 'structs', title: 'Structs and interfaces' },
      { id: 'modules', title: 'Modules and visibility' },
      { id: 'concurrency', title: 'Concurrency' }
    ]
  },
  {
    group: 'Guides',
    items: [
      { id: 'guides', title: 'About the guides' },
      { id: 'guide-cli', title: 'A command-line tool' },
      { id: 'guide-rest-api', title: 'A REST API with SQL and JWT' },
      { id: 'guide-topcoat', title: 'An interactive web app' },
      { id: 'guide-concurrency', title: 'Concurrency in practice' },
      { id: 'guide-errors', title: 'Error handling' },
      { id: 'guide-storage', title: 'Storage and data' },
      { id: 'guide-realtime', title: 'Realtime with WebSockets' },
      { id: 'guide-broker', title: 'Messaging with the broker' },
      { id: 'guide-automation', title: 'Automation and DevOps' },
      { id: 'guide-ai', title: 'AI agents' },
      { id: 'guide-mcp', title: 'MCP servers and clients' },
      { id: 'guide-embedding', title: 'Embedding Fogu' },
      { id: 'guide-deploy', title: 'Deploy everywhere' }
    ]
  },
  {
    group: 'Standard library',
    items: [
      { id: 'library', title: 'Overview' },
      { id: 'globals', title: 'Global functions' },
      { id: 'values', title: 'Strings, lists, maps and buffers' },
      { id: 'fogu-object', title: 'The Fogu object' }
    ]
  },
  {
    group: 'Core modules',
    items: [
      { id: 'mod-math', title: 'fogu:math' },
      { id: 'mod-json', title: 'fogu:json' },
      { id: 'mod-time', title: 'fogu:time' },
      { id: 'mod-system', title: 'fogu:system' },
      { id: 'mod-fs', title: 'fogu:fs' },
      { id: 'mod-path', title: 'fogu:path' },
      { id: 'mod-process', title: 'fogu:process' },
      { id: 'mod-sync', title: 'fogu:sync' },
      { id: 'mod-test', title: 'fogu:test' },
      { id: 'mod-log', title: 'fogu:log' },
      { id: 'mod-regex', title: 'fogu:regex' },
      { id: 'mod-crypto', title: 'fogu:crypto' }
    ]
  },
  {
    group: 'Data formats',
    items: [
      { id: 'mod-csv', title: 'fogu:csv' },
      { id: 'mod-excel', title: 'fogu:excel' },
      { id: 'mod-yaml', title: 'fogu:yaml' },
      { id: 'mod-toml', title: 'fogu:toml' },
      { id: 'mod-xml', title: 'fogu:xml' }
    ]
  },
  {
    group: 'Storage',
    items: [
      { id: 'mod-kv', title: 'fogu:kv' },
      { id: 'mod-surrealkv', title: 'fogu:surrealkv' },
      { id: 'mod-sql', title: 'fogu:sql' },
      { id: 'mod-orm', title: 'fogu:orm' },
      { id: 'mod-redis', title: 'fogu:redis' },
      { id: 'mod-s3', title: 'fogu:s3' },
      { id: 'mod-scylla', title: 'fogu:scylla' },
      { id: 'mod-surrealdb', title: 'fogu:surrealdb' },
      { id: 'mod-etcd', title: 'fogu:etcd' }
    ]
  },
  {
    group: 'Web & network',
    items: [
      { id: 'mod-http', title: 'fogu:http' },
      { id: 'mod-web', title: 'fogu:web' },
      { id: 'mod-topcoat', title: 'fogu:topcoat' },
      { id: 'mod-jwt', title: 'fogu:jwt' },
      { id: 'mod-websocket', title: 'fogu:websocket' },
      { id: 'mod-net', title: 'fogu:net' }
    ]
  },
  {
    group: 'Messaging & events',
    items: [
      { id: 'mod-nats', title: 'fogu:nats' },
      { id: 'mod-rabbitmq', title: 'fogu:rabbitmq' },
      { id: 'mod-mqtt', title: 'fogu:mqtt' },
      { id: 'mod-fogumq', title: 'fogu:fogumq' },
      { id: 'mod-foguMQ-client', title: 'fogu:foguMQ-client' },
      { id: 'mod-bullmq', title: 'fogu:bullmq' },
      { id: 'mod-events', title: 'fogu:events' },
      { id: 'mod-cron', title: 'fogu:cron' }
    ]
  },
  {
    group: 'Integrations',
    items: [
      { id: 'mod-mail', title: 'fogu:mail' },
      { id: 'mod-ai', title: 'fogu:ai' },
      { id: 'mod-ssh', title: 'fogu:ssh' },
      { id: 'mod-shell', title: 'fogu:shell' },
      { id: 'mod-ffi', title: 'fogu:ffi' },
      { id: 'mod-playbook', title: 'fogu:playbook' },
      { id: 'mod-vm', title: 'fogu:vm' },
      { id: 'mod-cli', title: 'fogu:cli' }
    ]
  },
  {
    group: 'HTML & browser',
    items: [
      { id: 'mod-html', title: 'fogu:html' },
      { id: 'mod-webview', title: 'fogu:webview' }
    ]
  },
  {
    group: 'Images & charts',
    items: [
      { id: 'mod-image', title: 'fogu:image' },
      { id: 'mod-plot', title: 'fogu:plot' }
    ]
  },
  {
    group: 'Containers & services',
    items: [
      { id: 'mod-docker', title: 'fogu:docker' },
      { id: 'mod-service', title: 'fogu:service' }
    ]
  },
  {
    group: 'Archives & serialization',
    items: [
      { id: 'mod-zip', title: 'fogu:zip' },
      { id: 'mod-protobuf', title: 'fogu:protobuf' }
    ]
  },
  {
    group: 'Tools',
    items: [
      { id: 'cli', title: 'CLI' },
      { id: 'diagnostics', title: 'Diagnostics' },
      { id: 'testing', title: 'Testing' },
      { id: 'packages', title: 'Packages' },
      { id: 'build', title: 'Standalone executables' },
      { id: 'jit', title: 'JIT' }
    ]
  },
  {
    group: 'Performance',
    items: [{ id: 'benchmarks', title: 'Benchmarks vs Python & Bun' }]
  },
  {
    group: 'Project',
    items: [
      { id: 'status', title: 'Status and roadmap' }
    ]
  }
];

interface DocSidebarProps {
  searchQuery: string;
  onSearchChange: (q: string) => void;
  activeId: string;
  onNavigate: (id: string) => void;
  isMobileOpen: boolean;
  onCloseMobile: () => void;
}

export const DocSidebar: React.FC<DocSidebarProps> = ({
  searchQuery,
  onSearchChange,
  activeId,
  onNavigate,
  isMobileOpen,
  onCloseMobile
}) => {
  const q = searchQuery.toLowerCase().trim();

  return (
    <aside
      id="menu"
      className={`${
        isMobileOpen ? 'block' : 'hidden'
      } lg:block fixed lg:sticky inset-0 top-14 z-20 lg:z-auto lg:h-[calc(100vh-3.5rem)] w-full lg:w-60 shrink-0 overflow-y-auto bg-white dark:bg-[#0b0d12] lg:bg-transparent px-4 lg:px-0 py-6 text-left border-r border-slate-100 dark:border-slate-800/40 lg:border-none`}
    >
      <input
        type="search"
        placeholder="Search…"
        value={searchQuery}
        onChange={(e) => onSearchChange(e.target.value)}
        className="md:hidden mb-4 w-full rounded-lg border border-slate-300 dark:border-slate-700 bg-transparent px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-amber-500"
      />
      <nav id="toc" className="text-sm space-y-6">
        {TOC_GROUPS.map((group) => {
          const matchingItems = group.items.filter((item) => {
            if (!q) return true;
            return (
              item.title.toLowerCase().includes(q) ||
              group.group.toLowerCase().includes(q)
            );
          });

          if (matchingItems.length === 0) return null;

          return (
            <div key={group.group}>
              <p className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
                {group.group}
              </p>
              <ul className="space-y-1 border-l border-slate-200 dark:border-slate-800">
                {matchingItems.map((item) => {
                  const isActive = activeId === item.id;
                  return (
                    <li key={item.id}>
                      <a
                        href={`#${item.id}`}
                        onClick={(e) => {
                          e.preventDefault();
                          onNavigate(item.id);
                          onCloseMobile();
                        }}
                        className={`block -ml-px border-l pl-3 py-0.5 text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white hover:border-slate-400 transition-colors ${
                          isActive
                            ? 'active border-amber-600 dark:border-amber-400 text-amber-600 dark:text-amber-400 font-semibold'
                            : 'border-transparent'
                        }`}
                      >
                        {item.title}
                      </a>
                    </li>
                  );
                })}
              </ul>
            </div>
          );
        })}
      </nav>
    </aside>
  );
};
