import { sqliteTable, text, integer, index } from "drizzle-orm/sqlite-core";
import { customers } from "./counterparties.js";
import { products } from "./catalog.js";
import { users } from "./business.js";

export const customerOrders = sqliteTable("customer_orders", {
  id: text("id").primaryKey(),
  orderNumber: text("order_number").notNull().unique(),
  customerId: text("customer_id").references(() => customers.id, { onDelete: "set null" }),
  status: text("status").notNull().default("DRAFT"), // 'DRAFT' | 'CONVERTED' | 'CANCELLED'
  convertedSaleId: text("converted_sale_id"), // populated when converted to a confirmed sale
  notes: text("notes"),
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  createdAt: text("created_at").notNull(),
  updatedAt: text("updated_at").notNull(),
}, (table) => [
  index("idx_customer_orders_status").on(table.status),
  index("idx_customer_orders_customer").on(table.customerId),
]);

export const customerOrderItems = sqliteTable("customer_order_items", {
  id: text("id").primaryKey(),
  orderId: text("order_id").notNull().references(() => customerOrders.id, { onDelete: "cascade" }),
  productId: text("product_id").notNull().references(() => products.id, { onDelete: "restrict" }),
  quantity: integer("quantity").notNull(), // millie-units
  unitPriceCents: integer("unit_price_cents").notNull(), // paise/cents
  notes: text("notes"),
}, (table) => [
  index("idx_customer_order_items_order").on(table.orderId),
]);

export type CustomerOrder = typeof customerOrders.$inferSelect;
export type NewCustomerOrder = typeof customerOrders.$inferInsert;

export type CustomerOrderItem = typeof customerOrderItems.$inferSelect;
export type NewCustomerOrderItem = typeof customerOrderItems.$inferInsert;
