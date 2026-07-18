import type { Project, Task } from "./types";

// Placeholder catalog for the project→task selector until the backend serves `GET /v1/agent/tasks`
// (BUILD-PLAN M3a — an ETag pull mirroring config_version). Replace this file with that fetch; the
// selector already keys tasks by `projectId`, so no shape change is needed.
export const PROJECTS: Project[] = [
  { id: "p-core", name: "Core Platform" },
  { id: "p-mobile", name: "Mobile App" },
  { id: "p-insights", name: "Insights" },
];

export const TASKS: Task[] = [
  { id: "t-core-api", projectId: "p-core", name: "API work" },
  { id: "t-core-triage", projectId: "p-core", name: "Bug triage" },
  { id: "t-mobile-onboarding", projectId: "p-mobile", name: "Onboarding screen" },
  { id: "t-insights-charts", projectId: "p-insights", name: "Dashboard charts" },
];
