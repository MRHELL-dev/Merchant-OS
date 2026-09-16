import { sqliteTable, text, index } from "drizzle-orm/sqlite-core";
import { users } from "./business.js";

export const auditLogs = sqliteTable("audit_logs", {
  id: text("id").primaryKey(),
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  action: text("action").notNull(), // e.g. 'CONFIRM_SALE', 'APPROVE_CUSTOMER_RETURN', etc.
  entityType: text("entity_type").notNull(),
  entityId: text("entity_id").notNull(),
  details: text("details"), // JSON payload capturing change context
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_audit_logs_user").on(table.userId),
  index("idx_audit_logs_action").on(table.action),
  index("idx_audit_logs_entity").on(table.entityType, table.entityId),
  index("idx_audit_logs_created").on(table.createdAt),
]);

export type AuditLog = typeof auditLogs.$inferSelect;
export type NewAuditLog = typeof auditLogs.$inferInsert;
