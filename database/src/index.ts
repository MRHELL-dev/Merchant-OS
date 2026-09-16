/**
 * Merchant OS - Database Schema & Migration Definitions
 * 
 * Note: Runtime SQLite access is strictly owned by the Rust backend.
 * Drizzle is used here solely for schema definitions, contract typing,
 * and migration generation.
 */

export * from "./schema/index.js";
