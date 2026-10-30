import React, { useState } from 'react';
import { Play, RotateCcw, Copy, Download, Check, Terminal, Cpu, Clock, Layers, Zap } from 'lucide-react';
import { ENTERPRISE_SCENARIOS, EnterpriseScenario } from '../data/foguDocsData';

export const InteractivePlayground: React.FC = () => {
  const [selectedScenario, setSelectedScenario] = useState<EnterpriseScenario>(ENTERPRISE_SCENARIOS[0]);
  const [currentCode, setCurrentCode] = useState<string>(ENTERPRISE_SCENARIOS[0].code);
  const [isRunning, setIsRunning] = useState<boolean>(false);
  const [executionLogs, setExecutionLogs] = useState<string[]>(ENTERPRISE_SCENARIOS[0].simulatedOutput);
  const [copied, setCopied] = useState<boolean>(false);

  const handleSelectScenario = (scenario: EnterpriseScenario) => {
    setSelectedScenario(scenario);
    setCurrentCode(scenario.code);
    setExecutionLogs(scenario.simulatedOutput);
    setIsRunning(false);
  };

  const handleReset = () => {
    setCurrentCode(selectedScenario.code);
    setExecutionLogs(selectedScenario.simulatedOutput);
  };

  const handleRun = () => {
    setIsRunning(true);
    setExecutionLogs(['[FOGU RUNTIME] Initializing interpreter with Cranelift JIT compiler enabled...']);

    let step = 0;
    const interval = setInterval(() => {
      step++;
      if (step <= selectedScenario.simulatedOutput.length) {
        setExecutionLogs((prev) => [...prev, selectedScenario.simulatedOutput[step - 1]]);
      } else {
        clearInterval(interval);
        setIsRunning(false);
        setExecutionLogs((prev) => [
          ...prev,
          `[PROCESS TERMINATED] Exit code 0 (Success in ${selectedScenario.executionStats.duration})`
        ]);
      }
    }, 160);
  };

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(currentCode);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // ignore
    }
  };

  const handleDownload = () => {
    const blob = new Blob([currentCode], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${selectedScenario.id}.fg`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <section id="playground" className="scroll-mt-20 my-16 text-left">
      <div className="flex flex-col md:flex-row md:items-end justify-between gap-4 pb-6 border-b border-slate-200 dark:border-slate-800">
        <div>
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-amber-600 dark:text-amber-400">
            <Terminal className="w-4 h-4" />
            <span>Interactive Execution Environment</span>
          </div>
          <h2 className="mt-1 text-3xl font-bold tracking-tight text-slate-900 dark:text-white">
            Enterprise Playground & Simulator
          </h2>
          <p className="mt-2 text-sm text-slate-600 dark:text-slate-400 max-w-2xl">
            Test and run production-grade architectures in real-time featuring cooperative concurrency,
            distributed queues, IoT ingestion, and autonomous AI agents with database governance.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handleReset}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-slate-800 text-xs text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800 transition-colors"
          >
            <RotateCcw className="w-3.5 h-3.5" />
            <span>Reset Code</span>
          </button>
          <button
            onClick={handleDownload}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-slate-800 text-xs text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800 transition-colors"
          >
            <Download className="w-3.5 h-3.5" />
            <span>Download .fg</span>
          </button>
          <button
            onClick={handleRun}
            disabled={isRunning}
            className={`flex items-center gap-2 px-5 py-2 rounded-lg text-xs font-semibold text-white transition-all shadow-sm ${
              isRunning
                ? 'bg-amber-400 cursor-not-allowed'
                : 'bg-amber-600 hover:bg-amber-500 active:scale-95'
            }`}
          >
            <Play className={`w-3.5 h-3.5 fill-white ${isRunning ? 'animate-spin' : ''}`} />
            <span>{isRunning ? 'Running...' : 'Run Fogu'}</span>
          </button>
        </div>
      </div>

      {/* Scenario Selection Tabs */}
      <div className="mt-6 flex flex-wrap gap-2 p-1.5 bg-slate-100 dark:bg-slate-900/60 rounded-xl border border-slate-200 dark:border-slate-800">
        {ENTERPRISE_SCENARIOS.map((sc) => {
          const isSelected = selectedScenario.id === sc.id;
          return (
            <button
              key={sc.id}
              onClick={() => handleSelectScenario(sc)}
              className={`flex-1 min-w-[220px] text-left px-4 py-2.5 rounded-lg text-xs transition-all ${
                isSelected
                  ? 'bg-white dark:bg-slate-800 text-slate-900 dark:text-white font-medium shadow-sm ring-1 ring-slate-200 dark:ring-slate-700'
                  : 'text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-slate-200'
              }`}
            >
              <div className="font-semibold">{sc.title}</div>
              <div className="text-[11px] font-mono text-slate-500 dark:text-slate-400 truncate mt-0.5">
                {sc.subtitle}
              </div>
            </button>
          );
        })}
      </div>

      {/* Scenario Description and Architecture Notes */}
      <div className="mt-4 p-4 rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0e1118]">
        <div className="flex flex-col md:flex-row md:items-center justify-between gap-2 pb-3 border-b border-slate-100 dark:border-slate-800/80">
          <div>
            <h4 className="font-semibold text-slate-900 dark:text-white text-sm">
              {selectedScenario.title}
            </h4>
            <p className="text-xs text-slate-600 dark:text-slate-400 mt-0.5">
              {selectedScenario.description}
            </p>
          </div>
          <div className="flex items-center gap-2 text-xs font-mono text-amber-600 dark:text-amber-400 whitespace-nowrap">
            <span>{selectedScenario.subtitle}</span>
          </div>
        </div>

        <div className="mt-3 grid gap-2 sm:grid-cols-2 text-xs text-slate-600 dark:text-slate-400">
          {selectedScenario.architectureNotes.map((note, idx) => (
            <div key={idx} className="flex items-start gap-1.5">
              <span className="text-amber-500 font-bold shrink-0">✓</span>
              <span>{note}</span>
            </div>
          ))}
        </div>
      </div>

      {/* Main Split: Code Editor & Live Console */}
      <div className="mt-6 grid gap-6 lg:grid-cols-12 items-stretch">
        {/* Editor Area (7 Cols) */}
        <div className="lg:col-span-7 flex flex-col rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0e1118] overflow-hidden shadow-sm">
          <div className="flex items-center justify-between px-4 py-2 border-b border-slate-200 dark:border-slate-800 bg-slate-50/80 dark:bg-slate-900/60 text-xs">
            <div className="flex items-center gap-2">
              <span className="font-mono font-medium text-slate-700 dark:text-slate-300">
                {selectedScenario.id}.fg
              </span>
              <span className="text-slate-400 font-mono text-[11px]">(editable)</span>
            </div>
            <button
              onClick={handleCopy}
              className="flex items-center gap-1 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200"
            >
              {copied ? (
                <>
                  <Check className="w-3.5 h-3.5 text-emerald-500" />
                  <span className="text-emerald-500 text-[11px]">Copied</span>
                </>
              ) : (
                <>
                  <Copy className="w-3.5 h-3.5" />
                  <span className="text-[11px]">Copy</span>
                </>
              )}
            </button>
          </div>

          <div className="p-2 flex-1">
            <textarea
              value={currentCode}
              onChange={(e) => setCurrentCode(e.target.value)}
              className="w-full h-[460px] p-3 font-mono text-xs leading-relaxed bg-transparent text-slate-800 dark:text-slate-200 resize-none focus:outline-none focus:ring-1 focus:ring-amber-500/50 rounded-lg selection:bg-amber-500/20"
              spellCheck={false}
            />
          </div>
        </div>

        {/* Console & Metrics (5 Cols) */}
        <div className="lg:col-span-5 flex flex-col gap-4">
          {/* Performance Telemetry Strip */}
          <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-2 gap-3">
            <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0e1118] p-3 text-left">
              <div className="flex items-center gap-1.5 text-slate-400 text-xs">
                <Clock className="w-3.5 h-3.5 text-amber-500" />
                <span>P99 Latency</span>
              </div>
              <div className="mt-1 text-base font-bold font-mono text-slate-900 dark:text-white tabular-nums">
                {selectedScenario.executionStats.duration}
              </div>
            </div>

            <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0e1118] p-3 text-left">
              <div className="flex items-center gap-1.5 text-slate-400 text-xs">
                <Zap className="w-3.5 h-3.5 text-amber-500" />
                <span>Throughput</span>
              </div>
              <div className="mt-1 text-base font-bold font-mono text-slate-900 dark:text-white tabular-nums">
                {selectedScenario.executionStats.throughput}
              </div>
            </div>

            <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0e1118] p-3 text-left">
              <div className="flex items-center gap-1.5 text-slate-400 text-xs">
                <Layers className="w-3.5 h-3.5 text-blue-500" />
                <span>Fibers (fg)</span>
              </div>
              <div className="mt-1 text-base font-bold font-mono text-slate-900 dark:text-white tabular-nums">
                {selectedScenario.executionStats.tasksCount} tasks
              </div>
            </div>

            <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0e1118] p-3 text-left">
              <div className="flex items-center gap-1.5 text-slate-400 text-xs">
                <Cpu className="w-3.5 h-3.5 text-emerald-500" />
                <span>Cranelift JIT</span>
              </div>
              <div className="mt-1 text-base font-bold font-mono text-emerald-600 dark:text-emerald-400 tabular-nums">
                {selectedScenario.executionStats.jitActive ? 'ACTIVE' : 'INTERPRETER'}
              </div>
            </div>
          </div>

          {/* Real-time Output Terminal */}
          <div className="flex-1 flex flex-col rounded-xl border border-slate-200 dark:border-slate-800 bg-[#090b10] text-slate-200 overflow-hidden shadow-sm min-h-[340px]">
            <div className="flex items-center justify-between px-4 py-2 border-b border-slate-800 bg-slate-950 text-xs text-slate-400 font-mono">
              <div className="flex items-center gap-2">
                <span className="w-2.5 h-2.5 rounded-full bg-emerald-500 inline-block" />
                <span>stdout / stderr (fogu run)</span>
              </div>
              <span className="text-[11px] text-slate-500">v0.7.2 (Rust)</span>
            </div>

            <div className="p-4 flex-1 font-mono text-xs leading-relaxed overflow-y-auto space-y-2">
              {executionLogs.map((log, idx) => {
                const isError = log.includes('ERROR') || log.includes('error') || log.includes('FAILURE');
                const isAlert = log.includes('CRITICAL') || log.includes('ALERT');
                const isSuccess = log.includes('SUCCESS') || log.includes('COMPLETED') || log.includes('ok');

                let textColor = 'text-slate-300';
                if (isError) textColor = 'text-rose-400 font-medium';
                else if (isAlert) textColor = 'text-amber-400 font-medium';
                else if (isSuccess) textColor = 'text-emerald-400';

                return (
                  <div key={idx} className={`${textColor} break-all flex items-start gap-2`}>
                    <span className="text-slate-600 select-none text-[11px]">&gt;</span>
                    <span>{log}</span>
                  </div>
                );
              })}

              {isRunning && (
                <div className="text-amber-400 font-mono text-xs flex items-center gap-2 animate-pulse pt-2">
                  <span>Processing asynchronous pipeline...</span>
                </div>
              )}
            </div>

            <div className="px-4 py-2 border-t border-slate-800/80 bg-slate-950/60 text-[11px] text-slate-500 font-mono flex items-center justify-between">
              <span>RSS Memory: {selectedScenario.executionStats.memory}</span>
              <span>Zero-cost Async Abstractions</span>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
};
