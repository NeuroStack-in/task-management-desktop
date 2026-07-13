# Cloud-Native Desktop Agent — Reference Architecture

## Enterprise Activity Tracking & Task Management Platform

**Version:** 2.0
**Last Updated:** July 2026
**Status:** Architecture Baseline
**Scope:** Windows · macOS · Linux · AWS Backend

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Product Context & Competitive Landscape](#2-product-context--competitive-landscape)
3. [Architectural Philosophy: Cloud-Native vs. Traditional](#3-architectural-philosophy-cloud-native-vs-traditional)
4. [Core Design Principles](#4-core-design-principles)
5. [Responsibility Boundaries](#5-responsibility-boundaries)
6. [System Architecture](#6-system-architecture)
7. [Desktop Agent Internal Architecture](#7-desktop-agent-internal-architecture)
8. [Communication Architecture](#8-communication-architecture)
9. [Offline-First Data Flow](#9-offline-first-data-flow)
10. [Local Data Management](#10-local-data-management)
11. [Synchronization Strategy](#11-synchronization-strategy)
12. [Remote Administration & Configuration](#12-remote-administration--configuration)
13. [Auto-Update System](#13-auto-update-system)
14. [Fault Tolerance & Failure Scenarios](#14-fault-tolerance--failure-scenarios)
15. [Security Architecture](#15-security-architecture)
16. [Multi-Tenancy](#16-multi-tenancy)
17. [Scalability](#17-scalability)
18. [Observability](#18-observability)
19. [Technology Stack Decisions](#19-technology-stack-decisions)
20. [Language Decision: Rust vs. Go](#20-language-decision-rust-vs-go)
21. [AWS Service Mapping](#21-aws-service-mapping)
22. [CI/CD Pipeline](#22-cicd-pipeline)
23. [Final Recommendation](#23-final-recommendation)
24. [Next Steps](#24-next-steps)

---

## 1. Executive Summary

This document defines a **stable, production-grade architecture** for an enterprise endpoint agent comparable to Time Doctor, ActivTrak, Hubstaff, Insightful, and Teramind.

The central decision is to treat the desktop software **not as a standalone application, but as an edge node in a cloud-native distributed system**. The agent runs continuously on employee machines, captures activity locally, survives network outages, and synchronizes with an AWS backend that owns all business logic, storage, analytics, and administration.

Three decisions anchor the entire design:

- **Cloud-native, not cloud-dependent** — the agent is cloud-managed and cloud-synchronized but fully operational offline.
- **Offline-first with eventual consistency** — a local durable queue (SQLite) guarantees no data loss during outages, crashes, or restarts.
- **Rust for the agent, managed services for the backend** — the always-on component gets Rust's low memory footprint and predictable performance; the backend gets serverless velocity.

---

## 2. Product Context & Competitive Landscape

The platform is an **enterprise endpoint agent**, not a simple desktop utility. Its scope includes:

- Cross-platform operation (Windows, macOS, Linux)
- Always-on background service
- Keyboard and mouse activity metrics
- Application and browser monitoring
- Screenshot capture (policy-controlled)
- Offline-first synchronization
- Remote administration
- Automatic updates
- AWS cloud-native backend
- Long-term deployment to potentially tens of thousands of devices

Because it must run all day, every day, on customer hardware and scale across large fleets, the agent is a **strategic long-lived component** — architecture and language choices compound over the product lifetime.

---

## 3. Architectural Philosophy: Cloud-Native vs. Traditional

"Desktop application" and "cloud-native desktop application" do not describe different executables — they describe **different architectural philosophies**.

### Traditional Desktop Application

Nearly all processing happens locally; cloud connectivity, if present, is secondary.

```text
+-----------------------------+
| Desktop Application         |
+-----------------------------+
| User Interface              |
| Business Logic              |
| Local Database              |
| Local Reports               |
| File Storage                |
+-----------------------------+
```

Examples: VLC, Notepad++, legacy ERP software, Photoshop (offline workflows).

### Cloud-Native Desktop Application

The desktop is an **edge client** in a distributed cloud platform. It performs only tasks requiring direct OS access; the cloud owns everything else.

```text
                 Cloud Services

        API Gateway  /  WebSocket Gateway
                      │
          Authentication Service
          Configuration Service
          Analytics Service
          Reporting Service
          Update Service
          Storage Services
                      ▲
                      │
              HTTPS  /  WebSocket
                      │
             Desktop Agent (Edge Node)
```

### Side-by-Side Comparison

| Feature              | Traditional Desktop | Cloud-Native Desktop   |
| -------------------- | ------------------- | ---------------------- |
| Primary processing   | Local machine       | Cloud + local edge     |
| Business logic       | Local               | Cloud                  |
| Data storage         | Local               | Cloud with local cache |
| Authentication       | Local users         | Centralized identity   |
| Configuration        | Local settings      | Remote management      |
| Updates              | Manual              | Automatic              |
| Monitoring           | Local               | Centralized            |
| Analytics            | Local               | Cloud                  |
| Reporting            | Local               | Cloud                  |
| Multi-device support | Difficult           | Built in               |
| Multi-tenant support | Difficult           | Native                 |
| Scalability          | Limited             | Enterprise scale       |
| Device management    | Manual              | Centralized            |

**A critical clarification:** cloud-native does **not** mean internet-required.

```text
Cloud Native  =  Cloud Managed  +  Offline Capable  +  Eventually Consistent
```

---

## 4. Core Design Principles

The platform must satisfy all of the following:

- **Stateless APIs** — any instance can serve any request
- **Event-driven communication** — components are loosely coupled through an event bus
- **Offline-first agent** — monitoring never stops when connectivity is lost
- **Local persistent queue** — SQLite as a durable buffer, never the system of record
- **Horizontal scalability** — scale out, not up
- **Fault tolerant** — no single module failure terminates the agent
- **Auto-recoverable** — the agent restores its queue after crashes and restarts
- **Multi-region ready** — deployable across AWS regions
- **Multi-tenant** — every API is tenant-aware
- **Observable** — every component exposes metrics, logs, and traces
- **Zero-downtime deployment** — rolling, canary, and percentage rollouts

The universal data-handling pattern applied everywhere:

```text
Collect  →  Persist Locally  →  Synchronize Later
```

---

## 5. Responsibility Boundaries

A clean split of responsibilities is the foundation of the entire design.

### The Desktop Agent IS Responsible For

- Application and browser monitoring
- Keyboard and mouse activity **metrics** (not keystroke logging of content)
- Idle detection
- Screenshot capture (when policy allows)
- Local caching and durable queueing
- Compression and encryption
- Secure batch synchronization
- Health reporting
- Applying automatic updates
- Receiving and executing remote commands

### The Desktop Agent MUST NOT Contain

- Business logic
- Report generation
- Productivity calculations
- Organization / user management
- Billing
- Analytics

### The Cloud IS Responsible For

| Domain            | Responsibilities                                              |
| ----------------- | ------------------------------------------------------------ |
| Authentication    | Device registration, user auth, session management, tokens   |
| Business services | Activity processing, reporting, productivity analytics, RBAC |
| Administration    | Remote configuration, feature flags, update rollout, audit   |
| Data              | System of record, long-term storage, multi-tenancy           |
| Intelligence      | AI insights, scoring, risk detection, recommendations        |

---

## 6. System Architecture

### High-Level AWS Topology

```text
                          Users
                            │
                     Next.js Web App
                            │
                     Amazon CloudFront
                            │
        ┌───────────────────┴────────────────────┐
        │                                         │
  API Gateway / ALB                       WebSocket Gateway
        │                                         │
 ┌──────┴──────────┐                      Command Service
 │                 │                              │
Auth Service   Activity API                       │
 │                 │                              │
 └──────┬──────────┘                              │
        │                                         │
        ▼                                         ▼
  DynamoDB / Aurora            EventBridge  /  SQS  /  Amazon S3
        ▲                                         │
        │                                         ▼
   HTTPS Uploads                           Background Workers
        ▲                                         │
        │                              Reports / Analytics / AI
        └───────────── Desktop Agent ─────────────┘
                     (Rust + Tauri)
```

### Event-Driven Backend Flow

Instead of direct service-to-service calls, the backend is decoupled through an event bus:

```text
Desktop Agent
     ↓
HTTPS + WebSocket
     ↓
Amazon API Gateway
     ↓
Lambda Microservices
     ↓
Amazon EventBridge
     ↓
Worker Services
     ↓
DynamoDB  +  Amazon S3
     ↓
Analytics  →  Reports  →  Next.js Dashboard
```

Loosely coupling services through EventBridge means reporting, analytics, notifications, and AI pipelines can evolve and scale independently.

---

## 7. Desktop Agent Internal Architecture

The agent is **not one monolithic executable**. It is a background service composed of independent modules coordinated by a core.

```text
Agent Core
├── Authentication
├── Config Manager
├── Activity Monitor
├── Browser Monitor
├── Keyboard Monitor
├── Mouse Monitor
├── Screenshot Manager
├── SQLite Queue
├── Upload Manager
├── Health Service
├── Update Service
├── Logging / Tracing
└── IPC Server
```

Each module operates independently so that a failure in one (e.g. the screenshot subsystem) never halts the others (e.g. activity tracking).

### Engine / UI Separation

The monitoring engine must **not** contain the UI. The two are separated into a headless background service plus a small desktop UI:

```text
Background Service  +  Small Desktop UI

Tauri  →  Rust Backend  →  React  →  shadcn/ui
```

Benefits: tiny installer, native performance, modern UI, reusable frontend skills.

---

## 8. Communication Architecture

A **hybrid channel** strategy avoids the scalability trap of polling everything.

### REST / HTTPS — for bulk and batch

- Authentication
- Batched activity uploads
- Screenshot / large file transfers
- Configuration fetch
- Health reporting

### WebSocket — for real-time control

- Remote commands
- Configuration changes
- Forced synchronization
- Agent restart requests
- Immediate update notifications
- Health checks (supplementing periodic heartbeats)

```text
Desktop  ──HTTPS──▶  Batched uploads, auth, screenshots
Desktop  ◀─WebSocket─▶  Commands, config push, sync, restart, updates
```

This reduces unnecessary API traffic, enables near real-time fleet management, and keeps the agent lightweight. For the real-time channel, options include a self-managed WebSocket gateway, **AWS IoT Core**, or **AWS AppSync**.

---

## 9. Offline-First Data Flow

Offline resilience is a **core principle**, not an afterthought. The agent is an edge computing node that continues operating independently and synchronizes when connectivity returns.

### During an Outage

```text
Keyboard / Mouse / App Event
        ↓
   Activity Event
        ↓
     SQLite Queue
        ↓
   Retry Queue (uploads postponed)
```

Nothing is lost — only uploads are deferred.

### Example Timeline

| Time  | State                                                        |
| ----- | ----------------------------------------------------------- |
| 09:00 | Internet disconnects; employee keeps working; agent buffers |
| 09:15 | SQLite holds ~450 activity events + 12 screenshots          |
| 09:45 | Internet returns; worker uploads compressed, encrypted batch|

The backend processes the batch as though the outage never happened. The user never notices.

### When Connectivity Returns

```text
SQLite Queue
     ↓
Batch Upload  (compressed + encrypted)
     ↓
API Gateway  →  Lambda  →  DynamoDB
     ↓
Acknowledged
     ↓
Delete Local Copy
```

This is **eventual consistency**.

---

## 10. Local Data Management

SQLite is **not** the primary database. It is a **durable queue and cache**.

```text
Cloud                Desktop SQLite
  ↓                        ↓
Source of Truth      Temporary Cache
```

SQLite temporarily stores:

- Pending events
- Pending screenshots
- Upload status
- Retry counters
- Device configuration
- Health information

### Source of Truth

| State             | Location                          | Role                              |
| ----------------- | --------------------------------- | --------------------------------- |
| Operational state | SQLite on desktop                 | Pending work, enables offline ops |
| Authoritative state | Cloud (DynamoDB + S3)           | Permanent system of record        |

---

## 11. Synchronization Strategy

The agent follows an offline-first, acknowledgement-driven sync sequence for **every** data type:

```text
Collect Event
     ↓
Validate
     ↓
Store in SQLite
     ↓
Compress  (zstd)
     ↓
Encrypt   (AES-GCM)
     ↓
Batch Upload
     ↓
Receive Acknowledgement
     ↓
Delete Local Copy
```

Data is never discarded until the cloud confirms successful receipt. Background workers handle retries with **exponential backoff**.

Advantages:

- No data loss during network outages
- Lower API request volume
- Reduced AWS costs
- Improved reliability

---

## 12. Remote Administration & Configuration

Administrators manage the fleet centrally. Supported operations:

- Change screenshot interval and quality
- Enable / disable features
- Force synchronization
- Restart agent
- Request diagnostic logs
- Roll out and roll back updates
- View agent health

### Online Configuration Push

```text
Admin Dashboard  →  Configuration API  →  Desktop (via WebSocket)  →  Applied instantly
```

### Offline Configuration (Deferred)

```text
Admin  →  Cloud  →  Pending Configuration
                          ↓  (agent reconnects)
                 Desktop fetches latest config  →  Applied automatically
```

### Remote Command Set

```text
Start Screenshot · Stop Screenshot · Restart Agent
Refresh Config · Force Sync · Collect Logs · Upgrade · Rollback
```

Commands arrive via WebSocket when online, or are queued and delivered on reconnect.

---

## 13. Auto-Update System

```text
Admin Uploads Version
        ↓
     Amazon S3
        ↓
     Manifest
        ↓
Agent Startup / Notification
        ↓
Check Update Service
        ↓
New Version Available?
        ↓
     Download
        ↓
  Verify Signature
        ↓
     Install
        ↓
     Restart
```

The update service supports **canary deployments, percentage rollouts, scheduled updates, forced updates, and rollback**. If the agent is offline when a version ships, the update simply proceeds on reconnect.

Recommended libraries: `tauri-plugin-updater` or `self_update`.

---

## 14. Fault Tolerance & Failure Scenarios

Each subsystem operates independently behind the queue:

```text
Keyboard Monitor
        ↓
   Event Queue
        ↓
     SQLite
        ↓
    Uploader
        ↓
      AWS
```

**Design rule:** no single module should terminate the entire agent.

| Scenario           | Behavior                                                              |
| ------------------ | -------------------------------------------------------------------- |
| Uploader fails     | Monitoring continues; data stays in SQLite; automatic retry          |
| Screenshots fail   | Activity tracking continues unaffected                               |
| PC restarts        | On startup, agent reads SQLite, finds pending events, resumes upload |
| AWS unavailable    | Agent retries with backoff; monitoring continues                     |
| Agent crashes      | After restart, queue is recovered from SQLite and upload continues   |

Only in-memory events never committed to SQLite are at risk — minimized by frequent commits and intelligent batching.

---

## 15. Security Architecture

### Desktop Agent

- JWT access tokens + refresh tokens
- Device registration
- TLS 1.3 with certificate validation
- Encrypted sensitive configuration and secure token storage
- AES-GCM encryption of buffered data
- Digitally signed update packages with manifest verification and rollback protection

### Cloud

- Amazon Cognito (identity)
- AWS IAM (access control)
- AWS Secrets Manager (secrets)
- AWS KMS (encryption keys)
- AWS CloudTrail (audit)
- AWS WAF (edge protection)

```text
Cognito  →  JWT  →  Desktop  →  API Gateway
```

Identity is centralized; every request is authenticated and tenant-scoped.

---

## 16. Multi-Tenancy

Every API is tenant-aware, following a strict hierarchy:

```text
Organization
     ↓
 Workspace
     ↓
 Projects
     ↓
  Teams
     ↓
  Users
     ↓
Desktop Agents
```

This enables native multi-tenant isolation, per-organization configuration, and clean RBAC boundaries.

---

## 17. Scalability

The same architecture scales without redesigning the desktop agent.

Target capacity:

- 100,000+ devices
- Millions of activity records per day
- Thousands of screenshots per minute
- Multiple organizations
- Multi-region deployments

Achieved through:

- Stateless APIs
- Batch uploads (fewer, larger requests)
- Event-driven services
- Horizontal scaling
- Distributed storage

```text
100,000 Devices  →  Cloud APIs  →  Distributed Storage
```

---

## 18. Observability

Every component exposes metrics, logs, and traces.

### Metrics

- CPU and memory usage
- Queue size
- Upload latency
- Synchronization success rate
- Agent health and version distribution

### Logs

- Structured logs
- Error and crash logs
- Audit logs

### Tracing

- Distributed tracing across cloud services

| Layer   | Tooling                                      |
| ------- | -------------------------------------------- |
| Cloud   | Amazon CloudWatch · AWS X-Ray · OpenTelemetry |
| Desktop | `tracing` · OpenTelemetry · structured logs   |

---

## 19. Technology Stack Decisions

### Desktop Agent (Rust)

| Area               | Recommended                                            |
| ------------------ | ------------------------------------------------------ |
| Language           | Rust                                                   |
| UI Shell           | Tauri                                                  |
| UI Framework       | React + TypeScript + shadcn/ui                         |
| Async Runtime      | Tokio                                                  |
| HTTP Client        | reqwest                                                |
| REST (if agent-side) | axum                                                 |
| Serialization      | serde                                                  |
| SQLite             | rusqlite (or sqlx)                                     |
| Logging            | tracing                                                |
| Metrics            | OpenTelemetry                                          |
| TLS                | rustls                                                 |
| Encryption         | aes-gcm                                                |
| Compression        | zstd                                                   |
| Scheduler          | tokio-cron-scheduler (if local scheduling needed)      |
| Config             | config + serde                                         |
| Screenshots        | xcap (cross-platform)                                  |
| Keyboard / Mouse   | rdev (supplemented with native APIs where required)    |
| File Watching      | notify                                                 |
| Process / System   | sysinfo                                                |
| Auto Updates       | tauri-plugin-updater / self_update                     |

### Web Application

Next.js (App Router) · React · TypeScript · Tailwind CSS · shadcn/ui · TanStack Query · React Hook Form · Zod

Authentication: AWS Cognito or Auth.js.

### Backend Options (both cloud-native)

**Serverless:** API Gateway → Lambda → DynamoDB → S3
**Containers:** Amazon ECS → Rust services (axum / actix-web) → PostgreSQL → Redis

API framework by choice: `axum` or `actix-web` (Rust), `FastAPI` (Python), or Next.js Route Handlers.

---

## 20. Language Decision: Rust vs. Go

The agent runs continuously on customer machines, so the decision is made against **actual requirements**, not general language preference.

| Requirement            | Go        | Rust                              | Winner |
| ---------------------- | --------- | --------------------------------- | ------ |
| Cross-platform agent   | ✅         | ✅                                 | Tie    |
| Background service     | ✅         | ✅                                 | Tie    |
| Very low RAM usage     | Good      | Excellent                         | Rust   |
| Very low CPU usage     | Good      | Excellent                         | Rust   |
| Long-running process   | Good      | Excellent                         | Rust   |
| Native OS integration  | Good      | Excellent                         | Rust   |
| Keyboard / mouse hooks | Good      | Excellent                         | Rust   |
| Screenshot processing  | Good      | Excellent                         | Rust   |
| Offline queue          | ✅         | ✅                                 | Tie    |
| SQLite                 | ✅         | ✅                                 | Tie    |
| AWS communication      | Excellent | Excellent                         | Tie    |
| WebSocket support      | Excellent | Excellent                         | Tie    |
| Auto updater           | Good      | Excellent (with Tauri)            | Rust   |
| Fault tolerance        | Excellent | Excellent                         | Tie    |
| Developer productivity | Excellent | Moderate                          | Go     |
| Runtime predictability | Good      | Excellent                         | Rust   |
| Compile-time safety    | Good      | Excellent                         | Rust   |

### Resource Comparison (after 8 hours)

| Metric | Go Agent   | Rust Agent |
| ------ | ---------- | ---------- |
| RAM    | 30–60 MB   | 8–20 MB    |
| CPU    | 0.8–2%     | 0.2–0.8%   |

Across a 5,000-seat fleet, this difference is significant — lower CPU means better battery life, less user impact, and less chance the agent is perceived as "heavy."

### Development Speed (where Go wins)

| Language | MVP Timeline |
| -------- | ------------ |
| Go       | 8–10 weeks   |
| Rust     | 12–16 weeks  |

### Verdict

The agent is **event-driven, not CPU-intensive**, but it runs all day on customer hardware and is a strategic long-lived component (with a roadmap toward AI analysis, screen OCR, browser extensions, policy engine, and device management). **Rust's upfront cost pays off over the product lifetime.**

The pragmatic split — use each language where it fits:

```text
Next.js Dashboard
     ↓
API Gateway
     ↓
Python Lambda / Go Services
     ↓
AWS
     ↓
Rust Desktop Agent
```

---

## 21. AWS Service Mapping

| Purpose                    | Service                                           |
| -------------------------- | ------------------------------------------------- |
| Authentication             | Amazon Cognito                                    |
| API                        | Amazon API Gateway                                |
| Compute                    | AWS Lambda                                        |
| Object Storage             | Amazon S3                                         |
| NoSQL                      | Amazon DynamoDB                                   |
| Relational (optional)      | Amazon RDS PostgreSQL / Amazon Aurora PostgreSQL  |
| Caching                    | Amazon ElastiCache (Redis)                        |
| Messaging / Event Bus      | Amazon EventBridge                                |
| Queues                     | Amazon SQS                                         |
| Notifications              | Amazon SNS                                         |
| Monitoring                 | Amazon CloudWatch                                 |
| Distributed Tracing        | AWS X-Ray / OpenTelemetry                         |
| Secrets                    | AWS Secrets Manager                               |
| Encryption Keys            | AWS KMS                                           |
| CDN                        | Amazon CloudFront                                 |
| DNS                        | Amazon Route 53                                   |
| Real-time (optional)       | AWS IoT Core / AWS AppSync                        |

---

## 22. CI/CD Pipeline

```text
GitHub
   ↓
GitHub Actions
   ↓
Tests
   ↓
Cargo Build (agent)  /  Build (backend)
   ↓
Sign Packages
   ↓
Amazon S3
   ↓
Deploy  →  Lambda  →  CloudFront
```

Signed packages flow to S3 / CloudFront for automatic, staged deployment — supporting canary and percentage rollouts on the desktop side.

---

## 23. Final Recommendation

Build the platform as a **cloud-native, offline-first desktop agent**, not a traditional desktop application.

### Desktop Agent

- **Language:** Rust
- **UI:** Tauri + React + TypeScript + shadcn/ui
- **Async Runtime:** Tokio
- **Database:** SQLite (rusqlite / sqlx) — as a durable queue, not the source of truth
- **HTTP:** reqwest · **Serialization:** serde · **Logging:** tracing
- **Compression:** zstd · **TLS:** rustls · **Encryption:** aes-gcm
- **Screenshots:** xcap · **System:** sysinfo · **Config:** config + serde
- **Metrics:** OpenTelemetry

### Backend

- Next.js (frontend) on CloudFront
- API Gateway + Lambda (Python or Go)
- DynamoDB + S3 (system of record)
- Cognito · EventBridge · SQS · SNS · CloudWatch · X-Ray · Secrets Manager · KMS

### Why This Combination

The desktop agent runs continuously on customer machines and benefits most from Rust's low memory usage, predictable performance, and compile-time guarantees. The backend benefits more from rapid development, managed services, and serverless scaling than from systems-level optimization. This split balances **operational efficiency** with **development velocity**, and scales cleanly from a single device to enterprise fleets across Windows, macOS, and Linux.

The four pillars that make the architecture stable:

1. **Clean responsibility separation** — agent captures, cloud decides.
2. **Offline-first durability** — nothing is lost until the cloud acknowledges receipt.
3. **Hybrid communication** — REST for bulk, WebSocket for real-time control.
4. **Event-driven, loosely coupled backend** — every service scales independently.

---

## 24. Next Steps

This document is the architecture baseline. The next logical artifact is a **Cloud-Native Software Architecture Document (SAD)** that goes deeper into:

- Domain-Driven Design (DDD) and bounded contexts
- Microservice boundaries and ownership
- Event flows and event schemas
- API contracts (OpenAPI / AsyncAPI)
- Deployment topology and infrastructure diagrams (AWS CDK / Terraform)
- Sequence diagrams for sync, command, and update flows
- Data models for DynamoDB (single-table design) and S3 layout
- SLOs, error budgets, and alerting policy

---

*End of document.*
