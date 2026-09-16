# AI Engine

This directory is reserved for local, offline-capable AI capabilities (e.g. natural language queries, intelligent assistants, inventory predictions).

## Architecture Boundaries
1. The AI engine must **never** directly modify SQLite or bypass transaction safety boundaries.
2. AI-driven suggestions or actions must be proposed as intents or transactions submitted to the `core` Business Engine.
3. Operates entirely without external cloud dependency or data leakage.
