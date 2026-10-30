import React, { useState } from 'react';
import { Layers, Zap, Database, Cpu, Shield, ArrowRight } from 'lucide-react';

export const ArchitectureDiagram: React.FC = () => {
  const [selectedLayer, setSelectedLayer] = useState<number | null>(null);

  const layers = [
    {
      id: 1,
      title: '1. Ingress & Edge Gateway',
      tech: 'fogu:web + fogu:jwt',
      icon: Shield,
      description: 'TLS termination, HTTP/2 and WebSocket multiplexing, rate limiting, and zero-allocation cryptographic JWT verification.',
      details: [
        'Up to 180,000 req/s on standard 4 vCPU instances',
        'Zero-Trust auth middleware with zero extraneous heap allocations',
        'Bidirectional streaming via SSE and WebSockets on the same port'
      ]
    },
    {
      id: 2,
      title: '2. Fogu Concurrent Runtime',
      tech: 'fg (Green Threads) + Cranelift JIT',
      icon: Cpu,
      description: 'Ultra-lightweight cooperative execution in Rust; thousands of concurrent tasks with minimal RSS memory footprint.',
      details: [
        'Only ~4KB overhead per cooperative fg task',
        'JIT compilation for numerical loops and hot paths',
        'Strict runtime deadlock detection across channels'
      ]
    },
    {
      id: 3,
      title: '3. Distributed Messaging Backplane',
      tech: 'fogu:nats (JetStream) + fogu:bullmq',
      icon: Zap,
      description: 'Asynchronous decoupling with at-least-once delivery, msgId deduplication, and resilient queues with backoff.',
      details: [
        'NATS JetStream: high-frequency ingestion (millions of msgs/min)',
        'BullMQ: reliable workers with automated stalled job recovery',
        'Wire-protocol compatibility with Bull Board and Node.js workers'
      ]
    },
    {
      id: 4,
      title: '4. Hybrid Persistence Tier',
      tech: 'fogu:scylla + fogu:s3 + fogu:orm',
      icon: Database,
      description: 'Multi-storage topology: massive columnar writes to ScyllaDB, object storage in S3, and ACID relational data with ORM.',
      details: [
        'ScyllaDB: sub-millisecond P99 latency for time-series and ledgers',
        'S3/MinIO: storage of signed receipts, reports, and binary artifacts',
        'fogu:orm: declarative relational schemas with auto-migrations'
      ]
    }
  ];

  return (
    <div className="my-8 rounded-2xl border border-slate-200 dark:border-slate-800 bg-gradient-to-b from-slate-50 to-white dark:from-[#0d1017] dark:to-[#080a0f] p-6 lg:p-8 shadow-sm">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-6 border-b border-slate-200 dark:border-slate-800">
        <div>
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-amber-600 dark:text-amber-400">
            <Layers className="w-4 h-4" />
            <span>Enterprise High-Scalability Topology</span>
          </div>
          <h3 className="mt-1 text-xl sm:text-2xl font-bold text-slate-900 dark:text-white">
            Distributed Concurrency & Systems Architecture
          </h3>
        </div>
        <div className="text-xs text-slate-500 font-mono">
          Throughput: 100k+ ops/s · Memory &lt; 35MB
        </div>
      </div>

      {/* Interactive Architecture Flow */}
      <div className="mt-6 grid gap-4 lg:grid-cols-4">
        {layers.map((layer) => {
          const Icon = layer.icon;
          const isSelected = selectedLayer === layer.id;

          return (
            <div
              key={layer.id}
              onClick={() => setSelectedLayer(isSelected ? null : layer.id)}
              className={`cursor-pointer rounded-xl border p-5 transition-all text-left ${
                isSelected
                  ? 'border-amber-500 bg-amber-50/50 dark:bg-amber-950/20 shadow-md ring-1 ring-amber-500'
                  : 'border-slate-200 dark:border-slate-800 bg-white dark:bg-[#11141c] hover:border-slate-300 dark:hover:border-slate-700 hover:shadow-sm'
              }`}
            >
              <div className="flex items-center justify-between mb-3">
                <span className="p-2 rounded-lg bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-400">
                  <Icon className="w-5 h-5" />
                </span>
                <span className="text-[11px] font-mono text-slate-400">Layer {layer.id}</span>
              </div>
              <h4 className="font-semibold text-slate-900 dark:text-slate-100 text-sm">
                {layer.title}
              </h4>
              <p className="mt-1 text-xs font-mono text-amber-600 dark:text-amber-400">
                {layer.tech}
              </p>
              <p className="mt-2 text-xs text-slate-600 dark:text-slate-400 line-clamp-2">
                {layer.description}
              </p>

              <div className="mt-4 pt-3 border-t border-slate-100 dark:border-slate-800 text-[11px] font-medium text-slate-500 flex items-center justify-between">
                <span>{isSelected ? 'Hide specs' : 'View specifications'}</span>
                <ArrowRight className={`w-3.5 h-3.5 transition-transform ${isSelected ? 'rotate-90' : ''}`} />
              </div>

              {isSelected && (
                <div className="mt-3 pt-3 border-t border-amber-200 dark:border-amber-900/40 text-xs text-slate-600 dark:text-slate-300 space-y-1.5 animate-in fade-in duration-150">
                  {layer.details.map((detail, idx) => (
                    <div key={idx} className="flex items-start gap-1.5">
                      <span className="text-amber-500 font-bold">•</span>
                      <span>{detail}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* Pipeline Summary Bar */}
      <div className="mt-6 rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-100/60 dark:bg-slate-900/40 p-4 text-xs text-slate-600 dark:text-slate-400 flex flex-col md:flex-row md:items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <span className="font-semibold text-slate-800 dark:text-slate-200">Data Pipeline Flow:</span>
          <span>HTTP/WS Ingress → Cooperative Fibers (fg) → JetStream/BullMQ → ScyllaDB & S3 Cluster</span>
        </div>
        <div className="font-mono text-emerald-600 dark:text-emerald-400 font-medium">
          Zero Garbage Collection Pauses · Rust Reference Core
        </div>
      </div>
    </div>
  );
};
