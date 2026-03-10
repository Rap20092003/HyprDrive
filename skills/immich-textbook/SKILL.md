---
name: immich-textbook
description: Comprehensive reference guide extracted from Immich's codebase. Use as a textbook for understanding photo/video management patterns applicable to HyprDrive.
---

# Immich Textbook

> **Source**: [github.com/immich-app/immich](https://github.com/immich-app/immich)
> **Purpose**: Reference guide for HyprDrive's media management, ML search, and photo/video features.

> [!IMPORTANT]
> This skill is a **textbook**, not a dependency. Immich is TypeScript + PostgreSQL; HyprDrive is Rust + SQLite.
> Study patterns and translate them to HyprDrive's architecture. DO NOT copy-paste code.

---

## Chapter 1: Architecture Overview

### 4-Service Docker Composition

```
┌────────────────────────────────────────────────────────┐
│  immich-server (Node.js/NestJS, port 2283)             │
│  ├── REST API + WebSocket                              │
│  ├── Job queue (BullMQ on Redis)                       │
│  ├── Controllers → Services → Repositories → Schema    │
│  └── Handles: uploads, albums, sharing, sync, auth     │
├────────────────────────────────────────────────────────┤
│  immich-machine-learning (Python, port 3003)           │
│  ├── CLIP embeddings (OpenAI ViT-B/32)                 │
│  ├── Facial recognition (InsightFace antelopev2)       │
│  ├── ONNX Runtime inference                            │
│  └── Hardware: CPU / CUDA / ROCm / OpenVINO / RKNN     │
├────────────────────────────────────────────────────────┤
│  redis/valkey (Valkey 9, port 6379)                    │
│  └── Job queue broker + cache                          │
├────────────────────────────────────────────────────────┤
│  database (PostgreSQL 14 + VectorChord + pgvectors)    │
│  └── Vector similarity search for CLIP embeddings      │
└────────────────────────────────────────────────────────┘
```

### HyprDrive Adaptation

| Immich Component | HyprDrive Equivalent |
|---|---|
| NestJS server | Axum HTTP + rspc WebSocket (Rust) |
| BullMQ job queue | `task-system` crate (Rust native) |
| Python ML service | WASM extension OR `ort` crate (ONNX in Rust) |
| Redis | Not needed — SQLite + redb for cache |
| PostgreSQL+pgvector | SQLite + FTS5 (full-text) + in-memory vector index |

### Key Lesson: Separate ML from Server

```
Immich runs ML as a SEPARATE microservice because:
1. ML models are large (300MB+ for CLIP) — keeps server lightweight
2. Hardware acceleration (GPU) is isolated
3. ML can scale independently
4. Crashes in ML don't take down the server

HyprDrive adaptation:
  Option A: Same pattern — spawn ML worker as separate process
  Option B: Load ONNX models in WASM extension (Phase 20)
  Option C: Use `ort` crate directly in daemon (if GPU not needed)
  
  Recommendation: Option A for GPU, Option C for CPU-only deployments.
```

---

## Chapter 2: Server Architecture

### Layered Architecture (NestJS Pattern)

```
REQUEST FLOW:

  HTTP/WS Client
    → Controller (route + auth + DTO validation)
      → Service (business logic)
        → Repository (database queries)
          → Schema/Table (Kysely definitions)

DIRECTORY STRUCTURE (server/src/):

  controllers/    — HTTP route handlers (thin layer)
  services/       — Business logic (30+ services)
  repositories/   — Database access layer
  schema/tables/  — Kysely table definitions (40+ tables)
  dtos/           — Data Transfer Objects (API contracts)
  queries/        — Raw SQL queries for complex operations
  types/          — Shared TypeScript types
  middleware/     — Auth guards, error handlers
  workers/        — Background job workers
  commands/       — CLI commands
  emails/         — Email templates
  maintenance/    — Scheduled maintenance tasks
```

### Service Inventory (30+ Services)

```
CORE SERVICES:
  asset.service          — File CRUD, visibility, soft-delete
  asset-media.service    — Upload handling, thumbnail generation
  album.service          — Album CRUD, sharing, user roles
  library.service        — External library import/scan
  auth.service           — JWT, OAuth, session management
  user.service           — User profiles, quotas, preferences

MEDIA PROCESSING:
  media.service          — Video transcoding, image conversion
  job.service            — Background job dispatch + monitoring
  duplicate.service      — CLIP-based duplicate detection

SMART FEATURES:
  map.service            — GPS coordinate → location name
  memory.service         — "On this day" memories generation
  search.service         — Full-text + CLIP vector search

INFRASTRUCTURE:
  database.service       — Migration runner, health checks
  database-backup.service — Automated PostgreSQL dumps
  audit.service          — Entity change audit trail
  maintenance.service    — Scheduled cleanup (orphans, trash)
  cli.service            — Admin CLI commands

SOCIAL:
  activity.service       — Comments/likes on albums
  notification.service   — Push notifications
  download.service       — Bulk download (zip generation)

HyprDrive mapping:
  Most maps 1:1 to our planned CQRS actions/queries.
  audit.service → our sync_operations table
  memory.service → virtual_folder with date-based FilterExpr
  duplicate.service → our FilterExpr::Duplicate variant
```

---

## Chapter 3: Domain Model

### Asset (Central Entity)

```
Asset {
  id:               UUID
  checksum:         Buffer (content hash — like our ObjectId)
  deviceAssetId:    string (device-local identifier)
  deviceId:         string (which device uploaded this)
  fileCreatedAt:    Date
  fileModifiedAt:   Date
  isExternal:       boolean (from external library import)
  visibility:       AssetVisibility (Timeline | Hidden | Archive | Locked)
  libraryId:        UUID?
  livePhotoVideoId: UUID? (links photo to its Live Photo video)
  localDateTime:    Date (timezone-adjusted)
  originalFileName: string
  originalPath:     string
  ownerId:          UUID
  type:             AssetType (Image | Video | Audio | Other)
}

HyprDrive parallel:
  Asset ≈ Object + Location combined
  checksum ≈ ObjectId (content-addressed)
  visibility ≈ could be a tag/filter-based concept
  Key difference: Immich has ONE table; HyprDrive splits into
  Object (content) + Location (path) for dedup awareness.
```

### Album Model

```
Album {
  id, ownerId, albumName, description
  createdAt, updatedAt, deletedAt
  albumThumbnailAssetId — cover photo
  isActivityEnabled — comments/likes toggle
  order — AssetOrder (Asc/Desc)
  albumUsers[] — shared with (Editor/Viewer roles)
  assets[] — many-to-many via album_assets
}

HyprDrive adaptation:
  Album → VirtualFolder with a fixed asset list (not a query).
  Or: Tag-based albums (tag assets, album = tag query).
  Sharing → CapabilityToken with album-scoped permissions.
```

### EXIF / Metadata

```
AssetExif {
  assetId, make, model, exifImageWidth/Height
  orientation, exposureTime, fNumber, iso
  focalLength, lensModel, latitude, longitude
  country, state, city, description
  dateTimeOriginal, projectionType
  fps, rating  
}

HyprDrive adaptation:
  Store in `metadata` table with namespace = "exif".
  GPS data enables map view (Phase 11 desktop UI).
```

### User & Permissions

```
130+ fine-grained Permission enum values:
  asset.read, asset.upload, asset.delete, asset.download
  album.create, album.share, albumAsset.create
  person.read, person.merge, person.reassign
  tag.create, tag.asset
  workflow.create/read/update/delete
  admin.user.create/read/update/delete
  ...

Key pattern: RESOURCE.ACTION format
  "asset.download" → can download assets
  "album.share"    → can share albums
  "person.merge"   → can merge face clusters

HyprDrive adaptation:
  Our CapabilityToken.permissions already uses a similar pattern.
  Adopt the RESOURCE.ACTION naming convention.
```

---

## Chapter 4: Machine Learning Pipeline

### Architecture

```
ML SERVICE (Python, separate process):

  Endpoints:
    POST /predict  — Run CLIP/face detection on uploaded asset
    
  Pipeline per upload:
    1. Asset uploaded → job queued (BullMQ)
    2. ML worker picks up job
    3. CLIP: Generate 512-dim embedding vector
    4. Face: Detect faces → crop → generate 512-dim face embedding
    5. Store vectors in PostgreSQL (pgvector/VectorChord)
    6. Vectors enable: smart search, duplicate detection, face grouping

  Models used:
    CLIP:    OpenAI ViT-B/32 (ONNX format)
    Faces:   InsightFace antelopev2 / buffalo_l/m/s
    Runtime: ONNX Runtime (supports CPU, CUDA, ROCm, OpenVINO, RKNN)
```

### CLIP Smart Search

```
How "search for sunset photos" works:

  1. User types "sunset" in search bar
  2. Server sends text to ML service
  3. ML service encodes "sunset" as 512-dim text vector
  4. Database query: cosine similarity between text vector and all asset vectors
  5. Return top-N most similar assets

  SQL (simplified):
    SELECT id, 1 - (embedding <=> $query_embedding) AS similarity
    FROM smart_search
    ORDER BY similarity DESC
    LIMIT 100;

  <=> is pgvector's cosine distance operator

HyprDrive adaptation:
  Phase 19 (Search): 
    - Use `ort` crate for ONNX inference (CLIP model)
    - Store embeddings in a separate table or redb cache
    - For SQLite: use custom vtab or brute-force with f32 arrays
    - For production scale: consider Qdrant/Milvus sidecar
```

### Facial Recognition

```
Pipeline:
  1. Detect faces in image (InsightFace detection model)
  2. Crop + align each face
  3. Generate 512-dim embedding per face
  4. Cluster embeddings (DBSCAN or similar)
  5. User names clusters → "This is Jamie"
  6. New photos auto-tagged with known faces

Tables:
  asset_faces   — face bounding boxes per asset
  person        — named face clusters
  face_search   — face embedding vectors  

HyprDrive adaptation:
  Phase 17 (Media Pipeline) or Phase 20 (WASM Extension):
    - Face detection model as WASM plugin
    - Store face regions in metadata table
    - Cluster embeddings in daemon
```

### Hardware Acceleration Matrix

```
| Platform   | API       | Immich Tag     |
|-----------|-----------|----------------|
| NVIDIA     | CUDA      | --extra cuda   |
| AMD        | ROCm      | --extra rocm   |
| Intel      | OpenVINO  | --extra openvino|
| ARM/RK35xx | RKNN      | --extra rknn   |
| CPU        | Default   | --extra cpu    |

HyprDrive: If using `ort` crate, these map to ort ExecutionProviders.
```

---

## Chapter 5: Database Schema

### Table Architecture (40+ Tables)

```
CORE TABLES:
  assets          — Central file metadata (id, checksum, type, dates, path)
  asset_files     — Multiple file versions (original, preview, thumbnail, sidecar)
  asset_exif      — EXIF metadata (camera, GPS, orientation)
  asset_faces     — Detected face regions (bounding box, embedding_id)
  asset_job_status — Per-asset ML job tracking
  asset_metadata  — Key-value metadata
  asset_ocr       — OCR text extracted from images
  
ORGANIZATION:
  albums          — Photo albums
  album_assets    — Many-to-many (album ↔ asset)
  album_users     — Sharing (album ↔ user + role)
  tags            — Hierarchical tags (parentId)
  tags_assets     — Many-to-many (tag ↔ asset)
  stacks          — Asset stacking (burst photos, edits)
  
SOCIAL:
  activities      — Comments/likes
  partners        — User-to-user sharing
  shared_links    — Public/private share links (with expiry, password)
  memories        — "On this day" auto-generated memories

ML / SEARCH:
  smart_search    — CLIP embedding vectors (pgvector)
  face_search     — Face embedding vectors
  person          — Named face clusters

AUDIT TRAIL (pattern: {entity}_audit tables):
  asset_audit, album_audit, album_asset_audit,
  album_user_audit, asset_face_audit, asset_edit_audit,
  asset_metadata_audit
  
  Pattern: Every entity change writes to {entity}_audit
  Enables: sync, undo, conflict resolution

SYSTEM:
  users, sessions, api_keys
  libraries       — External folder imports
  system_metadata — Server config, feature flags
  geodata_places  — Reverse geocoding (city/country from GPS)
  notifications   — Push notification queue

PLUGINS/WORKFLOWS:
  plugins         — Installed plugin registry
  plugin_actions  — Plugin-registered actions
  plugin_filters  — Plugin-registered filters
  workflows       — User-created automation workflows
  workflow_actions — Workflow action steps
  workflow_filters — Workflow trigger conditions
```

### Audit Trail Pattern

```
For every core entity, Immich creates a paired *_audit table:

  assets          →  asset_audit
  albums          →  album_audit
  album_assets    →  album_asset_audit
  asset_faces     →  asset_face_audit
  asset_edits     →  asset_edit_audit
  asset_metadata  →  asset_metadata_audit
  album_users     →  album_user_audit

Each audit row records:
  - entity_id
  - action (CREATE | UPDATE | DELETE)
  - changed_fields (JSON diff)
  - timestamp
  - user_id (who made the change)

This enables:
  1. Full change history ("who deleted this photo?")
  2. Incremental sync between devices  
  3. Undo/redo (reverse the diff)
  4. Compliance auditing

HyprDrive adaptation:
  Our sync_operations table serves a similar purpose.
  Consider adding per-entity audit tables for rich undo:
    - operation_id → SyncOperation.id
    - entity_type → "asset" | "album" | "tag"
    - entity_id → UUID
    - action → CREATE | UPDATE | DELETE  
    - diff → serde_json::Value (before/after)
```

---

## Chapter 6: Job System

### BullMQ Job Queue

```
Immich uses Redis-backed BullMQ for background processing:

Job Types:
  THUMBNAIL_GENERATION    — Create preview/thumbnail on upload
  METADATA_EXTRACTION     — Read EXIF, extract dates, GPS
  VIDEO_CONVERSION        — Transcode to web-playable format
  FACE_DETECTION          — Run InsightFace on new assets
  CLIP_ENCODING           — Generate CLIP embedding vector
  SIDECAR_DISCOVERY       — Find .xmp sidecar files
  LIBRARY_SCAN            — Scan external library folders
  STORAGE_MIGRATION       — Move files between storage locations
  DUPLICATE_DETECTION     — Find similar assets via CLIP distance
  OCR                     — Extract text from images

Job Lifecycle:
  Created → Waiting → Active → (Failed → Retried) → Completed

Each job type has configurable:
  - Concurrency (how many run in parallel)
  - Priority (thumbnails before CLIP)
  - Retry count and backoff strategy

HyprDrive adaptation:
  Our task-system crate handles this. Key patterns to adopt:
  1. Job types as enum variants (not strings)
  2. Configurable concurrency per job type
  3. Priority scheduling (thumbnails > CLIP > OCR)
  4. Per-job progress reporting (for UI progress bars)
  5. Durable checkpoints (survive daemon restart)
```

---

## Chapter 7: Plugin & Workflow System

### Plugin Architecture

```
Immich's plugin system (recently added):

  Plugin {
    id, name, version, description
    enabled: boolean
    sourceUrl: string (npm package or local)
    config: JSON
  }

  PluginAction {
    pluginId, name, description
    configSchema: JSONSchema
    handler: function reference
  }

  PluginFilter {
    pluginId, name, description
    triggerType: PluginTriggerType
    configSchema: JSONSchema  
  }

Trigger Types:
  - onAssetUpload
  - onAssetDelete
  - onAlbumCreate
  - onSchedule (cron-based)

HyprDrive adaptation:
  Phase 20 (WASM Extension System):
  - Plugins compile to WASM (not JS)
  - wasmtime AOT execution (ADR-002)
  - Same trigger types but type-safe via Rust traits
  - Plugin manifest in TOML (not JSON)
```

### Workflow Automation

```
Workflows = user-created automation chains:

  Workflow {
    name, description, enabled
    triggers: WorkflowFilter[]  — "when this happens"
    actions: WorkflowAction[]   — "do this"  
  }

  Example: "Auto-archive screenshots older than 30 days"
    Trigger: onSchedule (daily)
    Filter: asset.type == Image AND metadata.source == "screenshot"
    Action: setVisibility(Hidden)

HyprDrive adaptation:
  Our VirtualFolder + FilterExpr already enables the filter part.
  Add: ActionExpr enum to complement FilterExpr
    ActionExpr::SetTag(TagId)
    ActionExpr::Move(LocationId)
    ActionExpr::Archive
    ActionExpr::Delete
    ActionExpr::RunPlugin(PluginId)
```

---

## Chapter 8: Patterns Applicable to HyprDrive

### High-Value Patterns to Adopt

| # | Pattern | Immich Source | HyprDrive Phase |
|---|---|---|---|
| 1 | Separate ML process from server | Docker architecture | Phase 17/20 |
| 2 | CLIP embeddings for smart search | ML service | Phase 19 |
| 3 | Per-entity audit tables | *_audit schema tables | Phase 2 |
| 4 | Fine-grained RESOURCE.ACTION permissions | Permission enum | Phase 9 |
| 5 | Multiple file versions per asset | asset_files table | Phase 2 |
| 6 | Reverse geocoding from GPS | geodata_places | Phase 11 |
| 7 | Live Photo video linking | livePhotoVideoId FK | Phase 11 |
| 8 | Memory/timeline auto-generation | memory.service | Phase 11 |
| 9 | Plugin trigger types + workflow chains | plugin/workflow tables | Phase 20 |
| 10 | Configurable job concurrency | job.service | Phase 7 |

### Anti-Patterns to Avoid

```
1. SINGLE ASSET TABLE
   Immich stores everything in one `assets` table.
   HyprDrive uses Object+Location split for dedup awareness.
   → Keep our split. It's architecturally superior.

2. PostgreSQL DEPENDENCY
   Immich requires PostgreSQL + pgvector extension.
   HyprDrive uses embedded SQLite — zero external deps.
   → Don't adopt pgvector. Use FTS5 + in-memory vector index.

3. REDIS FOR JOB QUEUE
   Immich needs Redis/Valkey for BullMQ.
   HyprDrive's task-system crate uses SQLite-backed queues.
   → Keep our approach. One less service to manage.

4. TypeScript ORM (Kysely)
   Immich uses Kysely for type-safe SQL.
   HyprDrive uses raw sqlx queries.
   → Keep raw sqlx — more control, less abstraction.
```

---

## Quick Reference: File Locations

```
immich-app/immich/
├── server/src/
│   ├── controllers/     — 20+ API controllers
│   ├── services/        — 30+ business logic services
│   ├── repositories/    — Database access layer
│   ├── schema/tables/   — 40+ Kysely table definitions
│   ├── dtos/            — API DTOs (validation)
│   ├── enum.ts          — ALL enums (130+ permissions)
│   ├── database.ts      — Domain types (25+)
│   └── config.ts        — System config defaults
├── machine-learning/
│   ├── immich_ml/        — Python ML package
│   │   ├── clip/         — CLIP embedding model
│   │   ├── facial_recognition/ — InsightFace
│   │   └── session.py    — ONNX Runtime session
│   └── pyproject.toml    — uv/pip dependencies
├── web/                  — SvelteKit frontend
├── mobile/               — Flutter/Dart mobile app
├── open-api/             — Generated OpenAPI specs
└── docker/               — Docker compose + env
```
