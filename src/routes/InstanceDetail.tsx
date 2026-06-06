import { useParams, Link } from "react-router-dom";
import { ArrowLeft } from "lucide-react";

export function InstanceDetail() {
  const { slug } = useParams();

  return (
    <div className="px-8 py-7">
      <Link
        to="/instances"
        className="mb-4 inline-flex items-center gap-1.5 text-sm text-muted hover:text-foreground"
      >
        <ArrowLeft className="size-4" />
        Back to instances
      </Link>
      <h1 className="text-2xl font-semibold">{slug}</h1>
      <p className="text-sm text-muted">
        Instance detail (mods, versions, logs) arrives in Phase 1.
      </p>
    </div>
  );
}
