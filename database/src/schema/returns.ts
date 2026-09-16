import { sqliteTable, text, integer, index } from "drizzle-orm/sqlite-core";
import { customers, suppliers } from "./counterparties.js";
import { products } from "./catalog.js";
import { users } from "./business.js";

export const returns = sqliteTable("returns", {
  id: text("id").primaryKey(),
  returnNumber: text("return_number").notNull().unique(),
  returnType: text("return_type").notNull(), // 'CUSTOMER_RETURN' | 'SUPPLIER_RETURN'
  referenceId: text("reference_id"), // original sale_id or purchase_id
  customerId: text("customer_id").references(() => customers.id, { onDelete: "restrict" }),
  supplierId: text("supplier_id").references(() => suppliers.id, { onDelete: "restrict" }),
  totalAmountCents: integer("total_amount_cents").notNull(),
  reason: text("reason").notNull(),
  adminUserId: text("admin_user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_returns_type").on(table.returnType),
  index("idx_returns_customer").on(table.customerId),
  index("idx_returns_supplier").on(table.supplierId),
  index("idx_returns_created").on(table.createdAt),
]);

export const returnItems = sqliteTable("return_items", {
  id: text("id").primaryKey(),
  returnId: text("return_id").notNull().references(() => returns.id, { onDelete: "cascade" }),
  productId: text("product_id").notNull().references(() => products.id, { onDelete: "restrict" }),
  quantity: integer("quantity").notNull(), // millie-units
  unitPriceCents: integer("unit_price_cents").notNull(), // historical price (selling or cost)
  totalCents: integer("total_cents").notNull(),
}, (table) => [
  index("idx_return_items_return").on(table.returnId),
  index("idx_return_items_product").on(table.productId),
]);

export type Return = typeof returns.$inferSelect;
export type NewReturn = typeof returns.$inferInsert;

export type ReturnItem = typeof returnItems.$inferSelect;
export type NewReturnItem = typeof returnItems.$inferInsert;
