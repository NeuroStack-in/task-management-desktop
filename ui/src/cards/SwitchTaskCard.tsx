import { useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import type { Project, Task } from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * Switch-task strip — a faithful port of TaskFlow's TaskSelector, recoloured to the present theme
 * and with the **Meeting button removed** (per request). Description + a searchable project picker +
 * refresh + Start. Uses the panel's teal tokens throughout (bg-primary, border-input, bg-popover…).
 *
 * The reference also had a task dropdown; the agent tracks against a project + free-text description
 * (no task list yet), so the description is the "what" and the picker chooses the project.
 */
export function SwitchTaskCard({
  projects,
  tasks,
  running,
  onStart,
  onRefresh,
  loading,
}: {
  projects: Project[];
  tasks: Task[];
  running: boolean;
  onStart: (projectId: string, description: string, taskId?: string) => void;
  onRefresh?: () => void;
  loading?: boolean;
}) {
  const [projectId, setProjectId] = useState("");
  const [taskId, setTaskId] = useState("");
  const [description, setDescription] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const descRef = useRef<HTMLInputElement>(null);

  const selected = projects.find((p) => p.id === projectId) ?? null;
  const projectTasks = tasks.filter((t) => t.project_id === projectId);
  const canStart = description.trim().length > 0 && !!selected;

  function pickTask(id: string) {
    setTaskId(id);
    // Autofill the description with the task title if empty — TaskFlow's behaviour.
    const t = tasks.find((x) => x.id === id);
    if (t && !description.trim()) setDescription(t.title);
  }

  function start() {
    if (!canStart) return;
    onStart(selected!.id, description.trim(), taskId || undefined);
    setDescription("");
    setProjectId("");
    setTaskId("");
  }

  function refresh() {
    setRefreshing(true);
    onRefresh?.();
    window.setTimeout(() => setRefreshing(false), 500);
  }

  return (
    <div className="w-full min-w-0 space-y-2">
      {/* Description input with leading pencil glyph. */}
      <div className="relative">
        <span
          className="pointer-events-none absolute left-2.5 top-[18px] -translate-y-1/2 text-muted-foreground/60"
          aria-hidden
        >
          <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 20h9" />
            <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4Z" />
          </svg>
        </span>
        <input
          ref={descRef}
          type="text"
          placeholder="What are you working on?"
          value={description}
          maxLength={500}
          onChange={(e) => setDescription(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && canStart) start();
          }}
          className="h-9 w-full rounded-md border border-input bg-background pl-8 pr-3 text-sm outline-none placeholder:text-muted-foreground/70 focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40"
        />
      </div>

      <div className="flex min-w-0 gap-1.5">
        <Dropdown
          value={projectId}
          placeholder="Select Project"
          icon={<ProjectIcon />}
          searchPlaceholder="Search projects…"
          options={projects.map((p) => ({ value: p.id, label: p.name, swatch: colorFor(p.name) }))}
          onChange={(v) => {
            setProjectId(v);
            setTaskId("");
          }}
        />

        {selected && projectTasks.length > 0 && (
          <Dropdown
            value={taskId}
            placeholder="Select Task"
            icon={<TaskIcon />}
            searchPlaceholder="Search tasks…"
            options={projectTasks.map((t) => ({ value: t.id, label: t.title }))}
            onChange={pickTask}
          />
        )}

        <button
          type="button"
          onClick={refresh}
          disabled={refreshing}
          title={refreshing ? "Refreshing…" : "Refresh projects"}
          aria-label="Refresh projects"
          className={cn(
            "flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-md border border-input bg-background text-muted-foreground shadow-sm transition-all duration-150",
            "hover:border-ring/40 hover:bg-accent/30 hover:text-foreground",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring active:scale-[.96]",
            "disabled:cursor-not-allowed disabled:opacity-60",
          )}
        >
          <RefreshIcon spinning={refreshing} />
        </button>
      </div>

      <Button
        type="button"
        className="h-9 w-full gap-1.5 font-semibold shadow-sm hover:shadow"
        disabled={loading || !canStart}
        onClick={start}
      >
        {loading ? (
          <span className="opacity-80">Starting…</span>
        ) : (
          <>
            <PlayIcon />
            {running ? "Switch task" : "Start"}
          </>
        )}
      </Button>
    </div>
  );
}

/* ═══ Custom Dropdown (opens upward, searchable) — TaskFlow's, in React ═══ */

interface Opt {
  value: string;
  label: string;
  swatch?: string;
}

function Dropdown({
  value,
  placeholder,
  icon,
  options,
  onChange,
  searchPlaceholder = "Search…",
}: {
  value: string;
  placeholder: string;
  icon?: React.ReactNode;
  options: Opt[];
  onChange: (v: string) => void;
  searchPlaceholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const ref = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const selected = options.find((o) => o.value === value);

  useEffect(() => {
    if (!open) return;
    function handler(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  useEffect(() => {
    if (open) window.setTimeout(() => searchRef.current?.focus(), 0);
    else setQuery("");
  }, [open]);

  const q = query.trim().toLowerCase();
  const filtered = q ? options.filter((o) => o.label.toLowerCase().includes(q)) : options;
  const showSearch = options.length > 8 || q.length > 0;

  return (
    <div className="relative min-w-0 flex-1" ref={ref}>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        aria-haspopup="listbox"
        className={cn(
          "flex h-9 w-full items-center gap-2 rounded-md border px-3 text-left text-xs shadow-sm transition-all",
          "bg-background hover:border-ring/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          open ? "border-primary" : "border-input",
          selected ? "font-medium text-foreground" : "text-muted-foreground",
        )}
      >
        {icon && <span className="flex-shrink-0 text-muted-foreground">{icon}</span>}
        <span className="min-w-0 flex-1 truncate" title={selected?.label}>
          {selected ? selected.label : placeholder}
        </span>
        <ChevronIcon open={open} />
      </button>

      {open && (
        <div
          role="listbox"
          className="absolute bottom-full z-50 mb-1 flex max-h-56 w-full flex-col overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-lg"
        >
          {showSearch && (
            <div className="flex flex-shrink-0 items-center gap-1.5 border-b border-border/60 bg-muted/40 px-2 py-1.5">
              <svg className="h-3 w-3 flex-shrink-0 text-muted-foreground/80" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                <circle cx="11" cy="11" r="7" />
                <path d="M21 21l-4.3-4.3" />
              </svg>
              <input
                ref={searchRef}
                type="text"
                value={query}
                onChange={(e) => setQuery(e.currentTarget.value)}
                placeholder={searchPlaceholder}
                className="w-full bg-transparent text-[11.5px] text-foreground placeholder:text-muted-foreground/70 focus:outline-none"
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.preventDefault();
                    setOpen(false);
                  }
                }}
              />
            </div>
          )}
          <div className="flex-1 overflow-y-auto">
            {filtered.length === 0 ? (
              <div className="px-3 py-3 text-center text-[11.5px] text-muted-foreground">
                {q ? `No matches for "${query}"` : "No options"}
              </div>
            ) : (
              filtered.map((opt) => (
                <button
                  key={opt.value}
                  role="option"
                  type="button"
                  title={opt.label}
                  aria-selected={opt.value === value}
                  onClick={() => {
                    onChange(opt.value);
                    setOpen(false);
                  }}
                  className={cn(
                    "w-full px-3 py-2 text-left text-xs transition-colors focus-visible:bg-accent focus-visible:text-accent-foreground focus-visible:outline-none",
                    opt.value === value
                      ? "bg-primary/10 font-medium text-primary"
                      : "text-foreground hover:bg-accent hover:text-accent-foreground",
                  )}
                >
                  <div className="flex min-w-0 items-center gap-2">
                    {opt.value === value ? (
                      <svg className="h-3 w-3 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20" aria-hidden>
                        <path fillRule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clipRule="evenodd" />
                      </svg>
                    ) : opt.swatch ? (
                      <span className="h-2.5 w-2.5 flex-shrink-0 rounded-full ring-1 ring-foreground/10" style={{ background: opt.swatch }} aria-hidden />
                    ) : (
                      <span className="w-3 flex-shrink-0" aria-hidden />
                    )}
                    <span className="flex-1 truncate">{opt.label}</span>
                  </div>
                </button>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function colorFor(name: string): string {
  let h = 2166136261;
  for (let i = 0; i < name.length; i++) {
    h ^= name.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return `hsl(${h % 360} 62% 52%)`;
}

/* ═══ Icons ═══ */

function ChevronIcon({ open }: { open: boolean }) {
  return (
    <svg className={cn("h-3 w-3 flex-shrink-0 text-muted-foreground transition-transform", open && "rotate-180")} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2.5" aria-hidden>
      <path d="M19 9l-7 7-7-7" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function ProjectIcon() {
  return (
    <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="1.8" aria-hidden>
      <path strokeLinecap="round" strokeLinejoin="round" d="M9 17V7m0 10a2 2 0 01-2 2H5a2 2 0 01-2-2V7a2 2 0 012-2h2a2 2 0 012 2m0 10a2 2 0 002 2h2a2 2 0 002-2M9 7a2 2 0 012-2h2a2 2 0 012 2m0 10V7" />
    </svg>
  );
}

function TaskIcon() {
  return (
    <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="1.8" aria-hidden>
      <path strokeLinecap="round" strokeLinejoin="round" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4" />
    </svg>
  );
}

function PlayIcon() {
  return (
    <svg className="h-3 w-3 fill-current" viewBox="0 0 24 24" aria-hidden>
      <path d="M8 5v14l11-7z" />
    </svg>
  );
}

function RefreshIcon({ spinning }: { spinning: boolean }) {
  return (
    <svg className={cn("h-3.5 w-3.5 transition-transform", spinning && "animate-spin")} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" aria-hidden>
      <path strokeLinecap="round" strokeLinejoin="round" d="M4 4v5h5M20 20v-5h-5M4.5 15a8 8 0 0014.5 2M19.5 9A8 8 0 005 7" />
    </svg>
  );
}
