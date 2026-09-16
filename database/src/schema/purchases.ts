import { sqliteTable, text, integer, index } from "drizzle-orm/sqlite-core";
import { suppliers } from "./counterparties.js";
import { products } from "./catalog.js";
import { users } from "./business.js";

export const purchases = sqliteTable("purchases", {
  id: text("id").primaryKey(),
  purchaseNumber: text("purchase_number").notNull().unique(),
  supplierId: text("supplier_id").notNull().references(() => suppliers.id, { onDelete: "restrict" }),
  totalAmountCents: integer("total_amount_cents").notNull(),
  paidAmountCents: integer("paid_amount_cents").notNull(),
  creditAmountCents: integer("credit_amount_cents").notNull().default(0),
  paymentMethod: text("payment_method"), // 'CASH' | 'UPI' | 'BANK_TRANSFER' | 'CARD' | 'OTHER' (null if 100% on credit)
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  purchaseDate: text("purchase_date").notNull(), // YYYY-MM-DD
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_purchases_supplier").on(table.supplierId),
  index("idx_purchases_date").on(table.purchaseDate),
]);

export const purchaseItems = sqliteTable("purchase_items", {
  id: text("id").primaryKey(),
  purchaseId: text("purchase_id").notNull().references(() => purchases.id, { onDelete: "cascade" }),
  productId: text("product_id").notNull().references(() => products.id, { onDelete: "restrict" }),
  quantity: integer("quantity").notNull(), // millie-units
  unitCostCents: integer("unit_cost_cents").notNull(), // actual buying price
  totalCents: integer("total_cents").notNull(),
}, (table) => [
  index("idx_purchase_items_purchase").on(table.purchaseId),
  index("idx_purchase_items_product").on(table.productId),
]);

export type Purchase = typeof purchases.$inferSelect;
export type NewPurchase = typeof purchases.$inferInsert;

export type PurchaseItem = typeof purchaseItems.$inferSelect;
export type NewPurchaseItem = typeof purchaseItems.$inferInsert;
