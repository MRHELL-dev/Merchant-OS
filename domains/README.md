# Business Domains

This directory is reserved for domain-driven business logic modules (e.g. inventory, catalog, billing, order management).

## Architecture Boundaries
1. Domain logic must remain modular and decoupled from the presentation (React UI) layer.
2. Business operations must be mediated through the `core` Transaction and Business Engines.
3. No business logic in this directory directly accesses the SQLite database.
