import { useOutletContext } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import { Loader2, AlertCircle } from "lucide-react";
import type { InstanceTabContext } from "@/routes/InstanceDetail";
import { PackSourcePanel } from "@/routes/InstanceDetail";
import { getPackInfo } from "@/lib/ipc";

export function InfoTab() {
  const { slug, instance, invalidate } = useOutletContext<InstanceTabContext>();

  // Provider routing: wire value "modrinth" | "curseForge" → routing string "modrinth" | "curseforge".
  const providerRoute: "modrinth" | "curseforge" | null = instance.source
    ? instance.source.provider === "modrinth"
      ? "modrinth"
      : instance.source.provider === "curseForge"
        ? "curseforge"
        : null
    : null;

  const infoQuery = useQuery({
    queryKey: ["pack-info", providerRoute, instance.source?.projectId],
    queryFn: () => getPackInfo(providerRoute!, instance.source!.projectId),
    enabled: !!instance.source && providerRoute !== null,
    staleTime: 5 * 60_000,
  });

  if (!instance.source) {
    return (
      <div className="rounded-lg border border-dashed border-border bg-surface/40 px-4 py-8 text-center text-sm text-muted">
        This instance wasn't installed from a provider.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      {/* Pack info header + description */}
      {infoQuery.isLoading && (
        <div className="flex items-center gap-2 py-4 text-sm text-muted">
          <Loader2 className="size-4 animate-spin" />
          Loading description…
        </div>
      )}

      {infoQuery.isError && (
        <div className="flex items-center gap-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-sm text-danger">
          <AlertCircle className="size-4 shrink-0" />
          <span>Couldn't load description: {String(infoQuery.error)}</span>
        </div>
      )}

      {infoQuery.data && (
        <div className="flex flex-col gap-4">
          {/* Title row: icon + name */}
          <div className="flex items-center gap-3">
            {infoQuery.data.iconUrl && (
              <img
                src={infoQuery.data.iconUrl}
                alt=""
                className="size-14 shrink-0 rounded-xl object-cover"
                loading="lazy"
              />
            )}
            <h2 className="text-xl font-semibold">{infoQuery.data.title}</h2>
          </div>

          {/* Description: single render path handles both Modrinth Markdown and CF HTML */}
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
              {infoQuery.data.description}
            </ReactMarkdown>
          </div>
        </div>
      )}

      {/* Pack source panel — update/lock stays reachable */}
      <PackSourcePanel
        slug={slug}
        source={instance.source}
        packLocked={instance.packLocked ?? false}
        provider={instance.source.provider}
        projectId={instance.source.projectId}
        onMutate={invalidate}
      />
    </div>
  );
}
