import React from 'react';
import { DocCodeBlock } from './DocCodeBlock';
import { REFERENCE } from '../data/reference.generated';

/** One block of a module's reference (see docs/tools/reference/*.py). */
export interface RefBlock {
  h3?: string;
  text?: string;
  note?: string;
  /** [signature, returns, description (HTML)] */
  api?: [string, string, string][];
  example?: string;
  lang?: string;
  title?: string;
  output?: string;
  outputTitle?: string;
}

export interface RefData {
  id: string;
  group: string;
  title: string;
  import?: string | null;
  intro?: string | null;
  blocks: RefBlock[];
}

interface Props {
  /** The section id in docs/tools/reference (e.g. "mod-grpc"). */
  id: string;
  onRun?: (code: string) => void;
}

/** The generated text is ours (docs/tools/reference), so its HTML is trusted. */
const Html: React.FC<{ html: string; as?: 'p' | 'span' | 'td' | 'div'; className?: string }> = ({ html, as = 'span', className }) =>
  React.createElement(as, { className, dangerouslySetInnerHTML: { __html: html } });

/** A module section rendered from the generated reference: always in sync with the code. */
export const RefSection: React.FC<Props> = ({ id, onRun }) => {
  const s = REFERENCE[id];
  if (!s) return null;
  const decode = (t: string) => t.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&').replace(/&quot;/g, '"');
  return (
    <section id={s.id} data-group={s.group} data-title={s.title} className="scroll-mt-20 mt-16">
      <h2 className="text-2xl font-bold">{s.title.startsWith('fogu:') ? <code>{s.title}</code> : s.title}</h2>
      {s.intro && <Html as="p" html={s.intro} />}
      {s.import && <DocCodeBlock lang="fogu" title="import" code={s.import} />}
      {s.blocks.map((b, i) => (
        <React.Fragment key={i}>
          {b.h3 && <h3 className="text-lg font-semibold mt-6">{decode(b.h3)}</h3>}
          {b.text && <Html as="p" className="mt-4" html={b.text} />}
          {b.api && (
            <div className="overflow-x-auto mt-4">
              <table className="w-full text-sm border-collapse">
                <thead>
                  <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                    <th className="py-2 pr-4">Function</th>
                    <th className="py-2 pr-4">Returns</th>
                    <th className="py-2">Description</th>
                  </tr>
                </thead>
                <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
                  {b.api.map(([sig, ret, desc], j) => (
                    <tr key={j} className="align-top">
                      <td className="py-2 pr-4 whitespace-nowrap"><code>{decode(sig)}</code></td>
                      <Html as="td" className="py-2 pr-4 text-slate-500 whitespace-nowrap" html={ret} />
                      <Html as="td" className="py-2" html={desc} />
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {b.example && (
            <DocCodeBlock
              lang={b.lang ?? 'fogu'}
              title={b.title ?? 'example.fg'}
              onRun={(b.lang ?? 'fogu') === 'fogu' ? onRun : undefined}
              code={b.example}
            />
          )}
          {b.output && <DocCodeBlock lang="text" title={b.outputTitle ?? 'output'} code={b.output} />}
          {b.note && <Html as="p" className="mt-4 text-sm text-slate-500" html={b.note} />}
        </React.Fragment>
      ))}
    </section>
  );
};
