# Merchant OS

Merchant OS is an offline-first desktop operating system and application for modern merchants, built with Tauri 2, React, TypeScript, Rust, SQLite, and Drizzle ORM.

## Project Structure

```
merchant-os/
├── apps/
│   └── desktop/           # Tauri 2 + React + Rust desktop application
│       ├── src/           # React UI layer
│       └── src-tauri/     # Rust backend & SQLite database owner
├── domains/               # Business domain modules (placeholder)
├── core/                  # Business Engine & Transaction Engine (placeholder)
├── database/              # Drizzle ORM schema definitions and migration tooling
├── ai/                    # AI Engine (placeholder)
├── shared/                # Shared TypeScript contracts & IPC types
├── tests/                 # Foundation verification tests
├── package.json           # Monorepo package workspaces
├── tsconfig.base.json     # Strict TypeScript configuration
└── README.md
```

## Non-Negotiable Architecture Rules

1. **Authoritative SQLite Ownership**: The Rust/Tauri backend (`apps/desktop/src-tauri/src/db/`) is the sole runtime owner of the local SQLite database.
2. **UI Isolation**: The React UI must **never** directly access or modify SQLite. All interactions occur via typed Tauri IPC.
3. **No Dual Runtime Database**: Do not create a second independent runtime database layer in TypeScript.
4. **AI Isolation**: AI engines must **never** directly modify SQLite.
5. **Engine Mediation**: All future business-changing operations must pass through the Business Engine and Transaction Engine.
6. **100% Offline Capability**: The application operates completely without internet connectivity.
7. **No Cloud/PostgreSQL**: Cloud and PostgreSQL are strictly out of scope for the local foundation.
8. **Minimal Dependencies**: Maintain a lean, auditable dependency footprint.
9. **Strict Type Safety**: TypeScript strict mode enabled across all packages.
10. **Honest Testing**: No mocked or simulated tests claiming database verification without actual execution.
