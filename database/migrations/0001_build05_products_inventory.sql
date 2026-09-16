ALTER TABLE `products` ADD `business_id` text REFERENCES `businesses`(`id`) ON UPDATE no action ON DELETE cascade;--> statement-breakpoint
ALTER TABLE `products` ADD `product_type` text DEFAULT 'PACKAGED' NOT NULL;--> statement-breakpoint
ALTER TABLE `products` ADD `min_stock_level` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
CREATE INDEX `idx_products_business` ON `products` (`business_id`);
