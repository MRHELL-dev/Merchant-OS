import { sqliteTable, text, integer, index } from "drizzle-orm/sqlite-core";
import { users } from "./business.js";

export const customers = sqliteTable("customers", {
  id: text("id").primaryKey(),
  name: text("name").notNull(),
  phone: text("phone").unique(),
  address: text("address"),
  currentCreditCents: integer("current_credit_cents").notNull().default(0),
  isActive: integer("is_active").notNull().default(1),
  createdAt: text("created_at").notNull(),
  updatedAt: text("updated_at").notNull(),
}, (table) => [
  index("idx_customers_name").on(table.name),
  index("idx_customers_phone").on(table.phone),
]);

export const customerLedger = sqliteTable("customer_ledger", {
  id: text("id").primaryKey(),
  customerId: text("customer_id").notNull().references(() => customers.id, { onDelete: "restrict" }),
  entryType: text("entry_type").notNull(), // 'SALE_CREDIT' | 'PAYMENT_RECEIVED' | 'RETURN_CREDIT'
  amountCents: integer("amount_cents").notNull(),
  balanceBeforeCents: integer("balance_before_cents").notNull(),
  balanceAfterCents: integer("balance_after_cents").notNull(),
  referenceType: text("reference_type").notNull(), // 'SALE' | 'PAYMENT' | 'RETURN'
  referenceId: text("reference_id").notNull(),
  notes: text("notes"),
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_customer_ledger_customer").on(table.customerId),
  index("idx_customer_ledger_created").on(table.createdAt),
]);

export const suppliers = sqliteTable("suppliers", {
  id: text("id").primaryKey(),
  name: text("name").notNull(),
  phone: text("phone"),
  address: text("address"),
  currentOutstandingCents: integer("current_outstanding_cents").notNull().default(0),
  isActive: integer("is_active").notNull().default(1),
  createdAt: text("created_at").notNull(),
  updatedAt: text("updated_at").notNull(),
}, (table) => [
  index("idx_suppliers_name").on(table.name),
]);

export const supplierLedger = sqliteTable("supplier_ledger", {
  id: text("id").primaryKey(),
  supplierId: text("supplier_id").notNull().references(() => suppliers.id, { onDelete: "restrict" }),
  entryType: text("entry_type").notNull(), // 'PURCHASE_CREDIT' | 'PAYMENT_MADE' | 'RETURN_DEBIT'
  amountCents: integer("amount_cents").notNull(),
  balanceBeforeCents: integer("balance_before_cents").notNull(),
  balanceAfterCents: integer("balance_after_cents").notNull(),
  referenceType: text("reference_type").notNull(), // 'PURCHASE' | 'PAYMENT' | 'RETURN'
  referenceId: text("reference_id").notNull(),
  notes: text("notes"),
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_supplier_ledger_supplier").on(table.supplierId),
  index("idx_supplier_ledger_created").on(table.createdAt),
]);

export type Customer = typeof customers.$inferSelect;
export type NewCustomer = typeof customers.$inferInsert;

export type CustomerLedgerEntry = typeof customerLedger.$inferSelect;
export type NewCustomerLedgerEntry = typeof customerLedger.$inferInsert;

export type Supplier = typeof suppliers.$inferSelect;
export type NewSupplier = typeof suppliers.$inferInsert;

export type SupplierLedgerEntry = typeof supplierLedger.$inferSelect;
export type NewSupplierLedgerEntry = typeof supplierLedger.$inferInsert;
