/**
 * Merchant OS - Shared IPC and Domain Contracts
 */

// --- Foundation Contracts ---
export interface SystemStatus {
  status: string;
  database: string;
  sqliteVersion: string;
  internet: string;
  timestamp: string;
}

export const SYSTEM_DEFAULTS = {
  status: "LOCAL SYSTEM ONLINE",
  databaseConnected: "SQLite — Connected",
  internet: "Not Required",
} as const;

// --- Domain Invariants & Constants ---
/** Fixed scale factor for deterministic fractional quantity arithmetic (millie-units: 1 unit = 1000) */
export const QUANTITY_SCALE = 1000 as const;

// --- Domain Enums & Types ---
export type Role = "ADMIN" | "EMPLOYEE";

export const PERMISSION_KEYS = [
  "DASHBOARD",
  "SALES",
  "PURCHASES",
  "INVENTORY",
  "PRICES",
  "CUSTOMERS",
  "CUSTOMER_CREDITS",
  "CUSTOMER_ORDERS",
  "SUPPLIERS",
  "RETURNS",
  "CORRECTION",
  "EMPLOYEES",
  "PERMISSIONS",
  "EXPENSES",
  "TRANSACTION_HISTORY",
  "REPORTS",
  "BUSINESS_PROFILE",
  "BACKUP_RESTORE",
] as const;

export type PermissionKey = (typeof PERMISSION_KEYS)[number];

export interface AuthenticatedIdentity {
  userId: string;
  username: string;
  role: Role;
}

export type AuthStateStatus = "FIRST_RUN_ADMIN_SETUP" | "AUTHENTICATED" | "UNAUTHENTICATED";

export interface AuthStateDto {
  status: AuthStateStatus;
  user: AuthenticatedIdentity | null;
}

export interface CreateInitialAdminInput {
  username: string;
  password: string;
  confirmPassword: string;
}

export interface LoginInput {
  username: string;
  password: string;
}
export type ProductType = "PACKAGED" | "LOOSE";
export type StockStatus = "LOW_STOCK" | "NORMAL";
export type BarcodeType = "MANUFACTURER" | "WEIGHING_SCALE" | "INTERNAL";
export type MovementType = "INITIAL_STOCK" | "PURCHASE" | "SALE" | "CUSTOMER_RETURN" | "SUPPLIER_RETURN" | "CORRECTION";
export type StockReferenceType = "PRODUCTS" | "SALES" | "PURCHASES" | "RETURNS" | "STOCK_CORRECTIONS";
export type CorrectionReason = "DAMAGED" | "EXPIRED" | "LOST" | "MISCOUNT";
export type CustomerLedgerEntryType = "SALE_CREDIT" | "PAYMENT_RECEIVED" | "RETURN_CREDIT";
export type SupplierLedgerEntryType = "PURCHASE_CREDIT" | "PAYMENT_MADE" | "RETURN_DEBIT";
export type OrderStatus = "DRAFT" | "CONVERTED" | "CANCELLED";
export type SalePaymentStatus = "PAID" | "PARTIAL" | "UNPAID";
export type PaymentMethod = "CASH" | "UPI" | "BANK_TRANSFER" | "CARD" | "OTHER";
export type PaymentType =
  | "CUSTOMER_SALE"
  | "CUSTOMER_CREDIT_SETTLEMENT"
  | "PURCHASE_PAYMENT"
  | "SUPPLIER_DUES_SETTLEMENT"
  | "CUSTOMER_RETURN_REFUND";
export type RelatedEntityType = "SALE" | "CUSTOMER" | "PURCHASE" | "SUPPLIER" | "RETURN";
export type ReturnType = "CUSTOMER_RETURN" | "SUPPLIER_RETURN";
export type ExpenseCategory =
  | "UTILITIES"
  | "RENT"
  | "TRANSPORT"
  | "TEA_SNACKS"
  | "PACKAGING"
  | "MAINTENANCE"
  | "OTHER";

// --- Domain Entity Contracts ---

export interface Business {
  id: string;
  name: string;
  phone: string;
  address: string;
  createdAt: string;
  updatedAt: string;
}

export interface User {
  id: string;
  username: string;
  passwordHash: string;
  role: Role;
  isActive: number;
  createdAt: string;
  updatedAt: string;
}

export interface Permission {
  id: string;
  userId: string;
  featureKey: string;
  isEnabled: number;
  updatedAt: string;
}

export interface Category {
  id: string;
  name: string;
  slug: string;
  description: string | null;
  createdAt: string;
}

export interface Product {
  id: string;
  businessId: string | null;
  categoryId: string | null;
  name: string;
  productType: ProductType;
  unit: string;
  costPriceCents: number;
  sellingPriceCents: number;
  minStockLevel: number;
  isActive: number;
  createdAt: string;
  updatedAt: string;
}

export interface BarcodeMapping {
  id: string;
  barcode: string;
  productId: string;
  barcodeType: BarcodeType;
  notes: string | null;
  createdAt: string;
}

export interface Inventory {
  id: string;
  productId: string;
  currentQuantity: number; // millie-units (scale 1000)
  lastUpdatedAt: string;
}

export interface StockMovement {
  id: string;
  productId: string;
  quantityChange: number; // signed delta in millie-units
  quantityBefore: number;
  quantityAfter: number;
  movementType: MovementType;
  referenceType: StockReferenceType;
  referenceId: string;
  userId: string;
  createdAt: string;
}

export interface StockCorrection {
  id: string;
  productId: string;
  quantityChange: number;
  reason: CorrectionReason;
  note: string;
  adminUserId: string;
  createdAt: string;
}

export interface Customer {
  id: string;
  name: string;
  phone: string | null;
  address: string | null;
  currentCreditCents: number;
  isActive: number;
  createdAt: string;
  updatedAt: string;
}

export interface CustomerLedgerEntry {
  id: string;
  customerId: string;
  entryType: CustomerLedgerEntryType;
  amountCents: number;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  referenceType: "SALE" | "PAYMENT" | "RETURN";
  referenceId: string;
  notes: string | null;
  userId: string;
  createdAt: string;
}

export interface Supplier {
  id: string;
  name: string;
  phone: string | null;
  address: string | null;
  currentOutstandingCents: number;
  isActive: number;
  createdAt: string;
  updatedAt: string;
}

export interface SupplierLedgerEntry {
  id: string;
  supplierId: string;
  entryType: SupplierLedgerEntryType;
  amountCents: number;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  referenceType: "PURCHASE" | "PAYMENT" | "RETURN";
  referenceId: string;
  notes: string | null;
  userId: string;
  createdAt: string;
}

export interface CustomerOrder {
  id: string;
  orderNumber: string;
  customerId: string | null;
  status: OrderStatus;
  convertedSaleId: string | null;
  notes: string | null;
  userId: string;
  createdAt: string;
  updatedAt: string;
}

export interface CustomerOrderItem {
  id: string;
  orderId: string;
  productId: string;
  quantity: number; // millie-units
  unitPriceCents: number; // paise/cents
  notes: string | null;
}

export interface Sale {
  id: string;
  saleNumber: string;
  customerId: string | null;
  totalAmountCents: number;
  paidAmountCents: number;
  creditAmountCents: number;
  paymentStatus: SalePaymentStatus;
  userId: string;
  saleDate: string;
  createdAt: string;
}

export interface SaleItem {
  id: string;
  saleId: string;
  productId: string;
  quantity: number; // millie-units
  unitPriceCents: number; // historical selling price
  costPriceCents: number; // historical cost price
  totalCents: number;
}

export interface Purchase {
  id: string;
  purchaseNumber: string;
  supplierId: string;
  totalAmountCents: number;
  paidAmountCents: number;
  creditAmountCents: number;
  paymentMethod: PaymentMethod | null;
  userId: string;
  purchaseDate: string;
  createdAt: string;
}

export interface PurchaseItem {
  id: string;
  purchaseId: string;
  productId: string;
  quantity: number; // millie-units
  unitCostCents: number; // actual buying price
  totalCents: number;
}

export interface Return {
  id: string;
  returnNumber: string;
  returnType: ReturnType;
  referenceId: string | null;
  customerId: string | null;
  supplierId: string | null;
  totalAmountCents: number;
  reason: string;
  adminUserId: string;
  createdAt: string;
}

export interface ReturnItem {
  id: string;
  returnId: string;
  productId: string;
  quantity: number; // millie-units
  unitPriceCents: number;
  totalCents: number;
}

export interface Payment {
  id: string;
  paymentType: PaymentType;
  relatedEntityType: RelatedEntityType;
  relatedEntityId: string;
  amountCents: number;
  paymentMethod: PaymentMethod;
  userId: string;
  notes: string | null;
  createdAt: string;
}

export interface Expense {
  id: string;
  expenseName: string;
  amountCents: number;
  category: ExpenseCategory;
  expenseDate: string;
  notes: string | null;
  userId: string;
  createdAt: string;
}

export interface AuditLog {
  id: string;
  userId: string;
  action: string;
  entityType: string;
  entityId: string;
  details: string | null;
  createdAt: string;
}

export interface SystemMetadata {
  id: number;
  key: string;
  value: string;
  updatedAt: string;
}

export interface SchemaMigration {
  id: number;
  version: string;
  appliedAt: string;
  checksum: string;
}

// --- Structured Command Contracts (BUILD 03) ---

export interface SaleItemCommandInput {
  productId: string;
  quantity: number; // millie-units (scale 1000)
  unitPriceCents?: number; // optional explicit price; defaults to product selling price
}

export interface ConfirmSaleCommand {
  saleId: string;
  saleNumber: string;
  customerId?: string | null;
  items: SaleItemCommandInput[];
  paidAmountCents: number;
  paymentMethod?: PaymentMethod | null;
  userId: string;
  saleDate: string;
}

export interface PurchaseItemCommandInput {
  productId: string;
  quantity: number; // millie-units (scale 1000)
  unitCostCents: number; // required actual buying cost
}

export interface ConfirmPurchaseCommand {
  purchaseId: string;
  purchaseNumber: string;
  supplierId: string;
  items: PurchaseItemCommandInput[];
  paidAmountCents: number;
  paymentMethod?: PaymentMethod | null;
  userId: string;
  purchaseDate: string;
}

export interface RecordCustomerPaymentCommand {
  paymentId: string;
  customerId: string;
  amountCents: number;
  paymentMethod: PaymentMethod;
  userId: string;
  notes?: string | null;
}

export interface RecordSupplierPaymentCommand {
  paymentId: string;
  supplierId: string;
  amountCents: number;
  paymentMethod: PaymentMethod;
  userId: string;
  notes?: string | null;
}

export interface ReturnItemCommandInput {
  productId: string;
  quantity: number; // millie-units (scale 1000)
  unitPriceCents: number;
}

export interface ProcessCustomerReturnCommand {
  returnId: string;
  returnNumber: string;
  referenceSaleId?: string | null;
  customerId: string;
  items: ReturnItemCommandInput[];
  reason: string;
  adminUserId: string;
}

export interface ProcessSupplierReturnCommand {
  returnId: string;
  returnNumber: string;
  referencePurchaseId?: string | null;
  supplierId: string;
  items: ReturnItemCommandInput[];
  reason: string;
  adminUserId: string;
}

export interface RecordStockCorrectionCommand {
  correctionId: string;
  productId: string;
  quantityChange: number; // signed delta millie-units
  reason: CorrectionReason;
  note: string;
  adminUserId: string;
}

export interface ConvertOrderToSaleCommand {
  orderId: string;
  saleId: string;
  saleNumber: string;
  paidAmountCents: number;
  paymentMethod?: PaymentMethod | null;
  userId: string;
  saleDate: string;
}

export interface RecordExpenseCommand {
  expenseId: string;
  expenseName: string;
  amountCents: number;
  category: ExpenseCategory;
  paymentMethod: PaymentMethod;
  notes?: string | null;
  userId: string;
  expenseDate: string;
}

// --- BUILD 05: Products + Inventory Commands & Results ---

export interface CreateProductCommand {
  productId?: string | null;
  businessId: string;
  name: string;
  productType: ProductType;
  unit: string;
  barcode?: string | null;
  costPriceCents: number;
  sellingPriceCents: number;
  initialStock: number; // millie-units (scale 1000)
  minStockLevel: number; // millie-units (scale 1000)
}

export interface UpdateProductCommand {
  productId: string;
  businessId: string;
  name?: string | null;
  unit?: string | null;
  barcode?: string | null;
  costPriceCents?: number | null;
  sellingPriceCents?: number | null;
  minStockLevel?: number | null;
}

export interface BatchCreateProductCommand {
  businessId: string;
  products: CreateProductCommand[];
}

export interface RemapBarcodeCommand {
  businessId: string;
  barcode: string;
  newProductId: string;
}

export interface ProductResult {
  id: string;
  businessId: string;
  name: string;
  productType: ProductType;
  unit: string;
  costPriceCents: number;
  sellingPriceCents: number;
  minStockLevel: number;
  isActive: number;
  createdAt: string;
  updatedAt: string;
}

export interface InventoryResult {
  productId: string;
  currentQuantity: number;
  minStockLevel: number;
  stockStatus: StockStatus;
  lastUpdatedAt: string;
}

export interface ProductStockStatusResult {
  productId: string;
  productName: string;
  currentQuantity: number;
  minStockLevel: number;
  stockStatus: StockStatus;
}

export interface WeighingScaleMetadata {
  rawPayload: string;
  embeddedWeightMillieUnits?: number | null;
  embeddedPriceCents?: number | null;
}

export interface BarcodeResolutionResult {
  barcode: string;
  barcodeType: BarcodeType;
  productId: string;
  productName: string;
  productType: ProductType;
  unit: string;
  costPriceCents: number;
  sellingPriceCents: number;
  currentQuantity: number;
  weighingMetadata?: WeighingScaleMetadata | null;
}

// --- BUILD 06: Purchase IPC Contracts ---

export interface SupplierSummary {
  id: string;
  name: string;
  phone: string | null;
  address: string | null;
  currentOutstandingCents: number;
}

export interface CreateSupplierForPurchaseInput {
  name: string;
  phone?: string | null;
}

export interface ProductForPurchase {
  id: string;
  name: string;
  productType: ProductType;
  unit: string;
  costPriceCents: number;
  sellingPriceCents: number;
  currentQuantity: number;
}

export interface PurchaseFormData {
  suppliers: SupplierSummary[];
  products: ProductForPurchase[];
}

export interface PurchaseItemIpcInput {
  productId: string;
  quantity: number; // millie-units (scale 1000)
  unitCostCents: number; // actual buying price in cents
}

export interface PreparePurchaseIpcInput {
  supplierId: string;
  items: PurchaseItemIpcInput[];
  paidAmountCents: number;
  paymentMethod?: PaymentMethod | null;
  purchaseDate?: string | null;
}

export interface PreparedPurchaseItemQuote {
  productId: string;
  productName: string;
  unit: string;
  quantity: number; // millie-units
  unitCostCents: number;
  lineTotalCents: number;
}

export interface PreparedPurchaseQuote {
  preparationToken: string; // opaque server-generated token
  purchaseId: string;
  purchaseNumber: string;
  supplierId: string;
  supplierName: string;
  items: PreparedPurchaseItemQuote[];
  totalAmountCents: number;
  paidAmountCents: number;
  creditAmountCents: number;
  paymentStatus: "PAID" | "PARTIAL" | "CREDIT";
  paymentMethod: PaymentMethod | null;
  purchaseDate: string;
  preparedAt: string;
}

export interface ConfirmPurchaseIpcInput {
  preparationToken: string;
}

export interface PurchaseReceiptItem {
  productId: string;
  productName: string;
  unit: string;
  quantity: number;
  unitCostCents: number;
  totalCents: number;
}

export interface PurchaseReceipt {
  purchaseId: string;
  purchaseNumber: string;
  supplierId: string;
  supplierName: string;
  supplierPhone: string | null;
  items: PurchaseReceiptItem[];
  totalAmountCents: number;
  paidAmountCents: number;
  creditAmountCents: number;
  paymentStatus: "PAID" | "PARTIAL" | "CREDIT";
  paymentMethod: PaymentMethod | null;
  purchaseDate: string;
  createdAt: string;
}

// --- BUILD 07: Sales / POS IPC Contracts ---

export interface ProductForSale {
  id: string;
  name: string;
  productType: ProductType;
  unit: string;
  costPriceCents: number;
  sellingPriceCents: number;
  currentQuantity: number; // millie-units (scale 1000)
}

export interface CustomerSummary {
  id: string;
  name: string;
  phone: string | null;
  currentBalanceCents: number;
}

export interface SalesFormData {
  customers: CustomerSummary[];
  products: ProductForSale[];
}

export interface CreateCustomerForSaleInput {
  name: string;
  phone?: string | null;
}

export interface SaleItemIpcInput {
  productId: string;
  quantity: number; // millie-units (scale 1000)
}

export interface PrepareSaleIpcInput {
  customerId?: string | null;
  items: SaleItemIpcInput[];
  settlementMode: "PAID" | "CREDIT";
  paymentMethod?: PaymentMethod | null;
}

export interface PreparedSaleItemQuote {
  productId: string;
  productName: string;
  unit: string;
  quantity: number;
  unitPriceCents: number;
  lineTotalCents: number;
}

export interface PreparedSaleQuote {
  preparationToken: string;
  saleId: string;
  saleNumber: string;
  customerId: string | null;
  customerName: string | null;
  items: PreparedSaleItemQuote[];
  totalAmountCents: number;
  paidAmountCents: number;
  creditAmountCents: number;
  settlementMode: "PAID" | "CREDIT";
  paymentMethod: PaymentMethod | null;
  preparedAt: string;
}

export interface ConfirmSaleIpcInput {
  preparationToken: string;
}

export interface SaleReceiptItem {
  productId: string;
  productName: string;
  unit: string;
  quantity: number;
  unitPriceCents: number;
  totalCents: number;
}

export interface SaleReceipt {
  saleId: string;
  saleNumber: string;
  customerId: string | null;
  customerName: string | null;
  customerPhone: string | null;
  items: SaleReceiptItem[];
  totalAmountCents: number;
  paidAmountCents: number;
  creditAmountCents: number;
  settlementMode: "PAID" | "CREDIT";
  paymentMethod: PaymentMethod | null;
  saleDate: string;
  createdAt: string;
}

// --- BUILD 08: Customer Credits & Payments IPC Contracts ---

export interface CustomerCreditItem {
  id: string;
  name: string;
  phone: string | null;
  address: string | null;
  currentCreditCents: number;
  isActive: number;
  createdAt: string;
  updatedAt: string;
}

export interface CustomerCreditsSummary {
  customers: CustomerCreditItem[];
  totalOutstandingCents: number;
}

export interface CustomerLedgerItem {
  id: string;
  customerId: string;
  entryType: CustomerLedgerEntryType;
  amountCents: number;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  referenceType: "SALE" | "PAYMENT" | "RETURN";
  referenceId: string;
  notes: string | null;
  userId: string;
  userName?: string;
  createdAt: string;
}

export interface CustomerLedgerHistory {
  customer: CustomerCreditItem;
  entries: CustomerLedgerItem[];
}

export interface PrepareCustomerPaymentIpcInput {
  customerId: string;
  amountCents: number;
  paymentMethod: PaymentMethod;
  notes?: string | null;
}

export interface PreparedCustomerPaymentQuote {
  preparationToken: string;
  paymentId: string;
  customerId: string;
  customerName: string;
  customerPhone: string | null;
  amountCents: number;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  paymentMethod: PaymentMethod;
  notes: string | null;
  preparedAt: string;
}

export interface ConfirmCustomerPaymentIpcInput {
  preparationToken: string;
}

export interface CustomerPaymentReceipt {
  paymentId: string;
  customerId: string;
  customerName: string;
  customerPhone: string | null;
  amountCents: number;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  paymentMethod: PaymentMethod;
  notes: string | null;
  paymentDate: string;
  createdAt: string;
}

// --- BUILD 09: Customer Orders IPC Contracts ---

export interface CustomerOrderItemInput {
  productId: string;
  quantity: number; // millie-units (scale 1000)
  notes?: string | null;
}

export interface CreateCustomerOrderIpcInput {
  customerId?: string | null;
  items: CustomerOrderItemInput[];
  notes?: string | null;
}

export interface UpdateCustomerOrderIpcInput {
  orderId: string;
  customerId?: string | null;
  items: CustomerOrderItemInput[];
  notes?: string | null;
}

export interface CustomerOrderItemDetail {
  id: string;
  orderId: string;
  productId: string;
  productName: string;
  productType: ProductType;
  unit: string;
  quantity: number; // millie-units
  unitPriceCents: number; // paise/cents
  lineTotalCents: number; // paise/cents
  availableStock: number; // informational current stock
  notes: string | null;
}

export interface CustomerOrderDetail {
  id: string;
  orderNumber: string;
  customerId: string | null;
  customerName: string | null;
  customerPhone: string | null;
  status: OrderStatus;
  totalAmountCents: number;
  convertedSaleId: string | null;
  convertedSaleNumber?: string | null;
  notes: string | null;
  userId: string;
  userName?: string;
  items: CustomerOrderItemDetail[];
  createdAt: string;
  updatedAt: string;
}

export interface CustomerOrderSummaryItem {
  id: string;
  orderNumber: string;
  customerId: string | null;
  customerName: string | null;
  customerPhone: string | null;
  status: OrderStatus;
  itemCount: number;
  totalAmountCents: number;
  convertedSaleId: string | null;
  notes: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface CustomerOrdersFormData {
  customers: CustomerSummary[];
  products: ProductForSale[];
}

export interface PrepareOrderConversionIpcInput {
  orderId: string;
  settlementMode: "PAID" | "CREDIT";
  paymentMethod?: PaymentMethod | null;
}

export interface PreparedOrderConversionQuote {
  preparationToken: string;
  orderId: string;
  orderNumber: string;
  saleId: string;
  saleNumber: string;
  customerId: string | null;
  customerName: string | null;
  customerPhone: string | null;
  items: PreparedSaleItemQuote[];
  totalAmountCents: number;
  paidAmountCents: number;
  creditAmountCents: number;
  settlementMode: "PAID" | "CREDIT";
  paymentMethod: PaymentMethod | null;
  preparedAt: string;
}

export interface ConfirmOrderConversionIpcInput {
  preparationToken: string;
}

// --- BUILD 10: Returns & Stock Reversal IPC Contracts ---

export interface ReturnItemInput {
  productId: string;
  quantity: number; // millie-units (scale 1000)
}

export interface PrepareCustomerReturnIpcInput {
  customerId?: string | null;
  referenceSaleId?: string | null;
  items: ReturnItemInput[];
  reason: string;
  refundPaymentMethod?: PaymentMethod | null;
}

export interface PreparedCustomerReturnItemQuote {
  productId: string;
  productName: string;
  productType: ProductType;
  unit: string;
  quantity: number;
  unitPriceCents: number; // Selling price
  lineTotalCents: number;
}

export interface PreparedCustomerReturnQuote {
  preparationToken: string;
  returnId: string;
  returnNumber: string;
  customerId: string | null;
  customerName: string | null;
  customerPhone: string | null;
  referenceSaleId: string | null;
  items: PreparedCustomerReturnItemQuote[];
  totalAmountCents: number;
  debtReductionCents: number;
  refundAmountCents: number;
  refundPaymentMethod: PaymentMethod | null;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  reason: string;
  preparedAt: string;
}

export interface ConfirmCustomerReturnIpcInput {
  preparationToken: string;
}

export interface CustomerReturnReceipt {
  returnId: string;
  returnNumber: string;
  customerId: string | null;
  customerName: string | null;
  items: PreparedCustomerReturnItemQuote[];
  totalAmountCents: number;
  debtReductionCents: number;
  refundAmountCents: number;
  refundPaymentMethod: PaymentMethod | null;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  reason: string;
  createdAt: string;
}

export interface PrepareSupplierReturnIpcInput {
  supplierId?: string | null;
  referencePurchaseId?: string | null;
  items: ReturnItemInput[];
  reason: string;
}

export interface PreparedSupplierReturnItemQuote {
  productId: string;
  productName: string;
  productType: ProductType;
  unit: string;
  quantity: number;
  unitCostCents: number; // Cost price
  lineTotalCents: number;
  availableStock: number;
}

export interface PreparedSupplierReturnQuote {
  preparationToken: string;
  returnId: string;
  returnNumber: string;
  supplierId: string | null;
  supplierName: string | null;
  referencePurchaseId: string | null;
  items: PreparedSupplierReturnItemQuote[];
  totalAmountCents: number;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  reason: string;
  preparedAt: string;
}

export interface ConfirmSupplierReturnIpcInput {
  preparationToken: string;
}

export interface SupplierReturnReceipt {
  returnId: string;
  returnNumber: string;
  supplierId: string | null;
  supplierName: string | null;
  items: PreparedSupplierReturnItemQuote[];
  totalAmountCents: number;
  balanceBeforeCents: number;
  balanceAfterCents: number;
  reason: string;
  createdAt: string;
}

export interface ReturnSummaryItem {
  id: string;
  returnNumber: string;
  returnType: ReturnType;
  counterpartyName: string | null;
  itemCount: number;
  totalAmountCents: number;
  reason: string;
  createdAt: string;
}

export interface ReturnItemDetail {
  productId: string;
  productName: string;
  productType: ProductType;
  unit: string;
  quantity: number;
  unitPriceCents: number;
  totalCents: number;
}

export interface ReturnDetail {
  id: string;
  returnNumber: string;
  returnType: ReturnType;
  referenceId: string | null;
  customerId: string | null;
  customerName: string | null;
  supplierId: string | null;
  supplierName: string | null;
  totalAmountCents: number;
  reason: string;
  adminUserId: string;
  adminUserName?: string;
  items: ReturnItemDetail[];
  createdAt: string;
}

export interface ReturnsFormData {
  customers: CustomerSummary[];
  suppliers: SupplierSummary[];
  products: ProductForSale[];
}

// --- BUILD 11: Stock Corrections & Physical Inventory Adjustment IPC Contracts ---

export interface ProductForCorrection {
  id: string;
  name: string;
  productType: ProductType;
  unit: string;
  currentQuantity: number; // millie-units (scale 1000)
  costPriceCents: number;
  sellingPriceCents: number;
}

export interface StockCorrectionsFormData {
  products: ProductForCorrection[];
}

export interface PrepareStockCorrectionIpcInput {
  productId: string;
  quantityChange: number; // signed delta in millie-units (scale 1000)
  reason: CorrectionReason; // "DAMAGED" | "EXPIRED" | "LOST" | "MISCOUNT"
  note: string;
}

export interface PreparedStockCorrectionQuote {
  preparationToken: string;
  correctionId: string;
  productId: string;
  productName: string;
  productType: ProductType;
  unit: string;
  quantityChange: number; // signed delta in millie-units
  quantityBefore: number; // millie-units
  quantityAfter: number; // millie-units
  reason: CorrectionReason;
  note: string;
  adminUserId: string;
  adminUsername: string;
  preparedAt: string;
}

export interface ConfirmStockCorrectionIpcInput {
  preparationToken: string;
}

export interface StockCorrectionReceipt {
  correctionId: string;
  productId: string;
  productName: string;
  productType: ProductType;
  unit: string;
  quantityChange: number;
  quantityBefore: number;
  quantityAfter: number;
  reason: CorrectionReason;
  note: string;
  adminUserId: string;
  adminUsername: string;
  createdAt: string;
}

export interface StockCorrectionHistoryItem {
  id: string;
  productId: string;
  productName: string;
  unit: string;
  quantityChange: number;
  quantityBefore: number;
  quantityAfter: number;
  reason: CorrectionReason;
  note: string;
  adminUserId: string;
  adminUsername: string;
  createdAt: string;
}

export interface StockCorrectionsSummary {
  corrections: StockCorrectionHistoryItem[];
  totalCorrectionsCount: number;
}

// ==============================================================================================
// BUILD 12: OFFLINE SYNC & BACKUP CONTRACTS
// ==============================================================================================

export type BackupType = "MANUAL" | "AUTO";

export interface BackupManifest {
  backupFormatVersion: number;
  appVersion: string;
  createdAt: string;
  backupType: BackupType;
  businessId: string;
  businessName: string;
  checksumSha256: string;
  tableCounts: Record<string, number>;
  totalRecords: number;
  payloadSizeBytes: number;
}

export interface BackupMetadata {
  id: string;
  backupType: BackupType;
  backupFormatVersion: number;
  appVersion: string;
  createdAt: string;
  businessId: string;
  businessName: string;
  checksumSha256: string;
  fileSizeBytes: number;
  payloadSizeBytes: number;
  tableCounts: Record<string, number>;
  totalRecords: number;
  fileName: string;
}

export interface BackupSettings {
  autoBackupEnabled: boolean;
  retentionLimit: number; // 7
  lastBackupAt: string | null;
  lastAutoBackupAt: string | null;
  isInternetAvailable: boolean;
  totalBackupsCount: number;
  autoBackupsCount: number;
  manualBackupsCount: number;
}

export interface BackupValidationResult {
  isValid: boolean;
  manifest: BackupManifest | null;
  integrityCheckPassed: boolean;
  foreignKeyCheckPassed: boolean;
  schemaTablesPassed: boolean;
  businessInvariantsPassed: boolean;
  tablesFound: string[];
  compatibilityStatus: "COMPATIBLE" | "INCOMPATIBLE" | "CORRUPTED";
  errors: string[];
}

export interface CreateBackupResult {
  success: boolean;
  metadata: BackupMetadata;
  removedOldestAutoBackupFileName?: string | null;
}

export interface RestoreBackupResult {
  success: boolean;
  backupId: string;
  restoredAt: string;
  tableCounts: Record<string, number>;
  totalRecords: number;
  verificationPassed: boolean;
  rolledBack: boolean;
  message: string;
}

export interface ToggleAutoBackupInput {
  enabled: boolean;
}

export interface CreateManualBackupInput {
  note?: string;
}

export interface TriggerAutoBackupInput {
  isInternetAvailable?: boolean;
}

export interface RestoreBackupInput {
  fileName: string;
}

export interface ValidateBackupInput {
  fileName: string;
}

export interface BackupStatusDto {
  settings: BackupSettings;
  backups: BackupMetadata[];
}

// ==============================================================================================
// BUILD 13: AI & VOICE INTELLIGENCE LAYER CONTRACTS
// ==============================================================================================

export type AIIntentType =
  | "CHECK_STOCK"
  | "CREATE_SALE"
  | "CREATE_PURCHASE"
  | "CREATE_CUSTOMER_ORDER"
  | "CHECK_CUSTOMER_CREDIT"
  | "RECORD_CUSTOMER_PAYMENT"
  | "CHECK_SUPPLIER_BALANCE"
  | "CHECK_SALES"
  | "CHECK_DEMAND"
  | "CHECK_LOW_STOCK"
  | "PREPARE_REORDER"
  | "TRANSLATE"
  | "GENERAL_QUERY";

export type AIResponseMode =
  | "INFORMATIONAL"
  | "RECOMMENDATION"
  | "PREPARED_ACTION"
  | "AMBIGUOUS"
  | "ERROR";

export interface SaleIntentItem {
  productReference: string;
  productId?: string | null;
  productName?: string | null;
  quantityDisplay: string;
  quantityMillie: number;
  unitPriceCents?: number | null;
}

export interface PurchaseIntentItem {
  productReference: string;
  productId?: string | null;
  productName?: string | null;
  quantityDisplay: string;
  quantityMillie: number;
  unitCostCents?: number | null;
}

export interface OrderIntentItem {
  productReference: string;
  productId?: string | null;
  productName?: string | null;
  quantityDisplay: string;
  quantityMillie: number;
}

export interface CreateSaleIntent {
  intentType: "CREATE_SALE";
  items: SaleIntentItem[];
  customerReference?: string | null;
  customerId?: string | null;
  paymentMethod: "CASH" | "UPI" | "CARD" | "CREDIT";
  settlementMode: "PAID" | "CREDIT";
}

export interface CreatePurchaseIntent {
  intentType: "CREATE_PURCHASE";
  items: PurchaseIntentItem[];
  supplierReference: string;
  supplierId?: string | null;
  paymentMethod: "CASH" | "UPI" | "BANK_TRANSFER" | "CREDIT";
}

export interface CreateCustomerOrderIntent {
  intentType: "CREATE_CUSTOMER_ORDER";
  items: OrderIntentItem[];
  customerReference?: string | null;
  customerId?: string | null;
  notes?: string | null;
}

export interface CheckStockIntent {
  intentType: "CHECK_STOCK";
  productReference: string;
  productId?: string | null;
}

export interface CheckCustomerCreditIntent {
  intentType: "CHECK_CUSTOMER_CREDIT";
  customerReference: string;
  customerId?: string | null;
}

export interface RecordCustomerPaymentIntent {
  intentType: "RECORD_CUSTOMER_PAYMENT";
  customerReference: string;
  customerId?: string | null;
  amountCents: number;
  paymentMethod: "CASH" | "UPI" | "CARD" | "BANK_TRANSFER";
}

export interface CheckSupplierBalanceIntent {
  intentType: "CHECK_SUPPLIER_BALANCE";
  supplierReference: string;
  supplierId?: string | null;
}

export interface CheckSalesIntent {
  intentType: "CHECK_SALES";
  period?: "TODAY" | "THIS_WEEK" | "THIS_MONTH" | null;
}

export interface CheckDemandIntent {
  intentType: "CHECK_DEMAND";
  categoryReference?: string | null;
}

export interface CheckLowStockIntent {
  intentType: "CHECK_LOW_STOCK";
}

export interface PrepareReorderIntent {
  intentType: "PREPARE_REORDER";
  productReference?: string | null;
  productId?: string | null;
}

export interface TranslateIntent {
  intentType: "TRANSLATE";
  text: string;
  targetLanguage: string;
}

export interface GeneralQueryIntent {
  intentType: "GENERAL_QUERY";
  query: string;
}

export type StructuredIntent =
  | CreateSaleIntent
  | CreatePurchaseIntent
  | CreateCustomerOrderIntent
  | CheckStockIntent
  | CheckCustomerCreditIntent
  | RecordCustomerPaymentIntent
  | CheckSupplierBalanceIntent
  | CheckSalesIntent
  | CheckDemandIntent
  | CheckLowStockIntent
  | PrepareReorderIntent
  | TranslateIntent
  | GeneralQueryIntent;

export interface DemandRecommendationDto {
  productId: string;
  productName: string;
  unit: string;
  currentStockMillie: number;
  minStockLevelMillie: number;
  recentSalesMillie: number;
  estimatedDaysToStockout: number | null;
  recommendedReorderMillie: number;
  suggestedSupplierId: string | null;
  suggestedSupplierName: string | null;
  reason: string;
  confidence: number; // 0.0 to 1.0
}

export interface AIResponseDto {
  mode: AIResponseMode;
  intentType: AIIntentType;
  explanation: string;
  preparedAction: StructuredIntent | null;
  recommendations: DemandRecommendationDto[];
  ambiguityOptions: string[];
  offlineMode: boolean;
  providerName: string;
}

export interface AIQueryInput {
  query: string;
  language?: string | null;
  isVoiceTranscript?: boolean;
}

export interface AIStatusDto {
  providerName: string;
  isAvailable: boolean;
  isOfflineCapable: boolean;
  voiceTranscriberAvailable: boolean;
  supportedLanguages: string[];
}



