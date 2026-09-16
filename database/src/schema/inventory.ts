import { sqliteTable, text, integer, index, uniqueIndex } from "drizzle-orm/sqlite-core";
import { products } from "./catalog.js";
import { users } from "./business.js";

export const inventory = sqliteTable("inventory", {
  id: text("id").primaryKey(),
  productId: text("product_id").notNull().references(() => products.id, { onDelete: "cascade" }),
  currentQuantity: integer("current_quantity").notNull().default(0), // scale: 1000 (millie-units)
  lastUpdatedAt: text("last_updated_at").notNull(),
}, (table) => [
  uniqueIndex("idx_inventory_product").on(table.productId),
]);

export const stockMovements = sqliteTable("stock_movements", {
  id: text("id").primaryKey(),
  productId: text("product_id").notNull().references(() => products.id, { onDelete: "restrict" }),
  quantityChange: integer("quantity_change").notNull(), // signed delta in millie-units
  quantityBefore: integer("quantity_before").notNull(),
  quantityAfter: integer("quantity_after").notNull(),
  movementType: text("movement_type").notNull(), // 'PURCHASE' | 'SALE' | 'CUSTOMER_RETURN' | 'SUPPLIER_RETURN' | 'CORRECTION'
  referenceType: text("reference_type").notNull(), // 'SALES' | 'PURCHASES' | 'RETURNS' | 'STOCK_CORRECTIONS'
  referenceId: text("reference_id").notNull(),
  userId: text("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_stock_movements_product").on(table.productId),
  index("idx_stock_movements_ref").on(table.referenceType, table.referenceId),
  index("idx_stock_movements_created").on(table.createdAt),
]);

export const stockCorrections = sqliteTable("stock_corrections", {
  id: text("id").primaryKey(),
  productId: text("product_id").notNull().references(() => products.id, { onDelete: "restrict" }),
  quantityChange: integer("quantity_change").notNull(), // signed delta in millie-units
  reason: text("reason").notNull(), // 'DAMAGED' | 'EXPIRED' | 'LOST' | 'MISCOUNT'
  note: text("note").notNull(),
  adminUserId: text("admin_user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_stock_corrections_product").on(table.productId),
]);

export type Inventory = typeof inventory.$inferSelect;
export type NewInventory = typeof inventory.$inferInsert;

export type StockMovement = typeof stockMovements.$inferSelect;
export type NewStockMovement = typeof stockMovements.$inferInsert;

export type StockCorrection = typeof stockCorrections.$inferSelect;
export type NewStockCorrection = typeof stockCorrections.$inferInsert;
