import { sqliteTable, text, integer, index } from "drizzle-orm/sqlite-core";
import { businesses } from "./business.js";

export const categories = sqliteTable("categories", {
  id: text("id").primaryKey(),
  name: text("name").notNull(),
  slug: text("slug").notNull().unique(),
  description: text("description"),
  createdAt: text("created_at").notNull(),
});

export const products = sqliteTable("products", {
  id: text("id").primaryKey(),
  businessId: text("business_id").references(() => businesses.id, { onDelete: "cascade" }),
  categoryId: text("category_id").references(() => categories.id, { onDelete: "set null" }),
  name: text("name").notNull(),
  productType: text("product_type").notNull().default("PACKAGED"), // 'PACKAGED' | 'LOOSE'
  unit: text("unit").notNull().default("pcs"), // 'pcs', 'kg', 'g', 'ltr', 'ml', 'pack'
  costPriceCents: integer("cost_price_cents").notNull().default(0),
  sellingPriceCents: integer("selling_price_cents").notNull().default(0),
  minStockLevel: integer("min_stock_level").notNull().default(0), // scale: 1000 (millie-units)
  isActive: integer("is_active").notNull().default(1),
  createdAt: text("created_at").notNull(),
  updatedAt: text("updated_at").notNull(),
}, (table) => [
  index("idx_products_name").on(table.name),
  index("idx_products_category").on(table.categoryId),
  index("idx_products_business").on(table.businessId),
]);

export const barcodeMappings = sqliteTable("barcode_mappings", {
  id: text("id").primaryKey(),
  barcode: text("barcode").notNull().unique(),
  productId: text("product_id").notNull().references(() => products.id, { onDelete: "cascade" }),
  barcodeType: text("barcode_type").notNull().default("MANUFACTURER"), // 'MANUFACTURER' | 'WEIGHING_SCALE' | 'INTERNAL'
  notes: text("notes"),
  createdAt: text("created_at").notNull(),
}, (table) => [
  index("idx_barcode_mappings_product").on(table.productId),
]);

export type Category = typeof categories.$inferSelect;
export type NewCategory = typeof categories.$inferInsert;

export type Product = typeof products.$inferSelect;
export type NewProduct = typeof products.$inferInsert;

export type BarcodeMapping = typeof barcodeMappings.$inferSelect;
export type NewBarcodeMapping = typeof barcodeMappings.$inferInsert;
