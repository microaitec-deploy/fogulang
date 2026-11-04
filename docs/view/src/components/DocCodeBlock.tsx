import React, { useState } from 'react';
import { highlight } from './highlight';

interface DocCodeBlockProps {
  code: string;
  lang?: string;
  title?: string;
  onRun?: (code: string) => void;
  /** Extra content in the header, on the right (e.g. a link to the source). */
  actions?: React.ReactNode;
  className?: string;
}

const LANG_LABEL: Record<string, string> = {
  fogu: 'Fogu',
  bash: 'Shell',
  sh: 'Shell',
  powershell: 'PowerShell',
  js: 'JavaScript',
  json: 'JSON',
  yaml: 'YAML',
  text: 'Text',
  markdown: 'Markdown',
};

/** Copies text to the clipboard (with a fallback for non-secure contexts). */
export function copyText(text: string): Promise<void> {
  if (navigator.clipboard && window.isSecureContext) return navigator.clipboard.writeText(text);
  const area = document.createElement('textarea');
  area.value = text;
  area.style.position = 'fixed';
  area.style.opacity = '0';
  document.body.appendChild(area);
  area.select();
  try {
    document.execCommand('copy');
  } finally {
    document.body.removeChild(area);
  }
  return Promise.resolve();
}

export const CopyButton: React.FC<{ text: string; compact?: boolean }> = ({ text, compact }) => {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      onClick={() =>
        copyText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1600);
        })
      }
      className={`code-btn ${copied ? 'is-done' : ''}`}
      aria-label={copied ? 'Copied' : 'Copy code'}
      title="Copy"
    >
      {copied ? (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>
      ) : (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
      )}
      {!compact && <span>{copied ? 'Copied' : 'Copy'}</span>}
    </button>
  );
};

export const DocCodeBlock: React.FC<DocCodeBlockProps> = ({ code, lang = 'fogu', title, onRun, actions, className = '' }) => {
  const clean = code.replace(/^\n/, '').replace(/\s+$/, '');
  const html = highlight(clean, lang);
  const isOutput = lang === 'text' && /output|stdout|prints/i.test(title ?? '');

  // a one-line import: a slim bar with the copy button
  if (title === 'import' && !clean.includes('\n')) {
    return (
      <div className={`codeblock codeblock-inline group ${className}`}>
        <pre><code dangerouslySetInnerHTML={{ __html: html }} /></pre>
        <CopyButton text={clean} compact />
      </div>
    );
  }

  const label = isOutput ? 'Output' : LANG_LABEL[lang] ?? lang;
  const name = title && title.toLowerCase() !== lang && title.toLowerCase() !== label.toLowerCase() && !isOutput ? title : null;

  return (
    <figure className={`codeblock ${isOutput ? 'codeblock-output' : ''} ${className}`}>
      <figcaption className="codeblock-head">
        <span className="flex min-w-0 items-center gap-2">
          <span className={`codeblock-lang lang-${isOutput ? 'output' : lang}`}>
            {isOutput ? (
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m4 17 6-6-6-6M12 19h8" /></svg>
            ) : (
              <span className="dot" aria-hidden="true" />
            )}
            {label}
          </span>
          {name && <span className="codeblock-title">{name}</span>}
        </span>
        <span className="flex shrink-0 items-center gap-1">
          {actions}
          {onRun && lang === 'fogu' && (
            <button type="button" onClick={() => onRun(clean)} className="code-btn code-btn-run" aria-label="Run this example" title="Run">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.5v15a1 1 0 0 0 1.5.86l12-7.5a1 1 0 0 0 0-1.72l-12-7.5A1 1 0 0 0 7 4.5Z" /></svg>
              <span>Run</span>
            </button>
          )}
          <CopyButton text={clean} />
        </span>
      </figcaption>
      <pre tabIndex={0}><code dangerouslySetInnerHTML={{ __html: html }} /></pre>
    </figure>
  );
};
