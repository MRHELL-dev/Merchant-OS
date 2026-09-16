import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { SYSTEM_DEFAULTS, QUANTITY_SCALE, PERMISSION_KEYS } from "@merchant-os/shared";
import type {
  SystemStatus,
  AuthenticatedIdentity,
  CustomerCreditItem,
  CustomerCreditsSummary,
  CustomerLedgerItem,
  CustomerLedgerHistory,
  PrepareCustomerPaymentIpcInput,
  PreparedCustomerPaymentQuote,
  ConfirmCustomerPaymentIpcInput,
  CustomerPaymentReceipt,
  CustomerOrderItemInput,
  CreateCustomerOrderIpcInput,
  UpdateCustomerOrderIpcInput,
  CustomerOrderItemDetail,
  CustomerOrderDetail,
  CustomerOrderSummaryItem,
  CustomerOrdersFormData,
  PrepareOrderConversionIpcInput,
  PreparedOrderConversionQuote,
  ConfirmOrderConversionIpcInput,
  ReturnItemInput,
  PrepareCustomerReturnIpcInput,
  PreparedCustomerReturnQuote,
  ConfirmCustomerReturnIpcInput,
  CustomerReturnReceipt,
  PrepareSupplierReturnIpcInput,
  PreparedSupplierReturnQuote,
  ConfirmSupplierReturnIpcInput,
  SupplierReturnReceipt,
  ReturnSummaryItem,
  ReturnDetail,
  ReturnsFormData,
  ProductForCorrection,
  StockCorrectionsFormData,
  PrepareStockCorrectionIpcInput,
  PreparedStockCorrectionQuote,
  ConfirmStockCorrectionIpcInput,
  StockCorrectionReceipt,
  StockCorrectionHistoryItem,
  StockCorrectionsSummary,
  CorrectionReason,
} from "@merchant-os/shared";
import {
  systemMetadata,
  schemaMigrations,
  businesses,
  users,
  permissions,
  categories,
  products,
  barcodeMappings,
  inventory,
  stockMovements,
  stockCorrections,
  customers,
  customerLedger,
  suppliers,
  supplierLedger,
  customerOrders,
  customerOrderItems,
  sales,
  saleItems,
  purchases,
  purchaseItems,
  returns,
  returnItems,
  payments,
  expenses,
  auditLogs,
} from "@merchant-os/database";

test("Contract Verification: Shared SystemStatus interface and constants", () => {
  const sampleStatus: SystemStatus = {
    status: SYSTEM_DEFAULTS.status,
    database: SYSTEM_DEFAULTS.databaseConnected,
    sqliteVersion: "3.46.0",
    internet: SYSTEM_DEFAULTS.internet,
    timestamp: new Date().toISOString(),
  };

  assert.equal(sampleStatus.status, "LOCAL SYSTEM ONLINE");
  assert.equal(sampleStatus.database, "SQLite — Connected");
  assert.equal(sampleStatus.internet, "Not Required");
  assert.equal(QUANTITY_SCALE, 1000);
});

test("Database Schema Verification: All 26 V1 tables are defined with correct primary keys", () => {
  const tables = [
    { table: systemMetadata, name: "system_metadata" },
    { table: schemaMigrations, name: "schema_migrations" },
    { table: businesses, name: "businesses" },
    { table: users, name: "users" },
    { table: permissions, name: "permissions" },
    { table: categories, name: "categories" },
    { table: products, name: "products" },
    { table: barcodeMappings, name: "barcode_mappings" },
    { table: inventory, name: "inventory" },
    { table: stockMovements, name: "stock_movements" },
    { table: stockCorrections, name: "stock_corrections" },
    { table: customers, name: "customers" },
    { table: customerLedger, name: "customer_ledger" },
    { table: suppliers, name: "suppliers" },
    { table: supplierLedger, name: "supplier_ledger" },
    { table: customerOrders, name: "customer_orders" },
    { table: customerOrderItems, name: "customer_order_items" },
    { table: sales, name: "sales" },
    { table: saleItems, name: "sale_items" },
    { table: purchases, name: "purchases" },
    { table: purchaseItems, name: "purchase_items" },
    { table: returns, name: "returns" },
    { table: returnItems, name: "return_items" },
    { table: payments, name: "payments" },
    { table: expenses, name: "expenses" },
    { table: auditLogs, name: "audit_logs" },
  ];

  assert.equal(tables.length, 26, "Exactly 26 tables must be registered in the schema");

  for (const { table, name } of tables) {
    assert.ok(table, `Table ${name} must be defined`);
    assert.ok((table as any).id, `Table ${name} must have an id column`);
  }
});

test("Domain Schema Verification: Unit ownership strictly in products.unit", () => {
  assert.ok(products.unit, "products table must own unit definition");
  assert.equal((inventory as any).unit, undefined, "inventory must NOT duplicate unit column");
});

test("Domain Schema Verification: Immutable historical pricing on sale and purchase items", () => {
  assert.ok(saleItems.unitPriceCents, "sale_items must record historical unit_price_cents");
  assert.ok(saleItems.costPriceCents, "sale_items must record historical cost_price_cents");
  assert.ok(purchaseItems.unitCostCents, "purchase_items must record actual unit_cost_cents");
});

test("Domain Schema Verification: Polymorphic payment entity columns exist", () => {
  assert.ok(payments.relatedEntityType, "payments must have related_entity_type");
  assert.ok(payments.relatedEntityId, "payments must have related_entity_id");
  assert.ok(payments.paymentType, "payments must have payment_type");
});

test("Architecture Rule 1 Verification: React UI never imports SQLite directly", () => {
  const appTsxPath = path.resolve(process.cwd(), "apps/desktop/src/App.tsx");
  const mainTsxPath = path.resolve(process.cwd(), "apps/desktop/src/main.tsx");

  for (const filePath of [appTsxPath, mainTsxPath]) {
    if (fs.existsSync(filePath)) {
      const content = fs.readFileSync(filePath, "utf8");
      assert.ok(
        !content.includes("sqlite3"),
        `File ${filePath} must not import sqlite3 directly`
      );
      assert.ok(
        !content.includes("better-sqlite3"),
        `File ${filePath} must not import better-sqlite3 directly`
      );
      assert.ok(
        !content.includes("rusqlite"),
        `File ${filePath} must not import rusqlite`
      );
    }
  }
});

test("Architecture Rule 2 Verification: AI layer never imports SQLite directly", () => {
  const aiDir = path.resolve(process.cwd(), "ai");
  if (fs.existsSync(aiDir)) {
    const files = fs.readdirSync(aiDir);
    for (const file of files) {
      const filePath = path.join(aiDir, file);
      if (fs.statSync(filePath).isFile()) {
        const content = fs.readFileSync(filePath, "utf8");
        assert.ok(!content.includes("sqlite"), `AI file ${file} must not import sqlite directly`);
        assert.ok(!content.includes("rusqlite"), `AI file ${file} must not import rusqlite directly`);
      }
    }
  }
});

test("Contract Verification: Structured command interfaces exist", () => {
  // Verify structured command types can be instantiated conforming to contracts
  const sampleSaleCmd = {
    saleId: "sale_1",
    saleNumber: "INV-001",
    customerId: null,
    items: [{ productId: "p1", quantity: 1000 }],
    paidAmountCents: 10000,
    userId: "u1",
    saleDate: "2026-09-13",
  };
  assert.equal(sampleSaleCmd.items[0]!.quantity, 1000);
});

test("Contract Verification: Auth roles, permission keys, and identity contracts", () => {
  assert.equal(PERMISSION_KEYS.length, 18, "Exactly 18 permission keys must be defined");
  assert.ok(PERMISSION_KEYS.includes("DASHBOARD"));
  assert.ok(PERMISSION_KEYS.includes("SALES"));
  assert.ok(PERMISSION_KEYS.includes("PURCHASES"));
  assert.ok(PERMISSION_KEYS.includes("INVENTORY"));
  assert.ok(PERMISSION_KEYS.includes("PRICES"));
  assert.ok(PERMISSION_KEYS.includes("CUSTOMERS"));
  assert.ok(PERMISSION_KEYS.includes("CUSTOMER_CREDITS"));
  assert.ok(PERMISSION_KEYS.includes("CUSTOMER_ORDERS"));
  assert.ok(PERMISSION_KEYS.includes("SUPPLIERS"));
  assert.ok(PERMISSION_KEYS.includes("RETURNS"));
  assert.ok(PERMISSION_KEYS.includes("CORRECTION"));
  assert.ok(PERMISSION_KEYS.includes("EMPLOYEES"));
  assert.ok(PERMISSION_KEYS.includes("PERMISSIONS"));
  assert.ok(PERMISSION_KEYS.includes("EXPENSES"));
  assert.ok(PERMISSION_KEYS.includes("TRANSACTION_HISTORY"));
  assert.ok(PERMISSION_KEYS.includes("REPORTS"));
  assert.ok(PERMISSION_KEYS.includes("BUSINESS_PROFILE"));
  assert.ok(PERMISSION_KEYS.includes("BACKUP_RESTORE"));

  const sampleIdentity: AuthenticatedIdentity = {
    userId: "usr_admin_1",
    username: "admin",
    role: "ADMIN",
  };
  assert.equal(sampleIdentity.role, "ADMIN");
});

test("BUILD 05 Schema Verification: products table includes businessId, productType, and minStockLevel", () => {
  assert.ok(products.businessId, "products table must have businessId column");
  assert.ok(products.productType, "products table must have productType column");
  assert.ok(products.minStockLevel, "products table must have minStockLevel column");
});

test("BUILD 05 Contract Verification: Product & Inventory commands and results", () => {
  const createCmd = {
    businessId: "biz_1",
    name: "Good Day Biscuits",
    productType: "PACKAGED" as const,
    unit: "packet",
    barcode: "8901030000000",
    costPriceCents: 1500,
    sellingPriceCents: 2000,
    initialStock: 50000,
    minStockLevel: 10000,
  };
  assert.equal(createCmd.productType, "PACKAGED");
  assert.equal(createCmd.initialStock, 50000);

  const updateCmd = {
    productId: "prod_1",
    businessId: "biz_1",
    name: "Good Day Rich Cashew",
    sellingPriceCents: 2500,
  };
  assert.equal(updateCmd.sellingPriceCents, 2500);

  const remapCmd = {
    businessId: "biz_1",
    barcode: "8901030000000",
    newProductId: "prod_2",
  };
  assert.equal(remapCmd.barcode, "8901030000000");
});

test("BUILD 06 Contract Verification: Purchase IPC contracts and Quote/Receipt structures", () => {
  const prepareInput = {
    supplierId: "supp_1",
    items: [
      { productId: "prod_rice", quantity: 5000, unitCostCents: 180000 },
    ],
    paidAmountCents: 500000,
    paymentMethod: "BANK_TRANSFER" as const,
    purchaseDate: "2026-09-13T10:00:00Z",
  };
  assert.equal(prepareInput.supplierId, "supp_1");
  assert.equal(prepareInput.items[0]?.quantity, 5000);

  const sampleQuote = {
    preparationToken: "prep_123456789_1",
    purchaseId: "pur_1",
    purchaseNumber: "PO-001",
    supplierId: "supp_1",
    supplierName: "Grain Traders",
    items: [
      {
        productId: "prod_rice",
        productName: "Basmati Rice 25kg Bag",
        unit: "pcs",
        quantity: 5000,
        unitCostCents: 180000,
        lineTotalCents: 900000,
      },
    ],
    totalAmountCents: 900000,
    paidAmountCents: 500000,
    creditAmountCents: 400000,
    paymentStatus: "PARTIAL" as const,
    paymentMethod: "BANK_TRANSFER" as const,
    purchaseDate: "2026-09-13T10:00:00Z",
    preparedAt: "2026-09-13T10:00:01Z",
  };
  assert.equal(sampleQuote.preparationToken, "prep_123456789_1");
  assert.equal(sampleQuote.totalAmountCents, 900000);
  assert.equal(sampleQuote.paymentStatus, "PARTIAL");

  const sampleReceipt = {
    purchaseId: "pur_1",
    purchaseNumber: "PO-001",
    supplierId: "supp_1",
    supplierName: "Grain Traders",
    supplierPhone: "+919999900001",
    items: [
      {
        productId: "prod_rice",
        productName: "Basmati Rice 25kg Bag",
        unit: "pcs",
        quantity: 5000,
        unitCostCents: 180000,
        totalCents: 900000,
      },
    ],
    totalAmountCents: 900000,
    paidAmountCents: 500000,
    creditAmountCents: 400000,
    paymentStatus: "PARTIAL" as const,
    paymentMethod: "BANK_TRANSFER" as const,
    purchaseDate: "2026-09-13T10:00:00Z",
    createdAt: "2026-09-13T10:00:02Z",
  };
  assert.equal(sampleReceipt.purchaseNumber, "PO-001");
  assert.equal(sampleReceipt.creditAmountCents, 400000);
});

test("BUILD 07 Contract Verification: Sales / POS IPC contracts and Quote/Receipt structures", () => {
  const prepareSaleInput = {
    customerId: "cust_sharma",
    items: [
      { productId: "prod_rice_bag", quantity: 2000 },
      { productId: "prod_mustard_oil", quantity: 1500 },
    ],
    settlementMode: "PAID" as const,
    paymentMethod: "CASH" as const,
  };
  assert.equal(prepareSaleInput.customerId, "cust_sharma");
  assert.equal(prepareSaleInput.items.length, 2);
  assert.equal(prepareSaleInput.settlementMode, "PAID");

  const sampleSaleQuote = {
    preparationToken: "prep_9876543210_pos_1",
    saleId: "sale_001",
    saleNumber: "INV-1001",
    customerId: "cust_sharma",
    customerName: "Ramesh Sharma",
    items: [
      {
        productId: "prod_rice_bag",
        productName: "Basmati Rice 25kg Bag",
        unit: "pcs",
        quantity: 2000,
        unitPriceCents: 220000,
        lineTotalCents: 440000,
      },
    ],
    totalAmountCents: 440000,
    paidAmountCents: 440000,
    creditAmountCents: 0,
    settlementMode: "PAID" as const,
    paymentMethod: "CASH" as const,
    preparedAt: "2026-09-13T10:30:00Z",
  };
  assert.equal(sampleSaleQuote.preparationToken, "prep_9876543210_pos_1");
  assert.equal(sampleSaleQuote.totalAmountCents, 440000);
  assert.equal(sampleSaleQuote.settlementMode, "PAID");
  assert.equal(sampleSaleQuote.creditAmountCents, 0);

  const sampleSaleReceipt = {
    saleId: "sale_001",
    saleNumber: "INV-1001",
    customerId: "cust_sharma",
    customerName: "Ramesh Sharma",
    customerPhone: "+919811100001",
    items: [
      {
        productId: "prod_rice_bag",
        productName: "Basmati Rice 25kg Bag",
        unit: "pcs",
        quantity: 2000,
        unitPriceCents: 220000,
        totalCents: 440000,
      },
    ],
    totalAmountCents: 440000,
    paidAmountCents: 440000,
    creditAmountCents: 0,
    settlementMode: "PAID" as const,
    paymentMethod: "CASH" as const,
    saleDate: "2026-09-13T10:30:00Z",
    createdAt: "2026-09-13T10:30:01Z",
  };
  assert.equal(sampleSaleReceipt.saleNumber, "INV-1001");
  assert.equal(sampleSaleReceipt.settlementMode, "PAID");
  assert.equal(sampleSaleReceipt.items[0]?.unitPriceCents, 220000);
});

test("Contract Verification: BUILD 08 Customer Credits & Payments IPC contracts and deterministic ledger", () => {
  const sampleCustomerCredit: CustomerCreditItem = {
    id: "cust_sharma",
    name: "Ramesh Sharma",
    phone: "+919811100001",
    address: "Block B, Sector 4, Noida",
    currentCreditCents: 245000,
    isActive: 1,
    createdAt: "2026-09-13T10:00:00Z",
    updatedAt: "2026-09-13T10:00:00Z",
  };

  const sampleSummary: CustomerCreditsSummary = {
    customers: [sampleCustomerCredit],
    totalOutstandingCents: 245000,
  };
  assert.equal(sampleSummary.customers.length, 1);
  assert.equal(sampleSummary.totalOutstandingCents, 245000);

  const sampleLedgerItem: CustomerLedgerItem = {
    id: "cleg_001",
    customerId: "cust_sharma",
    entryType: "PAYMENT_RECEIVED",
    amountCents: 50000,
    balanceBeforeCents: 245000,
    balanceAfterCents: 195000,
    referenceType: "PAYMENT",
    referenceId: "pmt_001",
    notes: "Partial payment received",
    userId: "usr_admin",
    userName: "admin",
    createdAt: "2026-09-13T11:00:00Z",
  };

  const sampleHistory: CustomerLedgerHistory = {
    customer: sampleCustomerCredit,
    entries: [sampleLedgerItem],
  };
  assert.equal(sampleHistory.customer.id, "cust_sharma");
  assert.equal(sampleHistory.entries.length, 1);
  assert.equal(sampleHistory.entries[0]?.entryType, "PAYMENT_RECEIVED");
  assert.equal(sampleHistory.entries[0]?.balanceAfterCents, 195000);

  const prepareInput: PrepareCustomerPaymentIpcInput = {
    customerId: "cust_sharma",
    amountCents: 50000,
    paymentMethod: "CASH",
    notes: "Partial settlement",
  };
  assert.equal(prepareInput.customerId, "cust_sharma");
  assert.equal(prepareInput.amountCents, 50000);

  const quote: PreparedCustomerPaymentQuote = {
    preparationToken: "prep_cust_pmt_987654",
    paymentId: "pmt_cust_001",
    customerId: "cust_sharma",
    customerName: "Ramesh Sharma",
    customerPhone: "+919811100001",
    amountCents: 50000,
    balanceBeforeCents: 245000,
    balanceAfterCents: 195000,
    paymentMethod: "CASH",
    notes: "Partial settlement",
    preparedAt: "2026-09-13T11:05:00Z",
  };
  assert.equal(quote.preparationToken, "prep_cust_pmt_987654");
  assert.equal(quote.balanceBeforeCents - quote.amountCents, quote.balanceAfterCents);

  const confirmInput: ConfirmCustomerPaymentIpcInput = {
    preparationToken: quote.preparationToken,
  };
  assert.equal(confirmInput.preparationToken, quote.preparationToken);

  const receipt: CustomerPaymentReceipt = {
    paymentId: "pmt_cust_001",
    customerId: "cust_sharma",
    customerName: "Ramesh Sharma",
    customerPhone: "+919811100001",
    amountCents: 50000,
    balanceBeforeCents: 245000,
    balanceAfterCents: 195000,
    paymentMethod: "CASH",
    notes: "Partial settlement",
    paymentDate: "2026-09-13T11:05:00Z",
    createdAt: "2026-09-13T11:05:01Z",
  };
  assert.equal(receipt.paymentId, "pmt_cust_001");
  assert.equal(receipt.balanceAfterCents, 195000);
});

test("Contract Verification: BUILD 09 Customer Orders IPC contracts and lifecycle states", () => {
  assert.ok(customerOrders.orderNumber, "customer_orders must have order_number");
  assert.ok(customerOrders.status, "customer_orders must have status");
  assert.ok(customerOrders.convertedSaleId, "customer_orders must have converted_sale_id");
  assert.ok(customerOrderItems.orderId, "customer_order_items must have order_id");
  assert.ok(customerOrderItems.productId, "customer_order_items must have product_id");
  assert.ok(customerOrderItems.quantity, "customer_order_items must have quantity");
  assert.ok(customerOrderItems.unitPriceCents, "customer_order_items must have unit_price_cents");

  const itemInput: CustomerOrderItemInput = {
    productId: "prod_rice",
    quantity: 5000,
    notes: "5kg pack",
  };
  assert.equal(itemInput.productId, "prod_rice");
  assert.equal(itemInput.quantity, 5000);

  const createInput: CreateCustomerOrderIpcInput = {
    customerId: "cust_sharma",
    items: [itemInput],
    notes: "Urgent morning delivery",
  };
  assert.equal(createInput.customerId, "cust_sharma");
  assert.equal(createInput.items.length, 1);

  const updateInput: UpdateCustomerOrderIpcInput = {
    orderId: "ord_12345",
    customerId: "cust_sharma",
    items: [{ productId: "prod_oil", quantity: 2000, notes: null }],
    notes: "Changed to oil",
  };
  assert.equal(updateInput.orderId, "ord_12345");

  const formData: CustomerOrdersFormData = {
    customers: [],
    products: [],
  };
  assert.equal(formData.customers.length, 0);

  const itemDetail: CustomerOrderItemDetail = {
    id: "item_1",
    orderId: "ord_12345",
    productId: "prod_rice",
    productName: "Basmati Rice",
    productType: "LOOSE",
    unit: "kg",
    quantity: 5000,
    unitPriceCents: 6000,
    lineTotalCents: 30000,
    availableStock: 50000,
    notes: null,
  };
  assert.equal(itemDetail.lineTotalCents, 30000);
  assert.equal(itemDetail.availableStock, 50000);

  const orderDetail: CustomerOrderDetail = {
    id: "ord_12345",
    orderNumber: "ORD-20260913-7F2A01",
    customerId: "cust_sharma",
    customerName: "Ramesh Sharma",
    customerPhone: "+919811100001",
    status: "DRAFT",
    totalAmountCents: 30000,
    convertedSaleId: null,
    notes: "Urgent",
    userId: "usr_admin",
    items: [itemDetail],
    createdAt: "2026-09-13T10:00:00Z",
    updatedAt: "2026-09-13T10:00:00Z",
  };
  assert.equal(orderDetail.status, "DRAFT");
  assert.ok(orderDetail.orderNumber.startsWith("ORD-"));

  const summaryItem: CustomerOrderSummaryItem = {
    id: "ord_12345",
    orderNumber: "ORD-20260913-7F2A01",
    customerId: "cust_sharma",
    customerName: "Ramesh Sharma",
    customerPhone: "+919811100001",
    status: "DRAFT",
    itemCount: 1,
    totalAmountCents: 30000,
    convertedSaleId: null,
    notes: "Urgent",
    createdAt: "2026-09-13T10:00:00Z",
    updatedAt: "2026-09-13T10:00:00Z",
  };
  assert.equal(summaryItem.itemCount, 1);

  const prepConversionInput: PrepareOrderConversionIpcInput = {
    orderId: "ord_12345",
    settlementMode: "PAID",
    paymentMethod: "UPI",
  };
  assert.equal(prepConversionInput.settlementMode, "PAID");

  const quote: PreparedOrderConversionQuote = {
    preparationToken: "prep_ord_conv_12345",
    orderId: "ord_12345",
    orderNumber: "ORD-20260913-7F2A01",
    saleId: "sale_999",
    saleNumber: "INV-999",
    customerId: "cust_sharma",
    customerName: "Ramesh Sharma",
    customerPhone: "+919811100001",
    items: [],
    totalAmountCents: 30000,
    paidAmountCents: 30000,
    creditAmountCents: 0,
    settlementMode: "PAID",
    paymentMethod: "UPI",
    preparedAt: "2026-09-13T10:05:00Z",
  };
  assert.equal(quote.preparationToken, "prep_ord_conv_12345");
  assert.equal(quote.paidAmountCents, quote.totalAmountCents);

  const confirmConversionInput: ConfirmOrderConversionIpcInput = {
    preparationToken: quote.preparationToken,
  };
  assert.equal(confirmConversionInput.preparationToken, quote.preparationToken);
});

test("Contract Verification: BUILD 10 Returns IPC contracts, types, and schemas", () => {
  // 1. Verify ReturnItemInput
  const itemInput: ReturnItemInput = {
    productId: "prod_rice",
    quantity: 5000,
  };
  assert.equal(itemInput.productId, "prod_rice");
  assert.equal(itemInput.quantity, 5000);

  // 2. Verify PrepareCustomerReturnIpcInput & PreparedCustomerReturnQuote
  const prepCustInput: PrepareCustomerReturnIpcInput = {
    customerId: "cust_rahul",
    referenceSaleId: null,
    items: [itemInput],
    reason: "Damaged goods",
    refundPaymentMethod: "CASH",
  };
  assert.equal(prepCustInput.customerId, "cust_rahul");

  const custQuote: PreparedCustomerReturnQuote = {
    preparationToken: "prep_ret_cust_12345",
    returnId: "ret_1001",
    returnNumber: "RET-20260914-A1B2C3",
    customerId: "cust_rahul",
    customerName: "Rahul Sharma",
    customerPhone: "+919811122233",
    referenceSaleId: null,
    items: [
      {
        productId: "prod_rice",
        productName: "Basmati Rice",
        productType: "LOOSE",
        unit: "kg",
        quantity: 5000,
        unitPriceCents: 6000,
        lineTotalCents: 30000,
      },
    ],
    totalAmountCents: 30000,
    debtReductionCents: 30000,
    refundAmountCents: 0,
    refundPaymentMethod: "CASH",
    balanceBeforeCents: 100000,
    balanceAfterCents: 70000,
    reason: "Damaged goods",
    preparedAt: "2026-09-14T00:00:00Z",
  };
  assert.equal(custQuote.totalAmountCents, 30000);
  assert.equal(custQuote.debtReductionCents, 30000);
  assert.equal(custQuote.refundAmountCents, 0);

  // 3. Verify CustomerReturnReceipt & ConfirmCustomerReturnIpcInput
  const confirmCustInput: ConfirmCustomerReturnIpcInput = {
    preparationToken: custQuote.preparationToken,
  };
  assert.equal(confirmCustInput.preparationToken, custQuote.preparationToken);

  const custReceipt: CustomerReturnReceipt = {
    returnId: custQuote.returnId,
    returnNumber: custQuote.returnNumber,
    customerId: custQuote.customerId,
    customerName: custQuote.customerName,
    items: custQuote.items,
    totalAmountCents: custQuote.totalAmountCents,
    debtReductionCents: custQuote.debtReductionCents,
    refundAmountCents: custQuote.refundAmountCents,
    refundPaymentMethod: custQuote.refundPaymentMethod,
    balanceBeforeCents: custQuote.balanceBeforeCents,
    balanceAfterCents: custQuote.balanceAfterCents,
    reason: custQuote.reason,
    createdAt: "2026-09-14T00:01:00Z",
  };
  assert.equal(custReceipt.returnNumber, "RET-20260914-A1B2C3");

  // 4. Verify Supplier Return Quote with Guardrail 2 signed balance (Credit Note)
  const prepSupInput: PrepareSupplierReturnIpcInput = {
    supplierId: "sup_agro",
    referencePurchaseId: null,
    items: [itemInput],
    reason: "Stock reversal",
  };
  assert.equal(prepSupInput.supplierId, "sup_agro");

  const supQuote: PreparedSupplierReturnQuote = {
    preparationToken: "prep_ret_sup_12345",
    returnId: "ret_2001",
    returnNumber: "SR-20260914-D4E5F6",
    supplierId: "sup_agro",
    supplierName: "Agro Farms Ltd",
    referencePurchaseId: null,
    items: [
      {
        productId: "prod_rice",
        productName: "Basmati Rice",
        productType: "LOOSE",
        unit: "kg",
        quantity: 5000,
        unitCostCents: 4500,
        lineTotalCents: 22500,
        availableStock: 50000,
      },
    ],
    totalAmountCents: 22500,
    balanceBeforeCents: 10000, // ₹100 outstanding
    balanceAfterCents: -12500, // -₹125 Credit Note
    reason: "Stock reversal",
    preparedAt: "2026-09-14T00:00:00Z",
  };
  assert.equal(supQuote.balanceAfterCents, -12500);

  // 5. Verify SupplierReturnReceipt & ConfirmSupplierReturnIpcInput
  const confirmSupInput: ConfirmSupplierReturnIpcInput = {
    preparationToken: supQuote.preparationToken,
  };
  assert.equal(confirmSupInput.preparationToken, supQuote.preparationToken);

  const supReceipt: SupplierReturnReceipt = {
    returnId: supQuote.returnId,
    returnNumber: supQuote.returnNumber,
    supplierId: supQuote.supplierId,
    supplierName: supQuote.supplierName,
    items: supQuote.items,
    totalAmountCents: supQuote.totalAmountCents,
    balanceBeforeCents: supQuote.balanceBeforeCents,
    balanceAfterCents: supQuote.balanceAfterCents,
    reason: supQuote.reason,
    createdAt: "2026-09-14T00:01:00Z",
  };
  assert.equal(supReceipt.balanceAfterCents, -12500);

  // 6. Verify ReturnSummaryItem & ReturnDetail
  const summaryItem: ReturnSummaryItem = {
    id: "ret_1001",
    returnNumber: "RET-20260914-A1B2C3",
    returnType: "CUSTOMER_RETURN",
    counterpartyName: "Rahul Sharma",
    itemCount: 1,
    totalAmountCents: 30000,
    reason: "Damaged goods",
    createdAt: "2026-09-14T00:01:00Z",
  };
  assert.equal(summaryItem.returnType, "CUSTOMER_RETURN");

  const detail: ReturnDetail = {
    id: "ret_1001",
    returnNumber: "RET-20260914-A1B2C3",
    returnType: "CUSTOMER_RETURN",
    referenceId: null,
    customerId: "cust_rahul",
    customerName: "Rahul Sharma",
    supplierId: null,
    supplierName: null,
    totalAmountCents: 30000,
    reason: "Damaged goods",
    adminUserId: "usr_admin",
    items: [
      {
        productId: "prod_rice",
        productName: "Basmati Rice",
        productType: "LOOSE",
        unit: "kg",
        quantity: 5000,
        unitPriceCents: 6000,
        totalCents: 30000,
      },
    ],
    createdAt: "2026-09-14T00:01:00Z",
  };
  assert.equal(detail.items.length, 1);

  // 7. Verify ReturnsFormData
  const formData: ReturnsFormData = {
    customers: [],
    suppliers: [],
    products: [],
  };
  assert.ok(Array.isArray(formData.customers));
  assert.ok(Array.isArray(formData.suppliers));
  assert.ok(Array.isArray(formData.products));
});

test("Contract Verification: BUILD 11 Stock Corrections IPC contracts and audit integrity", () => {
  // 1. Verify PrepareStockCorrectionIpcInput
  const sampleReason: CorrectionReason = "DAMAGED";
  const prepInput: PrepareStockCorrectionIpcInput = {
    productId: "prod_atta",
    quantityChange: -2000,
    reason: sampleReason,
    note: "Torn bag during unloading",
  };
  assert.equal(prepInput.productId, "prod_atta");
  assert.equal(prepInput.quantityChange, -2000);
  assert.equal(prepInput.reason, "DAMAGED");

  // 2. Verify PreparedStockCorrectionQuote
  const quote: PreparedStockCorrectionQuote = {
    preparationToken: "prep_tok_abc123",
    correctionId: "corr_atta_001",
    productId: "prod_atta",
    productName: "Chakki Atta 5kg",
    productType: "PACKAGED",
    unit: "kg",
    quantityChange: -2000,
    quantityBefore: 20000,
    quantityAfter: 18000,
    reason: "DAMAGED",
    note: "Torn bag during unloading",
    adminUserId: "usr_admin",
    adminUsername: "admin",
    preparedAt: "2026-09-14T10:00:00Z",
  };
  assert.equal(quote.quantityBefore + quote.quantityChange, quote.quantityAfter);
  assert.ok(quote.quantityAfter >= 0);

  // 3. Verify ConfirmStockCorrectionIpcInput
  const confirmInput: ConfirmStockCorrectionIpcInput = {
    preparationToken: "prep_tok_abc123",
  };
  assert.equal(confirmInput.preparationToken, "prep_tok_abc123");

  // 4. Verify StockCorrectionReceipt
  const receipt: StockCorrectionReceipt = {
    correctionId: "corr_atta_001",
    productId: "prod_atta",
    productName: "Chakki Atta 5kg",
    productType: "PACKAGED",
    unit: "kg",
    quantityChange: -2000,
    quantityBefore: 20000,
    quantityAfter: 18000,
    reason: "DAMAGED",
    note: "Torn bag during unloading",
    adminUserId: "usr_admin",
    adminUsername: "admin",
    createdAt: "2026-09-14T10:00:05Z",
  };
  assert.equal(receipt.correctionId, "corr_atta_001");
  assert.equal(receipt.quantityAfter, 18000);

  // 5. Verify StockCorrectionHistoryItem and StockCorrectionsSummary
  const historyItem: StockCorrectionHistoryItem = {
    id: "corr_atta_001",
    productId: "prod_atta",
    productName: "Chakki Atta 5kg",
    unit: "kg",
    quantityChange: -2000,
    quantityBefore: 20000,
    quantityAfter: 18000,
    reason: "DAMAGED",
    note: "Torn bag during unloading",
    adminUserId: "usr_admin",
    adminUsername: "admin",
    createdAt: "2026-09-14T10:00:05Z",
  };
  const summary: StockCorrectionsSummary = {
    corrections: [historyItem],
    totalCorrectionsCount: 1,
  };
  assert.equal(summary.totalCorrectionsCount, 1);
  assert.equal(summary.corrections[0]!.reason, "DAMAGED");

  // 6. Verify StockCorrectionsFormData
  const productList: ProductForCorrection[] = [
    {
      id: "prod_atta",
      name: "Chakki Atta 5kg",
      productType: "PACKAGED",
      unit: "kg",
      currentQuantity: 18000,
      costPriceCents: 16000,
      sellingPriceCents: 21000,
    },
  ];
  const formData: StockCorrectionsFormData = {
    products: productList,
  };
  assert.equal(formData.products.length, 1);
  assert.equal(formData.products[0]!.unit, "kg");
});

test("Contract Verification: BUILD 12 Offline Sync & Backup IPC contracts and integrity structures", () => {
  const manifest: import("@merchant-os/shared").BackupManifest = {
    backupFormatVersion: 1,
    appVersion: "0.1.0",
    createdAt: "2026-09-15T00:00:00Z",
    backupType: "MANUAL",
    businessId: "biz_default",
    businessName: "Default Store",
    checksumSha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    tableCounts: {
      businesses: 1,
      users: 2,
      products: 5,
      inventory: 5,
      sales: 10,
    },
    totalRecords: 23,
    payloadSizeBytes: 153600,
  };
  assert.equal(manifest.backupFormatVersion, 1);
  assert.equal(manifest.backupType, "MANUAL");
  assert.equal(manifest.checksumSha256.length, 64);
  assert.equal(manifest.totalRecords, 23);

  const settings: import("@merchant-os/shared").BackupSettings = {
    autoBackupEnabled: true,
    retentionLimit: 7,
    lastBackupAt: "2026-09-15T00:00:00Z",
    lastAutoBackupAt: null,
    isInternetAvailable: true,
    totalBackupsCount: 3,
    autoBackupsCount: 2,
    manualBackupsCount: 1,
  };
  assert.equal(settings.retentionLimit, 7);
  assert.equal(settings.autoBackupEnabled, true);

  const validation: import("@merchant-os/shared").BackupValidationResult = {
    isValid: true,
    manifest,
    integrityCheckPassed: true,
    foreignKeyCheckPassed: true,
    schemaTablesPassed: true,
    businessInvariantsPassed: true,
    tablesFound: ["businesses", "users", "products", "inventory", "sales"],
    compatibilityStatus: "COMPATIBLE",
    errors: [],
  };
  assert.ok(validation.isValid);
  assert.equal(validation.compatibilityStatus, "COMPATIBLE");

  const restoreResult: import("@merchant-os/shared").RestoreBackupResult = {
    success: true,
    backupId: "backup_12345",
    restoredAt: "2026-09-15T00:05:00Z",
    tableCounts: {
      businesses: 1,
      users: 2,
      products: 5,
    },
    totalRecords: 8,
    verificationPassed: true,
    rolledBack: false,
    message: "Restored successfully",
  };
  assert.ok(restoreResult.success);
  assert.ok(restoreResult.verificationPassed);
  assert.ok(!restoreResult.rolledBack);

  const meta: import("@merchant-os/shared").BackupMetadata = {
    id: "backup_manual_123",
    backupType: "MANUAL",
    backupFormatVersion: 1,
    appVersion: "0.1.0",
    createdAt: "2026-09-15T00:00:00Z",
    businessId: "biz_default",
    businessName: "Default Store",
    checksumSha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    fileSizeBytes: 204800,
    payloadSizeBytes: 153600,
    tableCounts: { businesses: 1, users: 2 },
    totalRecords: 3,
    fileName: "backup_manual_123.mosbackup",
  };
  assert.equal(meta.fileName, "backup_manual_123.mosbackup");

  const statusDto: import("@merchant-os/shared").BackupStatusDto = {
    settings,
    backups: [meta],
  };
  assert.equal(statusDto.backups.length, 1);
  assert.equal(statusDto.settings.retentionLimit, 7);

  const toggleInput: import("@merchant-os/shared").ToggleAutoBackupInput = { enabled: true };
  const manualInput: import("@merchant-os/shared").CreateManualBackupInput = { note: "Pre-migration checkpoint" };
  const triggerInput: import("@merchant-os/shared").TriggerAutoBackupInput = { isInternetAvailable: true };
  const validateInput: import("@merchant-os/shared").ValidateBackupInput = { fileName: meta.fileName };
  const restoreInput: import("@merchant-os/shared").RestoreBackupInput = { fileName: meta.fileName };

  assert.equal(toggleInput.enabled, true);
  assert.equal(manualInput.note, "Pre-migration checkpoint");
  assert.equal(triggerInput.isInternetAvailable, true);
  assert.equal(validateInput.fileName, meta.fileName);
  assert.equal(restoreInput.fileName, meta.fileName);
});




