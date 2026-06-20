/**
 * Shared markdown/HTML renderer for pack descriptions.
 * Handles both Modrinth Markdown and CurseForge HTML (via rehypeRaw + rehypeSanitize).
 */

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";

interface PackDescriptionProps {
  markdown: string;
}

export function PackDescription({ markdown }: PackDescriptionProps) {
  return (
    <div className="prose-pack text-sm leading-relaxed text-muted">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeRaw, rehypeSanitize]}
        components={{
          h1: ({ children }) => (
            <h1 className="mb-2 mt-4 text-lg font-semibold text-foreground">{children}</h1>
          ),
          h2: ({ children }) => (
            <h2 className="mb-2 mt-3 text-base font-semibold text-foreground">{children}</h2>
          ),
          h3: ({ children }) => (
            <h3 className="mb-1 mt-2 text-sm font-semibold text-foreground">{children}</h3>
          ),
          p: ({ children }) => <p className="mb-2">{children}</p>,
          ul: ({ children }) => (
            <ul className="mb-2 list-disc pl-5">{children}</ul>
          ),
          ol: ({ children }) => (
            <ol className="mb-2 list-decimal pl-5">{children}</ol>
          ),
          li: ({ children }) => <li className="mb-0.5">{children}</li>,
          a: ({ href, children }) => (
            <a
              href={href}
              target="_blank"
              rel="noopener noreferrer"
              className="text-accent hover:underline"
            >
              {children}
            </a>
          ),
          code: ({ children }) => (
            <code className="rounded bg-surface-2 px-1 py-0.5 font-mono text-xs">
              {children}
            </code>
          ),
          pre: ({ children }) => (
            <pre className="mb-2 overflow-x-auto rounded-lg border border-border bg-surface p-3 font-mono text-xs">
              {children}
            </pre>
          ),
          img: ({ src, alt }) => (
            <img
              src={src}
              alt={alt}
              referrerPolicy="no-referrer"
              className="my-2 max-w-full rounded-lg"
              loading="lazy"
            />
          ),
          blockquote: ({ children }) => (
            <blockquote className="mb-2 border-l-4 border-border pl-4 text-muted">
              {children}
            </blockquote>
          ),
        }}
      >
        {markdown}
      </ReactMarkdown>
    </div>
  );
}
