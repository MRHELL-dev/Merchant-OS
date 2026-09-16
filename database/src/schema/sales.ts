import { sqliteTable, text, integer, index } from "drizzle-orm/sqlite-core";
import { customers } from "./counterparties.js";
import { products } from "./catalog.js";
import { users } from "./business.js";

export const sales = sqliteTable("sales", {
  id: text("id").primaryKey(),
  saleNumber: text("sale_number").notNull().unique(),
  customerId: text("customer_id").references(() => customers.id, { onDelete: "set null" }),
  totalAmountCents: integer("total_amount_cents").notNull(),
  paidAmountCents: integer("paid_amount_cents").notNull(),
  creditAmountCents: integer("credit_amount_cents").notNull().default(0),
  paymentStatus: text("payment_status").notNull(), // 'PAID' | 'PARTIAL' | 'UNPAID'
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  saleDate: text("sale_date").notNull(), // YYYY-MM-DD
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_sales_customer").on(table.customerId),
  index("idx_sales_date").on(table.saleDate),
  index("idx_sales_created").on(table.createdAt),
]);

export const saleItems = sqliteTable("sale_items", {
  id: text("id").primaryKey(),
  saleId: text("sale_id").notNull().references(() => sales.id, { onDelete: "cascade" }),
  productId: text("product_id").notNull().references(() => products.id, { onDelete: "restrict" }),
  quantity: integer("quantity").notNull(), // millie-units
  unitPriceCents: integer("unit_price_cents").notNull(), // historical selling price
  costPriceCents: integer("cost_price_cents").notNull(), // historical cost price
  totalCents: integer("total_cents").notNull(),
}, (table) => [
  index("idx_sale_items_sale").on(table.saleId),
  index("idx_sale_items_product").on(table.productId),
]);

export type Sale = typeof sales.$inferSelect;
export type NewSale = typeof sales.$inferInsert;

export type SaleItem = typeof saleItems.$inferSelect;
export type NewSaleItem = typeof saleItems.$inferInsert;
