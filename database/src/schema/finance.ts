import { sqliteTable, text, integer, index } from "drizzle-orm/sqlite-core";
import { users } from "./business.js";

export const payments = sqliteTable("payments", {
  id: text("id").primaryKey(),
  paymentType: text("payment_type").notNull(), // 'CUSTOMER_SALE' | 'CUSTOMER_CREDIT_SETTLEMENT' | 'PURCHASE_PAYMENT' | 'SUPPLIER_DUES_SETTLEMENT' | 'CUSTOMER_RETURN_REFUND'
  relatedEntityType: text("related_entity_type").notNull(), // 'SALE' | 'CUSTOMER' | 'PURCHASE' | 'SUPPLIER' | 'RETURN'
  relatedEntityId: text("related_entity_id").notNull(), // Polymorphic reference validated by application/Rust layer
  amountCents: integer("amount_cents").notNull(),
  paymentMethod: text("payment_method").notNull(), // 'CASH' | 'UPI' | 'BANK_TRANSFER' | 'CARD' | 'OTHER'
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  notes: text("notes"),
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_payments_related").on(table.relatedEntityType, table.relatedEntityId),
  index("idx_payments_method").on(table.paymentMethod),
  index("idx_payments_created").on(table.createdAt),
]);

export const expenses = sqliteTable("expenses", {
  id: text("id").primaryKey(),
  expenseName: text("expense_name").notNull(),
  amountCents: integer("amount_cents").notNull(),
  category: text("category").notNull(), // 'UTILITIES' | 'RENT' | 'TRANSPORT' | 'TEA_SNACKS' | 'PACKAGING' | 'MAINTENANCE' | 'OTHER'
  expenseDate: text("expense_date").notNull(), // YYYY-MM-DD
  notes: text("notes"),
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_expenses_date").on(table.expenseDate),
  index("idx_expenses_category").on(table.category),
]);

export type Payment = typeof payments.$inferSelect;
export type NewPayment = typeof payments.$inferInsert;

export type Expense = typeof expenses.$inferSelect;
export type NewExpense = typeof expenses.$inferInsert;
