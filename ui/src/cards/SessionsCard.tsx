import { formatWorked } from "@/lib/format";
import type { Project, Session, TimerState } from "@/lib/types";
import { cn } from "@/lib/utils";

/** Deterministic per-project dot colour (until the backend ships a real colour), TaskFlow-style. */
function colorForProject(name: string): string {
  let h = 2166136261;
  for (let i = 0; i < name.length; i++) {
    h ^= name.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return `hsl(${h % 360} 62% 52%)`;
}

interface Row {
  key: string;
  title: string;
  projectName: string;
  projectId: string;
  description: string;
  secs: number;
  running: boolean;
}

/**
 * Today's Sessions — a faithful port of TaskFlow's SessionBlock + TaskRow, recoloured to the present
 * theme (emerald → `--success`). Clicking a stopped row resumes that project + description; the
 * running row is shown active. Fed by the agent's aggregated sessions.
 */
export function SessionsCard({
  sessions,
  projects,
  timer,
  onResume,
  goalHours = 4,
  loading,
}: {
  sessions: Session[];
  projects: Project[];
  timer: TimerState;
  onResume: (projectId: string, description: string) => void;
  goalHours?: number;
  loading?: boolean;
}) {
  if (sessions.length === 0) return null;
  const nameOf = (id: string) => projects.find((p) => p.id === id)?.name ?? "Project";
  const runningDesc = timer.description.trim();

  const rows: Row[] = sessions.map((s) => ({
    key: `${s.project_id}:${s.description}`,
    title: s.description || "General",
    projectName: nameOf(s.project_id),
    projectId: s.project_id,
    description: s.description,
    secs: s.secs,
    running: timer.running && timer.project_id === s.project_id && runningDesc === s.description,
  }));

  const total = rows.reduce((sum, r) => sum + r.secs, 0);
  const goalReached = total >= goalHours * 3600;

  return (
    <div className="mx-3 mt-3 overflow-hidden rounded-xl border border-border bg-card">
      <div className="flex items-center justify-between border-b border-border bg-muted/40 px-3 py-2">
        <span className="text-[9.5px] font-semibold uppercase tracking-[0.10em] text-muted-foreground/85">
          Today's Sessions
        </span>
        <span className="tabular-nums text-[10px] font-medium text-muted-foreground/80">
          {rows.length} task{rows.length !== 1 ? "s" : ""}
        </span>
      </div>

      <div>
        {rows.map((r) => (
          <TaskRow key={r.key} row={r} loading={loading} onResume={() => onResume(r.projectId, r.description)} />
        ))}
      </div>

      <div className="flex items-center justify-between border-t border-border bg-muted/40 px-3 py-2">
        <span className="inline-flex items-center gap-1.5 text-[9.5px] font-semibold uppercase tracking-[0.10em] text-muted-foreground/85">
          Total
          {goalReached && (
            <span className="inline-flex items-center gap-1 rounded-full bg-success/15 px-1.5 py-0.5 text-[8.5px] font-bold tracking-[0.06em] text-success">
              <svg className="h-2.5 w-2.5" fill="currentColor" viewBox="0 0 20 20" aria-hidden>
                <path
                  fillRule="evenodd"
                  d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z"
                  clipRule="evenodd"
                />
              </svg>
              GOAL
            </span>
          )}
        </span>
        <span
          className={cn(
            "tabular-nums font-mono text-[13px] font-bold tracking-tight",
            goalReached ? "text-success" : "text-foreground",
          )}
        >
          {formatWorked(total)}
        </span>
      </div>
    </div>
  );
}

function TaskRow({ row, onResume, loading }: { row: Row; onResume: () => void; loading?: boolean }) {
  return (
    <button
      type="button"
      onClick={() => {
        if (!row.running) onResume();
      }}
      disabled={loading || row.running}
      title={row.running ? `${row.title} — currently active` : `Resume ${row.title}`}
      className={cn(
        "group relative flex w-full items-center gap-2.5 border-b border-border/60 px-3 py-2.5 text-left transition-all duration-150 last:border-0",
        "hover:bg-accent/40 focus-visible:bg-accent/60 focus-visible:outline-none",
        "disabled:cursor-default",
        loading && !row.running && "opacity-50",
      )}
    >
      <div
        className={cn(
          "flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full ring-1 ring-transparent transition-all duration-150",
          row.running
            ? "bg-success/15 text-success ring-success/25"
            : "bg-primary/10 text-primary group-hover:bg-primary/15 group-hover:ring-primary/20",
        )}
      >
        {row.running ? (
          <span className="relative flex h-2 w-2" aria-hidden>
            <span className="absolute h-full w-full animate-ping rounded-full bg-success opacity-70" />
            <span className="relative h-2 w-2 rounded-full bg-success" />
          </span>
        ) : (
          <svg className="ml-0.5 h-3 w-3 transition-transform duration-150 group-hover:scale-110" fill="currentColor" viewBox="0 0 24 24" aria-hidden>
            <path d="M8 5v14l11-7z" />
          </svg>
        )}
      </div>

      <div className="min-w-0 flex-1">
        <p className="truncate text-[12px] font-semibold leading-tight tracking-[-0.005em] text-foreground" title={row.title}>
          {row.title}
        </p>
        <p className="mt-0.5 flex items-center gap-1 truncate text-[10.5px] leading-tight text-muted-foreground">
          {row.projectName && (
            <span
              className="h-1.5 w-1.5 flex-shrink-0 rounded-full"
              style={{ background: colorForProject(row.projectName) }}
              aria-hidden
            />
          )}
          <span className="truncate">{row.projectName}</span>
        </p>
      </div>

      <span
        className={cn(
          "tabular-nums ml-2 flex-shrink-0 font-mono text-[12.5px] font-bold leading-none tracking-tight",
          row.running ? "text-success" : "text-foreground/85",
        )}
      >
        {formatWorked(row.secs)}
      </span>
    </button>
  );
}
