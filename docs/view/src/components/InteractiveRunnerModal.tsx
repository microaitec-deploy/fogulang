import React, { useState, useEffect } from 'react';
import { X, Play, RotateCcw, Copy, Check, Terminal } from 'lucide-react';

interface InteractiveRunnerModalProps {
  isOpen: boolean;
  onClose: () => void;
  initialCode: string;
}

export const InteractiveRunnerModal: React.FC<InteractiveRunnerModalProps> = ({
  isOpen,
  onClose,
  initialCode
}) => {
  const [code, setCode] = useState(initialCode);
  const [output, setOutput] = useState<string[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setCode(initialCode);
    setOutput([]);
  }, [initialCode]);

  if (!isOpen) return null;

  const handleRun = () => {
    setIsRunning(true);
    setOutput(['$ fogu run script.fg']);

    setTimeout(() => {
      // Simulate realistic execution
      const logs: string[] = [];
      const lines = code.split('\n');
      
      for (const line of lines) {
        const trimmed = line.trim();
        if (trimmed.startsWith('print(')) {
          const content = trimmed.substring(6, trimmed.length - 1);
          logs.push(content.replace(/["'`]/g, ''));
        }
      }

      if (logs.length === 0) {
        logs.push('Program exited with code 0 (clean exit).');
      }

      setOutput((prev) => [...prev, ...logs]);
      setIsRunning(false);
    }, 250);
  };

  const handleCopy = () => {
    if (navigator.clipboard) {
      navigator.clipboard.writeText(code).then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      });
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-in fade-in duration-100">
      <div className="relative w-full max-w-3xl rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0e1118] shadow-2xl overflow-hidden flex flex-col max-h-[85vh]">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-slate-200 dark:border-slate-800">
          <div className="flex items-center gap-2">
            <span className="p-1 rounded-md bg-ember-100 dark:bg-ember-950/40 text-ember-600 dark:text-ember-400">
              <Terminal className="w-4 h-4" />
            </span>
            <span className="font-semibold text-sm text-slate-900 dark:text-white">Fogu Interactive Playground</span>
            <span className="text-xs text-slate-400 font-mono">(v0.7.2 runtime)</span>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Editor Area */}
        <div className="p-4 flex-1 flex flex-col gap-3 overflow-hidden">
          <div className="flex items-center justify-between text-xs text-slate-500">
            <span>Editor (Fogu .fg code)</span>
            <div className="flex items-center gap-2">
              <button
                onClick={() => setCode(initialCode)}
                className="flex items-center gap-1 hover:text-slate-800 dark:hover:text-slate-200"
              >
                <RotateCcw className="w-3.5 h-3.5" />
                <span>Reset</span>
              </button>
              <button
                onClick={handleCopy}
                className="flex items-center gap-1 hover:text-slate-800 dark:hover:text-slate-200"
              >
                {copied ? <Check className="w-3.5 h-3.5 text-emerald-500" /> : <Copy className="w-3.5 h-3.5" />}
                <span>{copied ? 'Copied' : 'Copy'}</span>
              </button>
            </div>
          </div>
          <textarea
            value={code}
            onChange={(e) => setCode(e.target.value)}
            className="w-full h-56 p-3 font-mono text-xs leading-relaxed rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-[#11141b] text-slate-800 dark:text-slate-200 focus:outline-none focus:ring-2 focus:ring-ember-500 resize-none"
            spellCheck={false}
          />

          {/* Action and Output */}
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Execution Output</span>
            <button
              onClick={handleRun}
              disabled={isRunning}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded-lg bg-ember-600 hover:bg-ember-500 text-white font-medium text-xs shadow-sm transition-colors"
            >
              <Play className="w-3.5 h-3.5 fill-white" />
              <span>{isRunning ? 'Running...' : 'Run Fogu'}</span>
            </button>
          </div>

          <div className="flex-1 min-h-[120px] p-3 rounded-xl border border-slate-200 dark:border-slate-800 bg-[#090b10] text-slate-200 font-mono text-xs overflow-y-auto">
            {output.length === 0 ? (
              <span className="text-slate-500">Click "Run Fogu" to evaluate this snippet in the interpreter...</span>
            ) : (
              output.map((line, idx) => (
                <div key={idx} className="leading-relaxed">
                  {line}
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
