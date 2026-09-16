import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  SystemStatus,
  SYSTEM_DEFAULTS,
  SupplierSummary,
  ProductForPurchase,
  PurchaseFormData,
  CreateSupplierForPurchaseInput,
  PreparePurchaseIpcInput,
  PreparedPurchaseQuote,
  PurchaseReceipt,
  PaymentMethod,
  CustomerSummary,
  ProductForSale,
  SalesFormData,
  CreateCustomerForSaleInput,
  PrepareSaleIpcInput,
  PreparedSaleQuote,
  SaleReceipt,
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
  ReturnType,
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
  BackupMetadata,
  BackupSettings,
  BackupStatusDto,
  BackupValidationResult,
  RestoreBackupResult,
  ToggleAutoBackupInput,
  CreateManualBackupInput,
  ValidateBackupInput,
  RestoreBackupInput,
} from "@merchant-os/shared";

interface LocalPurchaseItem {
  id: string; // client temporary row key
  productId: string;
  productName: string;
  productType: "PACKAGED" | "LOOSE";
  unit: string;
  quantityDisplay: string; // e.g. "5" or "2.500"
  quantityMillie: number; // millie-units (scale 1000)
  unitCostRupees: string; // e.g. "175.00"
  unitCostCents: number; // paise/cents
}

interface LocalSaleItem {
  id: string;
  productId: string;
  productName: string;
  productType: "PACKAGED" | "LOOSE";
  unit: string;
  quantityDisplay: string;
  quantityMillie: number;
  sellingPriceCents: number; // Authoritative DB price (cents)
  availableStockMillie: number;
}

interface LocalReturnItem {
  id: string; // client temporary row key
  productId: string;
  productName: string;
  productType: "PACKAGED" | "LOOSE";
  unit: string;
  quantityDisplay: string;
  quantityMillie: number;
  unitPriceCents: number; // selling price for customer returns, cost price for supplier returns
  availableStockMillie: number;
}

export default function App() {
  // --- Workspace Tab State ---
  const [activeTab, setActiveTab] = useState<"pos" | "purchases" | "credits" | "orders" | "returns" | "corrections" | "backups">("pos");

  // --- System & Telemetry State ---
  const [systemStatus, setSystemStatus] = useState<SystemStatus>({
    status: SYSTEM_DEFAULTS.status,
    database: SYSTEM_DEFAULTS.databaseConnected,
    sqliteVersion: "Bundled",
    internet: SYSTEM_DEFAULTS.internet,
    timestamp: new Date().toISOString(),
  });
  const [isTauri, setIsTauri] = useState(true);

  // --- Notifications / Feedback ---
  const [feedback, setFeedback] = useState<{ message: string; type: "error" | "success" | "info" } | null>(null);
  const showNotification = (message: string, type: "error" | "success" | "info" = "info") => {
    setFeedback({ message, type });
    setTimeout(() => {
      setFeedback(null);
    }, 4500);
  };

  // ==============================================================================================
  // BUILD 07: SALES / POS STATE
  // ==============================================================================================
  const [posCustomers, setPosCustomers] = useState<CustomerSummary[]>([]);
  const [posProducts, setPosProducts] = useState<ProductForSale[]>([]);
  const [posLoading, setPosLoading] = useState(false);

  // Active POS Cart
  const [posBarcode, setPosBarcode] = useState("");
  const [posCart, setPosCart] = useState<LocalSaleItem[]>([]);
  const [posSettlementMode, setPosSettlementMode] = useState<"PAID" | "CREDIT">("PAID");
  const [posPaymentMethod, setPosPaymentMethod] = useState<PaymentMethod>("CASH");
  const [posSelectedCustomerId, setPosSelectedCustomerId] = useState<string>("");

  // Customer Modal
  const [showAddCustomerModal, setShowAddCustomerModal] = useState(false);
  const [newCustomerName, setNewCustomerName] = useState("");
  const [newCustomerPhone, setNewCustomerPhone] = useState("");
  const [addingCustomer, setAddingCustomer] = useState(false);

  // Quote & Receipt Modals
  const [preparedSaleQuote, setPreparedSaleQuote] = useState<PreparedSaleQuote | null>(null);
  const [preparingSale, setPreparingSale] = useState(false);
  const [confirmingSale, setConfirmingSale] = useState(false);
  const [completedSaleReceipt, setCompletedSaleReceipt] = useState<SaleReceipt | null>(null);

  // ==============================================================================================
  // BUILD 06: PURCHASES STATE
  // ==============================================================================================
  const [suppliers, setSuppliers] = useState<SupplierSummary[]>([]);
  const [purchaseProducts, setPurchaseProducts] = useState<ProductForPurchase[]>([]);
  const [loadingPurchases, setLoadingPurchases] = useState(false);

  // Active Purchase Form
  const [selectedSupplierId, setSelectedSupplierId] = useState<string>("");
  const [purchaseBarcode, setPurchaseBarcode] = useState("");
  const [purchaseItems, setPurchaseItems] = useState<LocalPurchaseItem[]>([]);
  const [paidAmountRupees, setPaidAmountRupees] = useState("0");
  const [purchasePaymentMethod, setPurchasePaymentMethod] = useState<PaymentMethod>("CASH");

  // Supplier Modal
  const [showAddSupplierModal, setShowAddSupplierModal] = useState(false);
  const [newSupplierName, setNewSupplierName] = useState("");
  const [newSupplierPhone, setNewSupplierPhone] = useState("");
  const [addingSupplier, setAddingSupplier] = useState(false);

  // Purchase Quote & Receipt Modals
  const [preparedPurchaseQuote, setPreparedPurchaseQuote] = useState<PreparedPurchaseQuote | null>(null);
  const [preparingPurchase, setPreparingPurchase] = useState(false);
  const [confirmingPurchase, setConfirmingPurchase] = useState(false);
  const [completedPurchaseReceipt, setCompletedPurchaseReceipt] = useState<PurchaseReceipt | null>(null);

  // ==============================================================================================
  // BUILD 08: CUSTOMER CREDITS & PAYMENTS (KHATA) STATE
  // ==============================================================================================
  const [creditCustomers, setCreditCustomers] = useState<CustomerCreditItem[]>([]);
  const [totalOutstandingCents, setTotalOutstandingCents] = useState<number>(0);
  const [loadingCredits, setLoadingCredits] = useState(false);
  const [creditSearchQuery, setCreditSearchQuery] = useState("");
  const [selectedCreditCustomer, setSelectedCreditCustomer] = useState<CustomerCreditItem | null>(null);
  const [customerLedgerHistory, setCustomerLedgerHistory] = useState<CustomerLedgerItem[]>([]);
  const [loadingLedger, setLoadingLedger] = useState(false);

  // Payment Form & Confirmation State
  const [showRecordPaymentModal, setShowRecordPaymentModal] = useState(false);
  const [paymentAmountRupees, setPaymentAmountRupees] = useState("");
  const [paymentMethod, setPaymentMethod] = useState<PaymentMethod>("CASH");
  const [paymentNotes, setPaymentNotes] = useState("");

  const [preparedCustomerPaymentQuote, setPreparedCustomerPaymentQuote] = useState<PreparedCustomerPaymentQuote | null>(null);
  const [preparingPayment, setPreparingPayment] = useState(false);
  const [confirmingPayment, setConfirmingPayment] = useState(false);
  const [completedCustomerPaymentReceipt, setCompletedCustomerPaymentReceipt] = useState<CustomerPaymentReceipt | null>(null);

  // ==============================================================================================
  // BUILD 09: CUSTOMER ORDERS STATE
  // ==============================================================================================
  const [ordersSummary, setOrdersSummary] = useState<CustomerOrderSummaryItem[]>([]);
  const [ordersLoading, setOrdersLoading] = useState(false);
  const [ordersFilter, setOrdersFilter] = useState<"ALL" | "DRAFT" | "CONVERTED" | "CANCELLED">("ALL");
  const [ordersSearchQuery, setOrdersSearchQuery] = useState("");
  const [selectedOrderId, setSelectedOrderId] = useState<string | null>(null);
  const [selectedOrderDetail, setSelectedOrderDetail] = useState<CustomerOrderDetail | null>(null);
  const [loadingOrderDetail, setLoadingOrderDetail] = useState(false);

  // Orders Form Data (customers & products for order creation/editing)
  const [ordersFormData, setOrdersFormData] = useState<CustomerOrdersFormData | null>(null);
  const [loadingOrdersFormData, setLoadingOrdersFormData] = useState(false);

  // Create / Edit Order Modal
  const [showOrderModal, setShowOrderModal] = useState(false);
  const [editingOrderId, setEditingOrderId] = useState<string | null>(null);
  const [orderFormCustomerId, setOrderFormCustomerId] = useState<string>("");
  const [orderFormItems, setOrderFormItems] = useState<
    Array<{ id: string; productId: string; quantityDisplay: string; quantityMillie: number; notes: string }>
  >([]);
  const [orderFormNotes, setOrderFormNotes] = useState("");
  const [savingOrder, setSavingOrder] = useState(false);

  // Convert Order Modal
  const [showConvertOrderModal, setShowConvertOrderModal] = useState(false);
  const [convertOrderTarget, setConvertOrderTarget] = useState<CustomerOrderDetail | null>(null);
  const [convertSettlementMode, setConvertSettlementMode] = useState<"PAID" | "CREDIT">("PAID");
  const [convertPaymentMethod, setConvertPaymentMethod] = useState<PaymentMethod>("CASH");
  const [preparedOrderConversionQuote, setPreparedOrderConversionQuote] =
    useState<PreparedOrderConversionQuote | null>(null);
  const [preparingConversion, setPreparingConversion] = useState(false);
  const [confirmingConversion, setConfirmingConversion] = useState(false);

  // ==============================================================================================
  // BUILD 10: RETURNS & STOCK REVERSAL STATE
  // ==============================================================================================
  const [returnsSummary, setReturnsSummary] = useState<ReturnSummaryItem[]>([]);
  const [returnsLoading, setReturnsLoading] = useState(false);
  const [returnsFilter, setReturnsFilter] = useState<"ALL" | ReturnType>("ALL");
  const [returnsSearchQuery, setReturnsSearchQuery] = useState("");
  const [selectedReturnId, setSelectedReturnId] = useState<string | null>(null);
  const [selectedReturnDetail, setSelectedReturnDetail] = useState<ReturnDetail | null>(null);
  const [loadingReturnDetail, setLoadingReturnDetail] = useState(false);
  const [returnsFormData, setReturnsFormData] = useState<ReturnsFormData | null>(null);

  // Customer Return Form Modal State
  const [showCustomerReturnModal, setShowCustomerReturnModal] = useState(false);
  const [custReturnCustomerId, setCustReturnCustomerId] = useState<string>("");
  const [custReturnItems, setCustReturnItems] = useState<LocalReturnItem[]>([]);
  const [custReturnRefundMethod, setCustReturnRefundMethod] = useState<PaymentMethod>("CASH");
  const [preparedCustomerReturnQuote, setPreparedCustomerReturnQuote] = useState<PreparedCustomerReturnQuote | null>(null);
  const [preparingCustReturn, setPreparingCustReturn] = useState(false);
  const [confirmingCustReturn, setConfirmingCustReturn] = useState(false);
  const [completedCustomerReturnReceipt, setCompletedCustomerReturnReceipt] = useState<CustomerReturnReceipt | null>(null);

  // Supplier Return Form Modal State
  const [showSupplierReturnModal, setShowSupplierReturnModal] = useState(false);
  const [suppReturnSupplierId, setSuppReturnSupplierId] = useState<string>("");
  const [suppReturnItems, setSuppReturnItems] = useState<LocalReturnItem[]>([]);
  const [suppReturnReason, setSuppReturnReason] = useState<string>("");
  const [preparedSupplierReturnQuote, setPreparedSupplierReturnQuote] = useState<PreparedSupplierReturnQuote | null>(null);
  const [preparingSuppReturn, setPreparingSuppReturn] = useState(false);
  const [confirmingSuppReturn, setConfirmingSuppReturn] = useState(false);
  const [completedSupplierReturnReceipt, setCompletedSupplierReturnReceipt] = useState<SupplierReturnReceipt | null>(null);

  // ==============================================================================================
  // BUILD 11: STOCK CORRECTIONS & PHYSICAL INVENTORY ADJUSTMENT STATE
  // ==============================================================================================
  const [correctionProducts, setCorrectionProducts] = useState<ProductForCorrection[]>([]);
  const [correctionHistory, setCorrectionHistory] = useState<StockCorrectionHistoryItem[]>([]);
  const [_loadingCorrections, setLoadingCorrections] = useState(false);

  // Active Adjustment Form
  const [selectedCorrectionProductId, setSelectedCorrectionProductId] = useState<string>("");
  const [adjustmentMode, setAdjustmentMode] = useState<"PHYSICAL_COUNT" | "MANUAL_DELTA">("PHYSICAL_COUNT");
  const [physicalCountInput, setPhysicalCountInput] = useState<string>("");
  const [manualDeltaInput, setManualDeltaInput] = useState<string>("");
  const [correctionReason, setCorrectionReason] = useState<CorrectionReason>("MISCOUNT");
  const [correctionNote, setCorrectionNote] = useState<string>("");
  const [correctionsSearchQuery, setCorrectionsSearchQuery] = useState<string>("");

  // Modals & Receipts
  const [preparedCorrectionQuote, setPreparedCorrectionQuote] = useState<PreparedStockCorrectionQuote | null>(null);
  const [preparingCorrection, setPreparingCorrection] = useState(false);
  const [confirmingCorrection, setConfirmingCorrection] = useState(false);
  const [completedCorrectionReceipt, setCompletedCorrectionReceipt] = useState<StockCorrectionReceipt | null>(null);

  // ==============================================================================================
  // BUILD 12: OFFLINE SYNC & BACKUP STATE
  // ==============================================================================================
  const [backupSettings, setBackupSettings] = useState<BackupSettings | null>(null);
  const [backupsList, setBackupsList] = useState<BackupMetadata[]>([]);
  const [loadingBackups, setLoadingBackups] = useState(false);
  const [togglingAutoBackup, setTogglingAutoBackup] = useState(false);
  const [creatingBackup, setCreatingBackup] = useState(false);
  const [validatingBackup, setValidatingBackup] = useState(false);
  const [restoringBackup, setRestoringBackup] = useState(false);

  // Modals & Controls
  const [showCreateBackupModal, setShowCreateBackupModal] = useState(false);
  const [manualBackupNote, setManualBackupNote] = useState("");
  const [backupSearchQuery, setBackupSearchQuery] = useState("");
  const [backupFilter, setBackupFilter] = useState<"ALL" | "MANUAL" | "AUTO">("ALL");

  // Validate Modal
  const [showValidationModal, setShowValidationModal] = useState(false);
  const [selectedBackupForAction, setSelectedBackupForAction] = useState<BackupMetadata | null>(null);
  const [validationReport, setValidationReport] = useState<BackupValidationResult | null>(null);

  // Restore Safety Modal
  const [showRestoreModal, setShowRestoreModal] = useState(false);
  const [restoreCandidate, setRestoreCandidate] = useState<BackupMetadata | null>(null);
  const [restoreValidationReport, setRestoreValidationReport] = useState<BackupValidationResult | null>(null);
  const [loadingRestoreValidation, setLoadingRestoreValidation] = useState(false);
  const [restoreConfirmedCheck, setRestoreConfirmedCheck] = useState(false);

  // --- Initial Data Loader ---
  const loadInitialData = useCallback(async () => {
    try {
      const statusRes = await invoke<SystemStatus>("get_system_status");
      setSystemStatus(statusRes);
      setIsTauri(true);

      // 1. Fetch Sales Form Data
      setPosLoading(true);
      const salesData = await invoke<SalesFormData>("get_sales_form_data");
      setPosCustomers(salesData.customers);
      setPosProducts(salesData.products);

      // 2. Fetch Purchases Form Data
      setLoadingPurchases(true);
      const purchaseData = await invoke<PurchaseFormData>("get_purchase_form_data");
      setSuppliers(purchaseData.suppliers);
      setPurchaseProducts(purchaseData.products);
      if (purchaseData.suppliers.length > 0) {
        setSelectedSupplierId(purchaseData.suppliers[0]!.id);
      }

      // 3. Fetch Customer Credits Summary
      setLoadingCredits(true);
      const creditsData = await invoke<CustomerCreditsSummary>("get_customer_credits_summary");
      setCreditCustomers(creditsData.customers);
      setTotalOutstandingCents(creditsData.totalOutstandingCents);

      // 4. Fetch Customer Orders Summary
      setOrdersLoading(true);
      const ordersData = await invoke<CustomerOrderSummaryItem[]>("get_customer_orders_summary", {
        statusFilter: null,
      });
      setOrdersSummary(ordersData);

      // 5. Fetch Returns Summary
      setReturnsLoading(true);
      const returnsData = await invoke<ReturnSummaryItem[]>("get_returns_summary", {
        typeFilter: null,
      });
      setReturnsSummary(returnsData);

      // 6. Fetch Stock Corrections Data (BUILD 11)
      setLoadingCorrections(true);
      const corrData = await invoke<StockCorrectionsFormData>("get_stock_corrections_form_data");
      setCorrectionProducts(corrData.products);
      const corrSummary = await invoke<StockCorrectionsSummary>("get_stock_corrections_summary");
      setCorrectionHistory(corrSummary.corrections);

      // 7. Fetch Offline Sync & Backup Status (BUILD 12)
      setLoadingBackups(true);
      const backupData = await invoke<BackupStatusDto>("get_backup_status");
      setBackupSettings(backupData.settings);
      setBackupsList(backupData.backups);
    } catch (err: any) {
      console.warn("Operating in web preview mode or IPC unavailable:", err);
      setIsTauri(false);
      // Fallback mock data for web browser preview
      const mockCustomers: CustomerSummary[] = [
        { id: "cust_1", name: "Ramesh Sharma", phone: "+919811100001", currentBalanceCents: 45000 },
        { id: "cust_2", name: "Sunita Verma", phone: "+919822200002", currentBalanceCents: 0 },
      ];
      const mockProducts: ProductForSale[] = [
        { id: "p1", name: "Basmati Rice 25kg Bag", productType: "PACKAGED", unit: "pcs", costPriceCents: 180000, sellingPriceCents: 220000, currentQuantity: 10000 },
        { id: "p2", name: "Mustard Oil Pure", productType: "LOOSE", unit: "litre", costPriceCents: 14000, sellingPriceCents: 17500, currentQuantity: 25000 },
        { id: "p3", name: "Chana Dal Premium", productType: "LOOSE", unit: "kg", costPriceCents: 7500, sellingPriceCents: 9500, currentQuantity: 40000 },
        { id: "p4", name: "Tata Salt 1kg", productType: "PACKAGED", unit: "pcs", costPriceCents: 2200, sellingPriceCents: 2800, currentQuantity: 50000 },
      ];
      setPosCustomers(mockCustomers);
      setPosProducts(mockProducts);
      setSuppliers([
        { id: "supp_1", name: "Grain Traders Co", phone: "+919999900001", address: "Delhi Wholesale Yard", currentOutstandingCents: 150000 },
      ]);
      setPurchaseProducts(mockProducts);
      const mockCreditCustomers: CustomerCreditItem[] = [
        { id: "cust_1", name: "Ramesh Sharma", phone: "+919811100001", address: "Sector 4, Noida", currentCreditCents: 45000, isActive: 1, createdAt: "2026-09-13T10:00:00Z", updatedAt: "2026-09-13T10:00:00Z" },
        { id: "cust_2", name: "Sunita Verma", phone: "+919822200002", address: "Connaught Place, Delhi", currentCreditCents: 120000, isActive: 1, createdAt: "2026-09-13T10:05:00Z", updatedAt: "2026-09-13T10:05:00Z" },
      ];
      setCreditCustomers(mockCreditCustomers);
      setTotalOutstandingCents(165000);
      setOrdersSummary([
        {
          id: "ord_1",
          orderNumber: "ORD-20260913-7F2A01",
          customerId: "cust_1",
          customerName: "Ramesh Sharma",
          customerPhone: "+919811100001",
          status: "DRAFT",
          itemCount: 2,
          totalAmountCents: 52000,
          convertedSaleId: null,
          notes: "Call before packing",
          createdAt: "2026-09-13T12:00:00Z",
          updatedAt: "2026-09-13T12:00:00Z",
        },
      ]);
      setReturnsSummary([
        {
          id: "ret_1",
          returnNumber: "RET-20260914-A1B2C3",
          returnType: "CUSTOMER_RETURN",
          counterpartyName: "Ramesh Sharma",
          itemCount: 1,
          totalAmountCents: 220000,
          reason: "Damaged packaging",
          createdAt: "2026-09-14T00:10:00Z",
        },
      ]);
      setBackupSettings({
        autoBackupEnabled: true,
        retentionLimit: 7,
        lastBackupAt: new Date().toISOString(),
        lastAutoBackupAt: null,
        isInternetAvailable: true,
        totalBackupsCount: 0,
        autoBackupsCount: 0,
        manualBackupsCount: 0,
      });
      setBackupsList([]);
    } finally {
      setPosLoading(false);
      setLoadingPurchases(false);
      setLoadingCredits(false);
      setOrdersLoading(false);
      setReturnsLoading(false);
      setLoadingBackups(false);
    }
  }, []);

  useEffect(() => {
    void loadInitialData();
  }, [loadInitialData]);

  // ==============================================================================================
  // BUILD 07: POS WORKFLOW HANDLERS
  // ==============================================================================================

  const addProductToPos = (prod: ProductForSale) => {
    if (prod.currentQuantity <= 0) {
      showNotification(`"${prod.name}" is out of stock!`, "error");
      return;
    }

    const existingIndex = posCart.findIndex((i) => i.productId === prod.id);
    if (existingIndex >= 0) {
      const updated = [...posCart];
      const current = updated[existingIndex]!;
      const step = prod.productType === "LOOSE" ? 500 : 1000;
      const newQty = current.quantityMillie + step;

      if (newQty > current.availableStockMillie) {
        showNotification(`Cannot exceed available stock of ${(current.availableStockMillie / 1000).toFixed(prod.productType === "LOOSE" ? 3 : 0)} ${prod.unit}`, "error");
        return;
      }

      updated[existingIndex] = {
        ...current,
        quantityMillie: newQty,
        quantityDisplay: (newQty / 1000).toString(),
      };
      setPosCart(updated);
    } else {
      const defaultQty = prod.productType === "LOOSE" ? 1000 : 1000;
      const newItem: LocalSaleItem = {
        id: `pos_row_${Date.now()}_${Math.random().toString(36).substring(2, 5)}`,
        productId: prod.id,
        productName: prod.name,
        productType: prod.productType,
        unit: prod.unit,
        quantityDisplay: (defaultQty / 1000).toString(),
        quantityMillie: defaultQty,
        sellingPriceCents: prod.sellingPriceCents,
        availableStockMillie: prod.currentQuantity,
      };
      setPosCart((prev) => [...prev, newItem]);
    }
    showNotification(`Added ${prod.name} to cart`, "info");
  };

  const handlePosBarcodeSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const barcode = posBarcode.trim();
    if (!barcode) return;

    try {
      let matchedProduct: ProductForSale;
      if (isTauri) {
        matchedProduct = await invoke<ProductForSale>("resolve_barcode_for_sale", { barcode });
      } else {
        const found = posProducts.find((p) => p.name.toLowerCase().includes(barcode.toLowerCase()));
        if (!found) throw new Error(`Barcode ${barcode} not found`);
        matchedProduct = found;
      }
      addProductToPos(matchedProduct);
      setPosBarcode("");
    } catch (err: any) {
      showNotification(err?.toString() || `Barcode '${barcode}' not found in catalog`, "error");
    }
  };

  const handlePosQuantityChange = (id: string, value: string) => {
    const parsed = parseFloat(value);
    const milli = isNaN(parsed) || parsed < 0 ? 0 : Math.round(parsed * 1000);
    setPosCart((prev) =>
      prev.map((item) => {
        if (item.id !== id) return item;
        return {
          ...item,
          quantityDisplay: value,
          quantityMillie: milli,
        };
      })
    );
  };

  const removePosItem = (id: string) => {
    setPosCart((prev) => prev.filter((item) => item.id !== id));
  };

  // Calculate Live POS Cart Total
  const posCartTotalCents = posCart.reduce((acc, item) => {
    const lineTotal = Math.round((item.quantityMillie * item.sellingPriceCents) / 1000);
    return acc + lineTotal;
  }, 0);

  // POS Prepare Sale
  const handlePrepareSale = async () => {
    if (posCart.length === 0) {
      showNotification("Cart is empty! Add products before checkout.", "error");
      return;
    }

    // Validate quantities and stock
    for (const item of posCart) {
      if (item.quantityMillie <= 0) {
        showNotification(`Item "${item.productName}" must have a quantity greater than zero.`, "error");
        return;
      }
      if (item.quantityMillie > item.availableStockMillie) {
        showNotification(`Stock exceeded for "${item.productName}": requesting ${(item.quantityMillie / 1000).toFixed(2)}, available ${(item.availableStockMillie / 1000).toFixed(2)} ${item.unit}.`, "error");
        return;
      }
    }

    // Binary settlement validation
    if (posSettlementMode === "CREDIT" && !posSelectedCustomerId) {
      showNotification("Credit sale requires a registered customer identity.", "error");
      return;
    }

    setPreparingSale(true);
    const input: PrepareSaleIpcInput = {
      customerId: posSelectedCustomerId ? posSelectedCustomerId : undefined,
      items: posCart.map((it) => ({
        productId: it.productId,
        quantity: it.quantityMillie,
      })),
      settlementMode: posSettlementMode,
      paymentMethod: posSettlementMode === "PAID" ? posPaymentMethod : undefined,
    };

    try {
      if (isTauri) {
        const quote = await invoke<PreparedSaleQuote>("prepare_sale", { input });
        setPreparedSaleQuote(quote);
      } else {
        // Mock Quote
        const quote: PreparedSaleQuote = {
          preparationToken: `prep_mock_${Date.now()}`,
          saleId: `sale_mock_${Date.now()}`,
          saleNumber: `INV-${Date.now().toString().slice(-4)}`,
          customerId: posSelectedCustomerId || null,
          customerName: posCustomers.find((c) => c.id === posSelectedCustomerId)?.name || null,
          items: posCart.map((i) => ({
            productId: i.productId,
            productName: i.productName,
            unit: i.unit,
            quantity: i.quantityMillie,
            unitPriceCents: i.sellingPriceCents,
            lineTotalCents: Math.round((i.quantityMillie * i.sellingPriceCents) / 1000),
          })),
          totalAmountCents: posCartTotalCents,
          paidAmountCents: posSettlementMode === "PAID" ? posCartTotalCents : 0,
          creditAmountCents: posSettlementMode === "CREDIT" ? posCartTotalCents : 0,
          settlementMode: posSettlementMode,
          paymentMethod: posSettlementMode === "PAID" ? posPaymentMethod : null,
          preparedAt: new Date().toISOString(),
        };
        setPreparedSaleQuote(quote);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to prepare sale quote", "error");
    } finally {
      setPreparingSale(false);
    }
  };

  // POS Confirm Sale
  const handleConfirmSale = async () => {
    if (!preparedSaleQuote) return;
    setConfirmingSale(true);

    try {
      if (isTauri) {
        const receipt = await invoke<SaleReceipt>("confirm_sale", {
          input: { preparationToken: preparedSaleQuote.preparationToken },
        });
        setCompletedSaleReceipt(receipt);
      } else {
        // Mock Receipt
        const receipt: SaleReceipt = {
          saleId: preparedSaleQuote.saleId,
          saleNumber: preparedSaleQuote.saleNumber,
          customerId: preparedSaleQuote.customerId,
          customerName: preparedSaleQuote.customerName,
          customerPhone: "+919811100001",
          items: preparedSaleQuote.items.map((i) => ({
            productId: i.productId,
            productName: i.productName,
            unit: i.unit,
            quantity: i.quantity,
            unitPriceCents: i.unitPriceCents,
            totalCents: i.lineTotalCents,
          })),
          totalAmountCents: preparedSaleQuote.totalAmountCents,
          paidAmountCents: preparedSaleQuote.paidAmountCents,
          creditAmountCents: preparedSaleQuote.creditAmountCents,
          settlementMode: preparedSaleQuote.settlementMode,
          paymentMethod: preparedSaleQuote.paymentMethod,
          saleDate: new Date().toISOString(),
          createdAt: new Date().toISOString(),
        };
        setCompletedSaleReceipt(receipt);
      }

      setPreparedSaleQuote(null);
      setPosCart([]);
      showNotification("Sale confirmed successfully!", "success");
      void loadInitialData();
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to confirm sale", "error");
    } finally {
      setConfirmingSale(false);
    }
  };

  // Inline Create Customer
  const handleCreateCustomer = async (e: React.FormEvent) => {
    e.preventDefault();
    const name = newCustomerName.trim();
    if (!name) {
      showNotification("Customer name is required", "error");
      return;
    }

    setAddingCustomer(true);
    const input: CreateCustomerForSaleInput = {
      name,
      phone: newCustomerPhone.trim() || undefined,
    };

    try {
      let created: CustomerSummary;
      if (isTauri) {
        created = await invoke<CustomerSummary>("create_customer_for_sale", { request: input });
      } else {
        created = {
          id: `cust_${Date.now()}`,
          name: input.name,
          phone: input.phone || null,
          currentBalanceCents: 0,
        };
      }

      setPosCustomers((prev) => [created, ...prev]);
      setPosSelectedCustomerId(created.id);
      setShowAddCustomerModal(false);
      setNewCustomerName("");
      setNewCustomerPhone("");
      showNotification(`Customer "${created.name}" registered successfully!`, "success");
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to register customer", "error");
    } finally {
      setAddingCustomer(false);
    }
  };

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "F2" && activeTab === "pos" && !preparedSaleQuote && !completedSaleReceipt && !showAddCustomerModal) {
        e.preventDefault();
        void handlePrepareSale();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [activeTab, posCart, posSettlementMode, posSelectedCustomerId, posPaymentMethod, preparedSaleQuote, completedSaleReceipt, showAddCustomerModal]);

  // ==============================================================================================
  // BUILD 06: PURCHASES WORKFLOW HANDLERS
  // ==============================================================================================

  const addProductToPurchase = (prod: ProductForPurchase) => {
    const existingIndex = purchaseItems.findIndex((i) => i.productId === prod.id);
    if (existingIndex >= 0) {
      const updated = [...purchaseItems];
      const current = updated[existingIndex]!;
      const newQty = current.quantityMillie + 1000;
      updated[existingIndex] = {
        ...current,
        quantityMillie: newQty,
        quantityDisplay: (newQty / 1000).toString(),
      };
      setPurchaseItems(updated);
    } else {
      const defaultQtyMillie = 1000;
      const newItem: LocalPurchaseItem = {
        id: `pur_row_${Date.now()}_${Math.random().toString(36).substring(2, 5)}`,
        productId: prod.id,
        productName: prod.name,
        productType: prod.productType,
        unit: prod.unit,
        quantityDisplay: (defaultQtyMillie / 1000).toString(),
        quantityMillie: defaultQtyMillie,
        unitCostRupees: (prod.costPriceCents / 100).toFixed(2),
        unitCostCents: prod.costPriceCents,
      };
      setPurchaseItems((prev) => [...prev, newItem]);
    }
    showNotification(`Added ${prod.name} to purchase`, "info");
  };

  const handlePurchaseBarcodeSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const barcode = purchaseBarcode.trim();
    if (!barcode) return;

    try {
      let matchedProduct: ProductForPurchase;
      if (isTauri) {
        matchedProduct = await invoke<ProductForPurchase>("resolve_barcode_for_purchase", { barcode });
      } else {
        const found = purchaseProducts.find((p) => p.name.toLowerCase().includes(barcode.toLowerCase()));
        if (!found) throw new Error(`Barcode ${barcode} not found`);
        matchedProduct = found;
      }
      addProductToPurchase(matchedProduct);
      setPurchaseBarcode("");
    } catch (err: any) {
      showNotification(err?.toString() || `Barcode '${barcode}' not found in catalog`, "error");
    }
  };

  const handlePurchaseQuantityChange = (id: string, value: string) => {
    const parsed = parseFloat(value);
    const milli = isNaN(parsed) || parsed < 0 ? 0 : Math.round(parsed * 1000);
    setPurchaseItems((prev) =>
      prev.map((item) => (item.id === id ? { ...item, quantityDisplay: value, quantityMillie: milli } : item))
    );
  };

  const handlePurchaseUnitCostChange = (id: string, value: string) => {
    const parsed = parseFloat(value);
    const cents = isNaN(parsed) || parsed < 0 ? 0 : Math.round(parsed * 100);
    setPurchaseItems((prev) =>
      prev.map((item) => (item.id === id ? { ...item, unitCostRupees: value, unitCostCents: cents } : item))
    );
  };

  const removePurchaseItem = (id: string) => {
    setPurchaseItems((prev) => prev.filter((item) => item.id !== id));
  };

  const estimatedPurchaseSubtotalCents = purchaseItems.reduce((acc, item) => {
    const lineTotal = Math.round((item.quantityMillie * item.unitCostCents) / 1000);
    return acc + lineTotal;
  }, 0);

  const purchasePaidCents = Math.round((parseFloat(paidAmountRupees) || 0) * 100);
  const estimatedPurchaseDueCents = Math.max(0, estimatedPurchaseSubtotalCents - purchasePaidCents);

  const handlePreparePurchase = async () => {
    if (!selectedSupplierId) {
      showNotification("Please select a supplier", "error");
      return;
    }
    if (purchaseItems.length === 0) {
      showNotification("Purchase must contain at least one item", "error");
      return;
    }
    for (const item of purchaseItems) {
      if (item.quantityMillie <= 0) {
        showNotification(`Quantity for ${item.productName} must be positive`, "error");
        return;
      }
      if (item.unitCostCents <= 0) {
        showNotification(`Buying cost for ${item.productName} must be positive`, "error");
        return;
      }
    }
    if (purchasePaidCents > estimatedPurchaseSubtotalCents) {
      showNotification("Overpayment rejected: paid amount cannot exceed total purchase amount", "error");
      return;
    }

    setPreparingPurchase(true);
    const input: PreparePurchaseIpcInput = {
      supplierId: selectedSupplierId,
      items: purchaseItems.map((i) => ({
        productId: i.productId,
        quantity: i.quantityMillie,
        unitCostCents: i.unitCostCents,
      })),
      paidAmountCents: purchasePaidCents,
      paymentMethod: purchasePaidCents > 0 ? purchasePaymentMethod : undefined,
    };

    try {
      if (isTauri) {
        const quote = await invoke<PreparedPurchaseQuote>("prepare_purchase", { input });
        setPreparedPurchaseQuote(quote);
      } else {
        const quote: PreparedPurchaseQuote = {
          preparationToken: `prep_mock_${Date.now()}`,
          purchaseId: `pur_mock_${Date.now()}`,
          purchaseNumber: `PO-${Date.now().toString().slice(-4)}`,
          supplierId: selectedSupplierId,
          supplierName: suppliers.find((s) => s.id === selectedSupplierId)?.name || "Supplier",
          items: purchaseItems.map((i) => ({
            productId: i.productId,
            productName: i.productName,
            unit: i.unit,
            quantity: i.quantityMillie,
            unitCostCents: i.unitCostCents,
            lineTotalCents: Math.round((i.quantityMillie * i.unitCostCents) / 1000),
          })),
          totalAmountCents: estimatedPurchaseSubtotalCents,
          paidAmountCents: purchasePaidCents,
          creditAmountCents: estimatedPurchaseDueCents,
          paymentStatus: purchasePaidCents === 0 ? "CREDIT" : purchasePaidCents === estimatedPurchaseSubtotalCents ? "PAID" : "PARTIAL",
          paymentMethod: purchasePaidCents > 0 ? purchasePaymentMethod : null,
          purchaseDate: new Date().toISOString(),
          preparedAt: new Date().toISOString(),
        };
        setPreparedPurchaseQuote(quote);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to prepare purchase", "error");
    } finally {
      setPreparingPurchase(false);
    }
  };

  const handleConfirmPurchase = async () => {
    if (!preparedPurchaseQuote) return;
    setConfirmingPurchase(true);

    try {
      if (isTauri) {
        const receipt = await invoke<PurchaseReceipt>("confirm_purchase", {
          input: { preparationToken: preparedPurchaseQuote.preparationToken },
        });
        setCompletedPurchaseReceipt(receipt);
      } else {
        const receipt: PurchaseReceipt = {
          purchaseId: preparedPurchaseQuote.purchaseId,
          purchaseNumber: preparedPurchaseQuote.purchaseNumber,
          supplierId: preparedPurchaseQuote.supplierId,
          supplierName: preparedPurchaseQuote.supplierName,
          supplierPhone: "+919999900001",
          items: preparedPurchaseQuote.items.map((i) => ({
            productId: i.productId,
            productName: i.productName,
            unit: i.unit,
            quantity: i.quantity,
            unitCostCents: i.unitCostCents,
            totalCents: i.lineTotalCents,
          })),
          totalAmountCents: preparedPurchaseQuote.totalAmountCents,
          paidAmountCents: preparedPurchaseQuote.paidAmountCents,
          creditAmountCents: preparedPurchaseQuote.creditAmountCents,
          paymentStatus: preparedPurchaseQuote.paymentStatus,
          paymentMethod: preparedPurchaseQuote.paymentMethod,
          purchaseDate: preparedPurchaseQuote.purchaseDate,
          createdAt: new Date().toISOString(),
        };
        setCompletedPurchaseReceipt(receipt);
      }

      setPreparedPurchaseQuote(null);
      setPurchaseItems([]);
      setPaidAmountRupees("0");
      showNotification("Purchase completed and inventory updated!", "success");
      void loadInitialData();
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to confirm purchase", "error");
    } finally {
      setConfirmingPurchase(false);
    }
  };

  const handleCreateSupplier = async (e: React.FormEvent) => {
    e.preventDefault();
    const name = newSupplierName.trim();
    if (!name) {
      showNotification("Supplier name is required", "error");
      return;
    }

    setAddingSupplier(true);
    const input: CreateSupplierForPurchaseInput = {
      name,
      phone: newSupplierPhone.trim() || undefined,
    };

    try {
      let created: SupplierSummary;
      if (isTauri) {
        created = await invoke<SupplierSummary>("create_supplier_for_purchase", { request: input });
      } else {
        created = {
          id: `supp_${Date.now()}`,
          name: input.name,
          phone: input.phone || null,
          address: null,
          currentOutstandingCents: 0,
        };
      }

      setSuppliers((prev) => [created, ...prev]);
      setSelectedSupplierId(created.id);
      setShowAddSupplierModal(false);
      setNewSupplierName("");
      setNewSupplierPhone("");
      showNotification(`Supplier "${created.name}" created!`, "success");
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to create supplier", "error");
    } finally {
      setAddingSupplier(false);
    }
  };

  // ==============================================================================================
  // BUILD 08: CUSTOMER CREDITS (KHATA) WORKFLOW HANDLERS
  // ==============================================================================================

  const loadCustomerCredits = useCallback(async () => {
    setLoadingCredits(true);
    try {
      if (isTauri) {
        const summary = await invoke<CustomerCreditsSummary>("get_customer_credits_summary");
        setCreditCustomers(summary.customers);
        setTotalOutstandingCents(summary.totalOutstandingCents);
        if (selectedCreditCustomer) {
          const updated = summary.customers.find((c) => c.id === selectedCreditCustomer.id);
          if (updated) setSelectedCreditCustomer(updated);
        }
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to load customer credits", "error");
    } finally {
      setLoadingCredits(false);
    }
  }, [isTauri, selectedCreditCustomer]);

  const loadCustomerHistory = useCallback(async (customerId: string) => {
    setLoadingLedger(true);
    try {
      if (isTauri) {
        const history = await invoke<CustomerLedgerHistory>("get_customer_ledger_history", { customerId });
        setSelectedCreditCustomer(history.customer);
        setCustomerLedgerHistory(history.entries);
      } else {
        const cust = creditCustomers.find((c) => c.id === customerId) || null;
        setSelectedCreditCustomer(cust);
        setCustomerLedgerHistory([
          {
            id: "cleg_sample_1",
            customerId,
            entryType: "SALE_CREDIT",
            amountCents: cust ? cust.currentCreditCents : 45000,
            balanceBeforeCents: 0,
            balanceAfterCents: cust ? cust.currentCreditCents : 45000,
            referenceType: "SALE",
            referenceId: "INV-1001",
            notes: "Previous credit sale grocery supplies",
            userId: "usr_admin",
            userName: "Admin",
            createdAt: new Date().toISOString(),
          },
        ]);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to load customer ledger history", "error");
    } finally {
      setLoadingLedger(false);
    }
  }, [isTauri, creditCustomers]);

  const handleSelectCustomer = (customer: CustomerCreditItem) => {
    setSelectedCreditCustomer(customer);
    void loadCustomerHistory(customer.id);
  };

  const handleOpenPaymentModal = () => {
    if (!selectedCreditCustomer) return;
    if (selectedCreditCustomer.currentCreditCents <= 0) {
      showNotification("Customer has no outstanding credit due to settle.", "info");
      return;
    }
    setPaymentAmountRupees((selectedCreditCustomer.currentCreditCents / 100).toFixed(2));
    setPaymentMethod("CASH");
    setPaymentNotes("");
    setShowRecordPaymentModal(true);
  };

  const handlePreparePayment = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!selectedCreditCustomer) return;

    const parsedRupees = parseFloat(paymentAmountRupees);
    if (isNaN(parsedRupees) || parsedRupees <= 0) {
      showNotification("Please enter a valid payment amount greater than zero", "error");
      return;
    }

    const amountCents = Math.round(parsedRupees * 100);
    if (amountCents > selectedCreditCustomer.currentCreditCents) {
      showNotification(
        `Payment amount (₹${parsedRupees.toFixed(2)}) cannot exceed outstanding due (₹${(selectedCreditCustomer.currentCreditCents / 100).toFixed(2)})`,
        "error"
      );
      return;
    }

    setPreparingPayment(true);
    try {
      if (isTauri) {
        const quote = await invoke<PreparedCustomerPaymentQuote>("prepare_customer_payment", {
          input: {
            customerId: selectedCreditCustomer.id,
            amountCents,
            paymentMethod,
            notes: paymentNotes.trim() ? paymentNotes.trim() : null,
          } as PrepareCustomerPaymentIpcInput,
        });
        setPreparedCustomerPaymentQuote(quote);
        setShowRecordPaymentModal(false);
      } else {
        const balBefore = selectedCreditCustomer.currentCreditCents;
        const balAfter = balBefore - amountCents;
        setPreparedCustomerPaymentQuote({
          preparationToken: `prep_mock_${Date.now()}`,
          paymentId: `pmt_mock_${Date.now()}`,
          customerId: selectedCreditCustomer.id,
          customerName: selectedCreditCustomer.name,
          customerPhone: selectedCreditCustomer.phone,
          amountCents,
          balanceBeforeCents: balBefore,
          balanceAfterCents: balAfter,
          paymentMethod,
          notes: paymentNotes.trim() ? paymentNotes.trim() : null,
          preparedAt: new Date().toISOString(),
        });
        setShowRecordPaymentModal(false);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to prepare payment quote", "error");
    } finally {
      setPreparingPayment(false);
    }
  };

  const handleConfirmPayment = async () => {
    if (!preparedCustomerPaymentQuote) return;
    setConfirmingPayment(true);
    try {
      if (isTauri) {
        const receipt = await invoke<CustomerPaymentReceipt>("confirm_customer_payment", {
          input: {
            preparationToken: preparedCustomerPaymentQuote.preparationToken,
          } as ConfirmCustomerPaymentIpcInput,
        });
        setCompletedCustomerPaymentReceipt(receipt);
        setPreparedCustomerPaymentQuote(null);
        showNotification(`Payment of ₹${(receipt.amountCents / 100).toFixed(2)} recorded and settled!`, "success");
        void loadCustomerCredits();
        void loadCustomerHistory(receipt.customerId);
      } else {
        const receipt: CustomerPaymentReceipt = {
          paymentId: preparedCustomerPaymentQuote.paymentId,
          customerId: preparedCustomerPaymentQuote.customerId,
          customerName: preparedCustomerPaymentQuote.customerName,
          customerPhone: preparedCustomerPaymentQuote.customerPhone,
          amountCents: preparedCustomerPaymentQuote.amountCents,
          balanceBeforeCents: preparedCustomerPaymentQuote.balanceBeforeCents,
          balanceAfterCents: preparedCustomerPaymentQuote.balanceAfterCents,
          paymentMethod: preparedCustomerPaymentQuote.paymentMethod,
          notes: preparedCustomerPaymentQuote.notes,
          paymentDate: new Date().toISOString(),
          createdAt: new Date().toISOString(),
        };
        setCompletedCustomerPaymentReceipt(receipt);
        setPreparedCustomerPaymentQuote(null);
        showNotification("Payment recorded in preview mode!", "success");
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Payment confirmation failed", "error");
    } finally {
      setConfirmingPayment(false);
    }
  };

  // ==============================================================================================
  // BUILD 09: CUSTOMER ORDERS OPERATIONS
  // ==============================================================================================

  const loadCustomerOrders = useCallback(async (filter?: string) => {
    setOrdersLoading(true);
    try {
      if (isTauri) {
        const filterVal = (filter || ordersFilter) === "ALL" ? null : (filter || ordersFilter);
        const data = await invoke<CustomerOrderSummaryItem[]>("get_customer_orders_summary", {
          statusFilter: filterVal,
        });
        setOrdersSummary(data);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to load orders summary", "error");
    } finally {
      setOrdersLoading(false);
    }
  }, [isTauri, ordersFilter]);

  const loadOrderDetail = useCallback(async (orderId: string) => {
    setLoadingOrderDetail(true);
    try {
      if (isTauri) {
        const detail = await invoke<CustomerOrderDetail>("get_customer_order_detail", { orderId });
        setSelectedOrderDetail(detail);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to load order detail", "error");
    } finally {
      setLoadingOrderDetail(false);
    }
  }, [isTauri]);

  const loadOrdersFormData = useCallback(async () => {
    setLoadingOrdersFormData(true);
    try {
      if (isTauri) {
        const data = await invoke<CustomerOrdersFormData>("get_customer_orders_form_data");
        setOrdersFormData(data);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to load order form data", "error");
    } finally {
      setLoadingOrdersFormData(false);
    }
  }, [isTauri]);

  const openCreateOrderModal = async () => {
    setEditingOrderId(null);
    setOrderFormCustomerId("");
    setOrderFormNotes("");
    setOrderFormItems([]);
    setShowOrderModal(true);
    void loadOrdersFormData();
  };

  const openEditOrderModal = (order: CustomerOrderDetail) => {
    if (order.status !== "DRAFT") {
      showNotification("Only DRAFT orders can be modified.", "error");
      return;
    }
    setEditingOrderId(order.id);
    setOrderFormCustomerId(order.customerId || "");
    setOrderFormNotes(order.notes || "");
    setOrderFormItems(
      order.items.map((it) => ({
        id: it.id,
        productId: it.productId,
        quantityDisplay: (it.quantity / 1000).toString(),
        quantityMillie: it.quantity,
        notes: it.notes || "",
      }))
    );
    setShowOrderModal(true);
    void loadOrdersFormData();
  };

  const addOrderFormItem = (productId: string) => {
    if (!productId) return;
    setOrderFormItems((prev) => {
      const existing = prev.find((x) => x.productId === productId);
      if (existing) {
        return prev.map((x) =>
          x.productId === productId
            ? {
                ...x,
                quantityMillie: x.quantityMillie + 1000,
                quantityDisplay: ((x.quantityMillie + 1000) / 1000).toString(),
              }
            : x
        );
      }
      return [
        ...prev,
        {
          id: `item_${Date.now()}_${Math.random()}`,
          productId,
          quantityDisplay: "1",
          quantityMillie: 1000,
          notes: "",
        },
      ];
    });
  };

  const removeOrderFormItem = (itemId: string) => {
    setOrderFormItems((prev) => prev.filter((x) => x.id !== itemId));
  };

  const handleSaveOrder = async () => {
    if (orderFormItems.length === 0) {
      showNotification("Please add at least one item to the order", "error");
      return;
    }
    for (const it of orderFormItems) {
      if (it.quantityMillie <= 0) {
        showNotification("All item quantities must be greater than zero", "error");
        return;
      }
    }
    setSavingOrder(true);
    try {
      if (isTauri) {
        const payloadItems: CustomerOrderItemInput[] = orderFormItems.map((it) => ({
          productId: it.productId,
          quantity: it.quantityMillie,
          notes: it.notes.trim() ? it.notes.trim() : null,
        }));

        if (editingOrderId) {
          const updated = await invoke<CustomerOrderDetail>("update_customer_order", {
            input: {
              orderId: editingOrderId,
              customerId: orderFormCustomerId.trim() ? orderFormCustomerId.trim() : null,
              items: payloadItems,
              notes: orderFormNotes.trim() ? orderFormNotes.trim() : null,
            } as UpdateCustomerOrderIpcInput,
          });
          setSelectedOrderDetail(updated);
          showNotification(`Order ${updated.orderNumber} updated successfully!`, "success");
        } else {
          const created = await invoke<CustomerOrderDetail>("create_customer_order", {
            input: {
              customerId: orderFormCustomerId.trim() ? orderFormCustomerId.trim() : null,
              items: payloadItems,
              notes: orderFormNotes.trim() ? orderFormNotes.trim() : null,
            } as CreateCustomerOrderIpcInput,
          });
          setSelectedOrderId(created.id);
          setSelectedOrderDetail(created);
          showNotification(`Draft order ${created.orderNumber} created!`, "success");
        }
        setShowOrderModal(false);
        void loadCustomerOrders();
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to save order", "error");
    } finally {
      setSavingOrder(false);
    }
  };

  const handleCancelOrder = async (orderId: string) => {
    if (!window.confirm("Are you sure you want to cancel this order? This action cannot be undone.")) {
      return;
    }
    try {
      if (isTauri) {
        await invoke("cancel_customer_order", { orderId });
        showNotification("Order cancelled successfully.", "info");
        void loadCustomerOrders();
        void loadOrderDetail(orderId);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to cancel order", "error");
    }
  };

  const openConvertOrderModal = (order: CustomerOrderDetail) => {
    if (order.status !== "DRAFT") {
      showNotification("Only DRAFT orders can be converted to sales.", "error");
      return;
    }
    setConvertOrderTarget(order);
    setConvertSettlementMode(order.customerId ? "PAID" : "PAID");
    setConvertPaymentMethod("CASH");
    setShowConvertOrderModal(true);
  };

  const handlePrepareConversion = async () => {
    if (!convertOrderTarget) return;
    if (convertSettlementMode === "CREDIT" && !convertOrderTarget.customerId) {
      showNotification("Credit conversion requires a registered customer identity on the order.", "error");
      return;
    }
    setPreparingConversion(true);
    try {
      if (isTauri) {
        const quote = await invoke<PreparedOrderConversionQuote>("prepare_order_conversion", {
          input: {
            orderId: convertOrderTarget.id,
            settlementMode: convertSettlementMode,
            paymentMethod: convertSettlementMode === "PAID" ? convertPaymentMethod : null,
          } as PrepareOrderConversionIpcInput,
        });
        setPreparedOrderConversionQuote(quote);
        setShowConvertOrderModal(false);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to prepare order conversion quote", "error");
    } finally {
      setPreparingConversion(false);
    }
  };

  const handleConfirmConversion = async () => {
    if (!preparedOrderConversionQuote) return;
    setConfirmingConversion(true);
    try {
      if (isTauri) {
        const receipt = await invoke<SaleReceipt>("confirm_order_conversion", {
          input: {
            preparationToken: preparedOrderConversionQuote.preparationToken,
          } as ConfirmOrderConversionIpcInput,
        });
        setCompletedSaleReceipt(receipt);
        setPreparedOrderConversionQuote(null);
        showNotification(`Order converted successfully! Invoice #${receipt.saleNumber}`, "success");
        void loadCustomerOrders();
        if (selectedOrderId) {
          void loadOrderDetail(selectedOrderId);
        }
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Order conversion failed", "error");
    } finally {
      setConfirmingConversion(false);
    }
  };

  const filteredOrders = ordersSummary.filter((order) => {
    const matchesFilter = ordersFilter === "ALL" || order.status === ordersFilter;
    if (!matchesFilter) return false;
    if (!ordersSearchQuery.trim()) return true;
    const q = ordersSearchQuery.toLowerCase();
    return (
      order.orderNumber.toLowerCase().includes(q) ||
      (order.customerName && order.customerName.toLowerCase().includes(q)) ||
      (order.customerPhone && order.customerPhone.includes(q)) ||
      (order.notes && order.notes.toLowerCase().includes(q))
    );
  });

  // ==============================================================================================
  // BUILD 10: RETURNS & STOCK REVERSAL WORKFLOW HANDLERS
  // ==============================================================================================

  const loadReturnsData = useCallback(async (filterType?: ReturnType | null) => {
    setReturnsLoading(true);
    try {
      if (isTauri) {
        const data = await invoke<ReturnSummaryItem[]>("get_returns_summary", {
          typeFilter: filterType === "CUSTOMER_RETURN" ? "CUSTOMER" : filterType === "SUPPLIER_RETURN" ? "SUPPLIER" : null,
        });
        setReturnsSummary(data);
      }
    } catch (err: any) {
      console.error("Failed to load returns summary:", err);
      showNotification("Failed to load returns: " + (err?.toString() || ""), "error");
    } finally {
      setReturnsLoading(false);
    }
  }, [isTauri]);

  const loadReturnDetail = useCallback(async (returnId: string) => {
    setLoadingReturnDetail(true);
    try {
      if (isTauri) {
        const detail = await invoke<ReturnDetail>("get_return_detail", { returnId });
        setSelectedReturnDetail(detail);
      }
    } catch (err: any) {
      console.error("Failed to load return detail:", err);
      showNotification("Failed to load return details: " + (err?.toString() || ""), "error");
    } finally {
      setLoadingReturnDetail(false);
    }
  }, [isTauri]);

  const fetchReturnsFormData = async () => {
    try {
      if (isTauri) {
        const data = await invoke<ReturnsFormData>("get_returns_form_data");
        setReturnsFormData(data);
        return data;
      }
    } catch (err: any) {
      console.error("Failed to load returns form data:", err);
      showNotification("Failed to load products/parties for returns: " + (err?.toString() || ""), "error");
    }
    return null;
  };

  // --- Customer Return Actions ---
  const openCustomerReturnModal = async () => {
    await fetchReturnsFormData();
    setCustReturnCustomerId("");
    setCustReturnItems([]);
    setCustReturnRefundMethod("CASH");
    setShowCustomerReturnModal(true);
  };

  const addCustomerReturnProduct = (productId: string) => {
    if (!returnsFormData) return;
    const prod = returnsFormData.products.find((p) => p.id === productId);
    if (!prod) return;

    const existingIdx = custReturnItems.findIndex((i) => i.productId === prod.id);
    if (existingIdx >= 0) {
      const updated = [...custReturnItems];
      const it = updated[existingIdx]!;
      const step = it.productType === "LOOSE" ? 500 : 1000;
      it.quantityMillie += step;
      it.quantityDisplay = (it.quantityMillie / 1000).toString();
      setCustReturnItems(updated);
    } else {
      const initialQty = prod.productType === "LOOSE" ? 500 : 1000;
      setCustReturnItems([
        ...custReturnItems,
        {
          id: `cr_${Date.now()}_${Math.random().toString(36).substring(2, 7)}`,
          productId: prod.id,
          productName: prod.name,
          productType: prod.productType,
          unit: prod.unit,
          quantityDisplay: (initialQty / 1000).toString(),
          quantityMillie: initialQty,
          unitPriceCents: prod.sellingPriceCents,
          availableStockMillie: prod.currentQuantity,
        },
      ]);
    }
  };

  const handleCustomerReturnQtyChange = (itemId: string, val: string) => {
    const updated = custReturnItems.map((item) => {
      if (item.id !== itemId) return item;
      const num = parseFloat(val);
      const millie = isNaN(num) || num < 0 ? 0 : Math.round(num * 1000);
      return {
        ...item,
        quantityDisplay: val,
        quantityMillie: millie,
      };
    });
    setCustReturnItems(updated);
  };

  const removeCustomerReturnItem = (itemId: string) => {
    setCustReturnItems(custReturnItems.filter((i) => i.id !== itemId));
  };

  const handlePrepareCustomerReturn = async () => {
    if (custReturnItems.length === 0) {
      showNotification("Please add at least one product to return.", "error");
      return;
    }
    for (const item of custReturnItems) {
      if (item.quantityMillie <= 0) {
        showNotification(`Quantity for ${item.productName} must be greater than zero.`, "error");
        return;
      }
    }

    setPreparingCustReturn(true);
    try {
      if (isTauri) {
        const input: PrepareCustomerReturnIpcInput = {
          customerId: custReturnCustomerId.trim() ? custReturnCustomerId : null,
          items: custReturnItems.map((it) => ({
            productId: it.productId,
            quantity: it.quantityMillie,
          })),
          reason: "Customer goods return",
          refundPaymentMethod: custReturnRefundMethod,
        };
        const quote = await invoke<PreparedCustomerReturnQuote>("prepare_customer_return", { input });
        setPreparedCustomerReturnQuote(quote);
        setShowCustomerReturnModal(false);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to prepare customer return", "error");
    } finally {
      setPreparingCustReturn(false);
    }
  };

  const handleConfirmCustomerReturn = async () => {
    if (!preparedCustomerReturnQuote) return;
    setConfirmingCustReturn(true);
    try {
      if (isTauri) {
        const receipt = await invoke<CustomerReturnReceipt>("confirm_customer_return", {
          input: {
            preparationToken: preparedCustomerReturnQuote.preparationToken,
          } as ConfirmCustomerReturnIpcInput,
        });
        setCompletedCustomerReturnReceipt(receipt);
        setPreparedCustomerReturnQuote(null);
        showNotification(`Customer return confirmed! Receipt #${receipt.returnNumber}`, "success");
        void loadReturnsData(returnsFilter === "ALL" ? null : returnsFilter);
        void loadInitialData();
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Customer return confirmation failed", "error");
    } finally {
      setConfirmingCustReturn(false);
    }
  };

  // --- Supplier Return Actions ---
  const openSupplierReturnModal = async () => {
    const data = await fetchReturnsFormData();
    if (data && data.suppliers.length > 0) {
      setSuppReturnSupplierId(data.suppliers[0]!.id);
    } else {
      setSuppReturnSupplierId("");
    }
    setSuppReturnItems([]);
    setSuppReturnReason("");
    setShowSupplierReturnModal(true);
  };

  const addSupplierReturnProduct = (productId: string) => {
    if (!returnsFormData) return;
    const prod = returnsFormData.products.find((p) => p.id === productId);
    if (!prod) return;

    if (prod.currentQuantity <= 0) {
      showNotification(`"${prod.name}" has zero inventory stock. Cannot return to supplier.`, "error");
      return;
    }

    const existingIdx = suppReturnItems.findIndex((i) => i.productId === prod.id);
    if (existingIdx >= 0) {
      const updated = [...suppReturnItems];
      const it = updated[existingIdx]!;
      const step = it.productType === "LOOSE" ? 500 : 1000;
      if (it.quantityMillie + step > prod.currentQuantity) {
        showNotification(`Cannot exceed current stock (${(prod.currentQuantity / 1000).toFixed(prod.productType === "LOOSE" ? 2 : 0)} ${prod.unit})`, "error");
        return;
      }
      it.quantityMillie += step;
      it.quantityDisplay = (it.quantityMillie / 1000).toString();
      setSuppReturnItems(updated);
    } else {
      const initialQty = Math.min(prod.currentQuantity, prod.productType === "LOOSE" ? 500 : 1000);
      setSuppReturnItems([
        ...suppReturnItems,
        {
          id: `sr_${Date.now()}_${Math.random().toString(36).substring(2, 7)}`,
          productId: prod.id,
          productName: prod.name,
          productType: prod.productType,
          unit: prod.unit,
          quantityDisplay: (initialQty / 1000).toString(),
          quantityMillie: initialQty,
          unitPriceCents: prod.costPriceCents,
          availableStockMillie: prod.currentQuantity,
        },
      ]);
    }
  };

  const handleSupplierReturnQtyChange = (itemId: string, val: string) => {
    const updated = suppReturnItems.map((item) => {
      if (item.id !== itemId) return item;
      const num = parseFloat(val);
      const millie = isNaN(num) || num < 0 ? 0 : Math.round(num * 1000);
      return {
        ...item,
        quantityDisplay: val,
        quantityMillie: millie,
      };
    });
    setSuppReturnItems(updated);
  };

  const removeSupplierReturnItem = (itemId: string) => {
    setSuppReturnItems(suppReturnItems.filter((i) => i.id !== itemId));
  };

  const handlePrepareSupplierReturn = async () => {
    if (!suppReturnSupplierId) {
      showNotification("Please select a supplier.", "error");
      return;
    }
    if (suppReturnItems.length === 0) {
      showNotification("Please add at least one product to return to the supplier.", "error");
      return;
    }
    for (const item of suppReturnItems) {
      if (item.quantityMillie <= 0) {
        showNotification(`Quantity for ${item.productName} must be greater than zero.`, "error");
        return;
      }
      if (item.quantityMillie > item.availableStockMillie) {
        showNotification(`Requested return for "${item.productName}" exceeds available inventory (${(item.availableStockMillie / 1000).toFixed(item.productType === "LOOSE" ? 2 : 0)} ${item.unit}).`, "error");
        return;
      }
    }

    setPreparingSuppReturn(true);
    try {
      if (isTauri) {
        const input: PrepareSupplierReturnIpcInput = {
          supplierId: suppReturnSupplierId,
          items: suppReturnItems.map((it) => ({
            productId: it.productId,
            quantity: it.quantityMillie,
          })),
          reason: suppReturnReason.trim() ? suppReturnReason.trim() : "Supplier goods return",
        };
        const quote = await invoke<PreparedSupplierReturnQuote>("prepare_supplier_return", { input });
        setPreparedSupplierReturnQuote(quote);
        setShowSupplierReturnModal(false);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to prepare supplier return", "error");
    } finally {
      setPreparingSuppReturn(false);
    }
  };

  const handleConfirmSupplierReturn = async () => {
    if (!preparedSupplierReturnQuote) return;
    setConfirmingSuppReturn(true);
    try {
      if (isTauri) {
        const receipt = await invoke<SupplierReturnReceipt>("confirm_supplier_return", {
          input: {
            preparationToken: preparedSupplierReturnQuote.preparationToken,
          } as ConfirmSupplierReturnIpcInput,
        });
        setCompletedSupplierReturnReceipt(receipt);
        setPreparedSupplierReturnQuote(null);
        showNotification(`Supplier return confirmed! Receipt #${receipt.returnNumber}`, "success");
        void loadReturnsData(returnsFilter === "ALL" ? null : returnsFilter);
        void loadInitialData();
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Supplier return confirmation failed", "error");
    } finally {
      setConfirmingSuppReturn(false);
    }
  };

  const filteredReturns = returnsSummary.filter((ret) => {
    const matchesFilter = returnsFilter === "ALL" || ret.returnType === returnsFilter;
    if (!matchesFilter) return false;
    if (!returnsSearchQuery.trim()) return true;
    const q = returnsSearchQuery.toLowerCase();
    return (
      ret.returnNumber.toLowerCase().includes(q) ||
      (ret.counterpartyName && ret.counterpartyName.toLowerCase().includes(q)) ||
      (ret.reason && ret.reason.toLowerCase().includes(q))
    );
  });

  // ==============================================================================================
  // BUILD 11: STOCK CORRECTIONS HANDLERS
  // ==============================================================================================

  const loadCorrectionsData = useCallback(async () => {
    setLoadingCorrections(true);
    try {
      if (isTauri) {
        const formData = await invoke<StockCorrectionsFormData>("get_stock_corrections_form_data");
        setCorrectionProducts(formData.products);
        const summary = await invoke<StockCorrectionsSummary>("get_stock_corrections_summary");
        setCorrectionHistory(summary.corrections);
      }
    } catch (err: any) {
      showNotification(`Failed to load corrections: ${String(err)}`, "error");
    } finally {
      setLoadingCorrections(false);
    }
  }, [isTauri]);

  const handlePrepareStockCorrection = async () => {
    if (!selectedCorrectionProductId) {
      showNotification("Please select a product for stock adjustment", "error");
      return;
    }
    const product = correctionProducts.find((p) => p.id === selectedCorrectionProductId);
    if (!product) {
      showNotification("Selected product not found", "error");
      return;
    }

    let deltaMillie = 0;
    if (adjustmentMode === "PHYSICAL_COUNT") {
      if (!physicalCountInput.trim() || isNaN(Number(physicalCountInput)) || Number(physicalCountInput) < 0) {
        showNotification("Please enter a valid non-negative physical count", "error");
        return;
      }
      const targetMillie = Math.round(Number(physicalCountInput) * 1000);
      deltaMillie = targetMillie - product.currentQuantity;
    } else {
      if (!manualDeltaInput.trim() || isNaN(Number(manualDeltaInput)) || Number(manualDeltaInput) === 0) {
        showNotification("Please enter a non-zero adjustment delta", "error");
        return;
      }
      deltaMillie = Math.round(Number(manualDeltaInput) * 1000);
    }

    if (deltaMillie === 0) {
      showNotification("Physical count matches current inventory. No adjustment delta required.", "info");
      return;
    }

    if (product.currentQuantity + deltaMillie < 0) {
      showNotification(
        `Adjustment would cause negative inventory (${((product.currentQuantity + deltaMillie) / 1000).toFixed(3)}). Minimum possible stock is 0.`,
        "error"
      );
      return;
    }

    if (!correctionNote.trim()) {
      showNotification("An audit note explaining the discrepancy is required", "error");
      return;
    }

    setPreparingCorrection(true);
    try {
      if (isTauri) {
        const payload: PrepareStockCorrectionIpcInput = {
          productId: selectedCorrectionProductId,
          quantityChange: deltaMillie,
          reason: correctionReason,
          note: correctionNote.trim(),
        };
        const quote = await invoke<PreparedStockCorrectionQuote>("prepare_stock_correction", { input: payload });
        setPreparedCorrectionQuote(quote);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Stock correction preparation failed", "error");
    } finally {
      setPreparingCorrection(false);
    }
  };

  const handleConfirmStockCorrection = async () => {
    if (!preparedCorrectionQuote) return;
    setConfirmingCorrection(true);
    try {
      if (isTauri) {
        const payload: ConfirmStockCorrectionIpcInput = {
          preparationToken: preparedCorrectionQuote.preparationToken,
        };
        const receipt = await invoke<StockCorrectionReceipt>("confirm_stock_correction", { input: payload });
        setCompletedCorrectionReceipt(receipt);
        setPreparedCorrectionQuote(null);
        setSelectedCorrectionProductId("");
        setPhysicalCountInput("");
        setManualDeltaInput("");
        setCorrectionNote("");
        showNotification(`Stock adjustment committed! Receipt #${receipt.correctionId.slice(0, 16)}`, "success");
        void loadCorrectionsData();
        void loadInitialData();
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Stock correction confirmation failed", "error");
    } finally {
      setConfirmingCorrection(false);
    }
  };

  const filteredCorrectionHistory = correctionHistory.filter((c) => {
    if (!correctionsSearchQuery.trim()) return true;
    const q = correctionsSearchQuery.toLowerCase();
    return (
      c.productName.toLowerCase().includes(q) ||
      c.reason.toLowerCase().includes(q) ||
      c.note.toLowerCase().includes(q) ||
      c.adminUsername.toLowerCase().includes(q)
    );
  });

  // ==============================================================================================
  // BUILD 12: OFFLINE SYNC & BACKUP HANDLERS
  // ==============================================================================================

  const loadBackupStatus = useCallback(async () => {
    setLoadingBackups(true);
    try {
      if (isTauri) {
        const res = await invoke<BackupStatusDto>("get_backup_status");
        setBackupSettings(res.settings);
        setBackupsList(res.backups);
      }
    } catch (err: any) {
      showNotification(`Failed to load backups: ${String(err)}`, "error");
    } finally {
      setLoadingBackups(false);
    }
  }, [isTauri]);

  const handleToggleAutoBackup = async (newEnabled: boolean) => {
    setTogglingAutoBackup(true);
    try {
      if (isTauri) {
        const updated = await invoke<BackupSettings>("toggle_auto_backup", {
          input: { enabled: newEnabled } as ToggleAutoBackupInput,
        });
        setBackupSettings(updated);
        showNotification(
          `Auto-backup ${updated.autoBackupEnabled ? "enabled (retains 7 latest backups)" : "disabled"}`,
          "success"
        );
      } else {
        setBackupSettings((prev) => (prev ? { ...prev, autoBackupEnabled: newEnabled } : null));
        showNotification(`Auto-backup ${newEnabled ? "enabled" : "disabled"} (preview)`, "info");
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to update auto backup setting", "error");
    } finally {
      setTogglingAutoBackup(false);
    }
  };

  const handleCreateManualBackup = async () => {
    setCreatingBackup(true);
    try {
      if (isTauri) {
        const meta = await invoke<BackupMetadata>("create_manual_backup", {
          input: { note: manualBackupNote.trim() || undefined } as CreateManualBackupInput,
        });
        showNotification(`Manual backup snapshot created: ${meta.fileName}`, "success");
        setShowCreateBackupModal(false);
        setManualBackupNote("");
        void loadBackupStatus();
      } else {
        showNotification("Manual backup simulated in preview mode", "success");
        setShowCreateBackupModal(false);
        setManualBackupNote("");
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Failed to create manual backup", "error");
    } finally {
      setCreatingBackup(false);
    }
  };

  const handleValidateBackup = async (backup: BackupMetadata) => {
    setSelectedBackupForAction(backup);
    setValidatingBackup(true);
    setValidationReport(null);
    setShowValidationModal(true);
    try {
      if (isTauri) {
        const report = await invoke<BackupValidationResult>("validate_backup", {
          input: { fileName: backup.fileName } as ValidateBackupInput,
        });
        setValidationReport(report);
      } else {
        setValidationReport({
          isValid: true,
          manifest: null,
          integrityCheckPassed: true,
          foreignKeyCheckPassed: true,
          schemaTablesPassed: true,
          businessInvariantsPassed: true,
          tablesFound: ["businesses", "users", "products", "inventory", "sales", "audit_logs"],
          compatibilityStatus: "COMPATIBLE",
          errors: [],
        });
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Backup validation failed", "error");
    } finally {
      setValidatingBackup(false);
    }
  };

  const handleOpenRestoreModal = async (backup: BackupMetadata) => {
    setRestoreCandidate(backup);
    setRestoreConfirmedCheck(false);
    setShowRestoreModal(true);
    setLoadingRestoreValidation(true);
    setRestoreValidationReport(null);
    try {
      if (isTauri) {
        const report = await invoke<BackupValidationResult>("validate_backup", {
          input: { fileName: backup.fileName } as ValidateBackupInput,
        });
        setRestoreValidationReport(report);
      } else {
        setRestoreValidationReport({
          isValid: true,
          manifest: null,
          integrityCheckPassed: true,
          foreignKeyCheckPassed: true,
          schemaTablesPassed: true,
          businessInvariantsPassed: true,
          tablesFound: ["businesses", "users", "products", "inventory", "sales", "audit_logs"],
          compatibilityStatus: "COMPATIBLE",
          errors: [],
        });
      }
    } catch (err: any) {
      showNotification(`Pre-restore validation error: ${String(err)}`, "error");
    } finally {
      setLoadingRestoreValidation(false);
    }
  };

  const handleExecuteRestore = async () => {
    if (!restoreCandidate) return;
    if (!restoreConfirmedCheck) {
      showNotification("Please check the confirmation box to authorize database restoration", "error");
      return;
    }
    setRestoringBackup(true);
    try {
      if (isTauri) {
        const res = await invoke<RestoreBackupResult>("restore_backup", {
          input: { fileName: restoreCandidate.fileName } as RestoreBackupInput,
        });
        showNotification(
          `Database restored successfully! Verified ${res.totalRecords} records across all tables.`,
          "success"
        );
        setShowRestoreModal(false);
        setRestoreCandidate(null);
        setRestoreConfirmedCheck(false);
        // Requirement 9: Invalidate and reload frontend state so the UI reflects the restored database
        await loadInitialData();
        await loadBackupStatus();
      } else {
        showNotification("Restore simulated in preview mode", "success");
        setShowRestoreModal(false);
        setRestoreCandidate(null);
        setRestoreConfirmedCheck(false);
      }
    } catch (err: any) {
      showNotification(err?.toString() || "Database restoration failed. Active database preserved.", "error");
    } finally {
      setRestoringBackup(false);
    }
  };

  const filteredBackups = backupsList.filter((b) => {
    if (backupFilter === "MANUAL" && b.backupType !== "MANUAL") return false;
    if (backupFilter === "AUTO" && b.backupType !== "AUTO") return false;
    if (!backupSearchQuery.trim()) return true;
    const q = backupSearchQuery.toLowerCase();
    return (
      b.fileName.toLowerCase().includes(q) ||
      b.backupType.toLowerCase().includes(q) ||
      b.createdAt.toLowerCase().includes(q) ||
      b.checksumSha256.toLowerCase().includes(q)
    );
  });

  // ==============================================================================================
  // RENDER UI
  // ==============================================================================================

  return (
    <div className="viewport-container">
      <div className="ambient-grid" />

      {/* Notification Toast */}
      {feedback && (
        <div className={`notification-toast toast-${feedback.type}`}>
          {feedback.message}
        </div>
      )}

      {/* Top Header & Navigation */}
      <header className="app-top-header">
        <div className="header-brand">
          <div className="brand-logo-badge">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
              <path strokeLinecap="round" strokeLinejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
            </svg>
          </div>
          <div>
            <div className="header-title">Merchant OS</div>
            <div className="header-subtitle">Local Sovereign POS & Business Engine</div>
          </div>
        </div>

        {/* Navigation Tabs */}
        <div className="header-nav-tabs">
          <button
            className={`nav-tab-btn ${activeTab === "pos" ? "active" : ""}`}
            onClick={() => setActiveTab("pos")}
          >
            <span>Sales / POS</span>
            <span className="tab-badge">{posLoading ? "..." : "F2"}</span>
          </button>
          <button
            className={`nav-tab-btn ${activeTab === "purchases" ? "active" : ""}`}
            onClick={() => setActiveTab("purchases")}
          >
            <span>Purchases</span>
            {loadingPurchases && <span className="tab-badge">...</span>}
          </button>
          <button
            className={`nav-tab-btn ${activeTab === "credits" ? "active" : ""}`}
            onClick={() => {
              setActiveTab("credits");
              void loadCustomerCredits();
            }}
          >
            <span>Customer Credits (Khata)</span>
            {creditCustomers.filter((c) => c.currentCreditCents > 0).length > 0 && (
              <span className="tab-badge" style={{ background: "rgba(245, 158, 11, 0.2)", color: "#fbbf24" }}>
                {creditCustomers.filter((c) => c.currentCreditCents > 0).length} Due
              </span>
            )}
          </button>
          <button
            className={`nav-tab-btn ${activeTab === "orders" ? "active" : ""}`}
            onClick={() => {
              setActiveTab("orders");
              void loadCustomerOrders();
            }}
          >
            <span>Orders</span>
            {ordersSummary.filter((o) => o.status === "DRAFT").length > 0 && (
              <span className="tab-badge" style={{ background: "rgba(59, 130, 246, 0.2)", color: "#60a5fa" }}>
                {ordersSummary.filter((o) => o.status === "DRAFT").length} Draft
              </span>
            )}
          </button>
          <button
            className={`nav-tab-btn ${activeTab === "returns" ? "active" : ""}`}
            onClick={() => {
              setActiveTab("returns");
              void loadReturnsData();
            }}
          >
            <span>Returns & Stock Reversal</span>
            {returnsSummary.length > 0 && (
              <span className="tab-badge" style={{ background: "rgba(16, 185, 129, 0.2)", color: "#34d399" }}>
                {returnsSummary.length}
              </span>
            )}
          </button>
          <button
            className={`nav-tab-btn ${activeTab === "corrections" ? "active" : ""}`}
            onClick={() => {
              setActiveTab("corrections");
              void loadCorrectionsData();
            }}
          >
            <span>Stock Adjustments</span>
            {correctionHistory.length > 0 && (
              <span className="tab-badge" style={{ background: "rgba(245, 158, 11, 0.2)", color: "#fbbf24" }}>
                {correctionHistory.length}
              </span>
            )}
          </button>
          <button
            className={`nav-tab-btn ${activeTab === "backups" ? "active" : ""}`}
            onClick={() => {
              setActiveTab("backups");
              void loadBackupStatus();
            }}
          >
            <span>Backups & Sync</span>
            {backupsList.length > 0 && (
              <span className="tab-badge" style={{ background: "rgba(99, 102, 241, 0.2)", color: "#a5b4fc" }}>
                {backupsList.length}
              </span>
            )}
          </button>
        </div>

        {/* System Telemetry Badges */}
        <div className="header-status-group">
          <div className="build-badge">Merchant OS v0.1.0</div>
          <div className="status-pill status-pill-sqlite">
            <span className="pulse-dot" />
            <span>{systemStatus.database}</span>
          </div>
          <div className="status-pill status-pill-offline">
            <span>OFFLINE-FIRST</span>
          </div>
        </div>
      </header>

      {/* ======================================================================================== */}
      {/* 1. SALES / POS WORKSPACE */}
      {/* ======================================================================================== */}
      {activeTab === "pos" && (
        <main className="purchase-workspace">
          <div className="purchase-layout-grid">
            {/* Left Column: Barcode Scan, Catalog Shelf & Cart Table */}
            <section className="panel-card">
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Point of Sale (POS) Checkout</h2>
                  <p className="panel-desc">Scan barcode or select items below. Press F2 to review & checkout.</p>
                </div>
              </div>

              {/* Barcode Search Form */}
              <form onSubmit={handlePosBarcodeSubmit} className="barcode-search-form" style={{ marginBottom: "0.75rem" }}>
                <input
                  type="text"
                  placeholder="Scan barcode or type product name... [Enter to add]"
                  value={posBarcode}
                  onChange={(e) => setPosBarcode(e.target.value)}
                  className="input-field barcode-input"
                  autoFocus
                />
                <button type="submit" className="btn btn-secondary">
                  Add Item
                </button>
              </form>

              {/* Quick Products Shelf Chips */}
              <div className="catalog-shelf-container">
                {posProducts.map((p) => {
                  const stockDisplay = (p.currentQuantity / 1000).toFixed(p.productType === "LOOSE" ? 2 : 0);
                  const isLow = p.currentQuantity <= 5000;
                  const isOut = p.currentQuantity <= 0;
                  return (
                    <div
                      key={p.id}
                      className="catalog-chip"
                      onClick={() => addProductToPos(p)}
                      title={`Add ${p.name}`}
                    >
                      <span className="chip-title">{p.name}</span>
                      <div className="chip-meta">
                        <span className="chip-price">₹{(p.sellingPriceCents / 100).toFixed(2)}</span>
                        <span className={`chip-stock ${isOut ? "stock-out" : isLow ? "stock-low" : "stock-in"}`}>
                          {isOut ? "OUT" : `${stockDisplay} ${p.unit}`}
                        </span>
                      </div>
                    </div>
                  );
                })}
              </div>

              {/* POS Cart Items Table */}
              <div className="items-table-container">
                <table className="items-table">
                  <thead>
                    <tr>
                      <th style={{ width: "42%" }}>Product & Stock</th>
                      <th style={{ width: "20%" }}>Quantity</th>
                      <th style={{ width: "18%" }}>Price (₹)</th>
                      <th style={{ width: "14%" }}>Total (₹)</th>
                      <th style={{ width: "6%", textAlign: "center" }}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {posCart.length === 0 ? (
                      <tr>
                        <td colSpan={5} className="empty-table-state">
                          <div className="empty-text">Cart is empty</div>
                          <div className="empty-hint">Scan a barcode or click a product chip above</div>
                        </td>
                      </tr>
                    ) : (
                      posCart.map((item) => {
                        const lineTotal = ((item.quantityMillie * item.sellingPriceCents) / 100000).toFixed(2);
                        const stockAvailableDisplay = (item.availableStockMillie / 1000).toFixed(item.productType === "LOOSE" ? 2 : 0);
                        const isExceeded = item.quantityMillie > item.availableStockMillie;

                        return (
                          <tr key={item.id} className={isExceeded ? "row-error" : ""}>
                            <td>
                              <div className="item-name-cell">
                                <span className="item-name">{item.productName}</span>
                                <div className="item-badges">
                                  <span className="type-badge">{item.productType}</span>
                                  <span className={`stock-badge ${isExceeded ? "badge-danger" : ""}`}>
                                    Stock: {stockAvailableDisplay} {item.unit}
                                  </span>
                                </div>
                              </div>
                            </td>
                            <td>
                              <div className="qty-input-group">
                                <input
                                  type="number"
                                  step={item.productType === "LOOSE" ? "0.05" : "1"}
                                  min="0"
                                  value={item.quantityDisplay}
                                  onChange={(e) => handlePosQuantityChange(item.id, e.target.value)}
                                  className={`qty-input ${isExceeded ? "input-error" : ""}`}
                                />
                                <span className="qty-unit">{item.unit}</span>
                              </div>
                            </td>
                            <td>
                              <span className="readonly-price">
                                ₹{(item.sellingPriceCents / 100).toFixed(2)}
                              </span>
                            </td>
                            <td>
                              <span className="line-total">₹{lineTotal}</span>
                            </td>
                            <td style={{ textAlign: "center" }}>
                              <button
                                type="button"
                                className="delete-row-btn"
                                onClick={() => removePosItem(item.id)}
                                title="Remove item"
                              >
                                &times;
                              </button>
                            </td>
                          </tr>
                        );
                      })
                    )}
                  </tbody>
                </table>
              </div>
            </section>

            {/* Right Column: Settlement & Checkout Card */}
            <section className="panel-card settlement-card">
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Cart Settlement</h2>
                  <p className="panel-desc">Strict binary settlement: PAID vs CREDIT</p>
                </div>
              </div>

              {/* Big Cart Total */}
              <div className="total-display-banner" style={{ marginBottom: "1.25rem" }}>
                <span className="total-caption">Total Cart Amount</span>
                <span className="total-figure">₹{(posCartTotalCents / 100).toFixed(2)}</span>
                <span className="total-items-badge">{posCart.length} item{posCart.length === 1 ? "" : "s"} in cart</span>
              </div>

              {/* Settlement Mode Pill Switch */}
              <div className="settlement-mode-switch">
                <button
                  type="button"
                  className={`mode-btn ${posSettlementMode === "PAID" ? "active-paid" : ""}`}
                  onClick={() => setPosSettlementMode("PAID")}
                >
                  <span>PAID (Full)</span>
                </button>
                <button
                  type="button"
                  className={`mode-btn ${posSettlementMode === "CREDIT" ? "active-credit" : ""}`}
                  onClick={() => setPosSettlementMode("CREDIT")}
                >
                  <span>CREDIT (Khata)</span>
                </button>
              </div>

              {/* Mode-Specific Settlement Form */}
              <div className="settlement-controls" style={{ flex: 1, display: "flex", flexDirection: "column", gap: "1rem" }}>
                {posSettlementMode === "PAID" ? (
                  <>
                    <div className="form-group">
                      <label className="field-label">Payment Method</label>
                      <select
                        value={posPaymentMethod}
                        onChange={(e) => setPosPaymentMethod(e.target.value as PaymentMethod)}
                        className="select-field"
                      >
                        <option value="CASH">CASH (Physical Currency)</option>
                        <option value="UPI">UPI (GPay / PhonePe / Paytm)</option>
                        <option value="CARD">Debit / Credit Card</option>
                        <option value="BANK_TRANSFER">Bank IMPS / NEFT</option>
                        <option value="OTHER">Other Method</option>
                      </select>
                    </div>

                    <div className="form-group">
                      <label className="field-label">
                        Customer Account <span className="label-optional">(Optional)</span>
                      </label>
                      <div className="supplier-select-row">
                        <select
                          value={posSelectedCustomerId}
                          onChange={(e) => setPosSelectedCustomerId(e.target.value)}
                          className="select-field"
                        >
                          <option value="">Walk-in Customer (No Khata)</option>
                          {posCustomers.map((c) => (
                            <option key={c.id} value={c.id}>
                              {c.name} {c.phone ? `(${c.phone})` : ""}
                            </option>
                          ))}
                        </select>
                        <button
                          type="button"
                          className="btn-icon"
                          onClick={() => setShowAddCustomerModal(true)}
                          title="Register New Customer"
                        >
                          +
                        </button>
                      </div>
                    </div>
                  </>
                ) : (
                  <>
                    <div className="form-group">
                      <label className="field-label" style={{ color: "var(--accent-amber)" }}>
                        Customer Khata <span className="label-required">* Mandatory</span>
                      </label>
                      <div className="supplier-select-row">
                        <select
                          value={posSelectedCustomerId}
                          onChange={(e) => setPosSelectedCustomerId(e.target.value)}
                          className="select-field"
                          style={{ borderColor: "rgba(245, 158, 11, 0.4)" }}
                        >
                          <option value="">-- Select Customer for Credit Due --</option>
                          {posCustomers.map((c) => (
                            <option key={c.id} value={c.id}>
                              {c.name} {c.phone ? `(${c.phone})` : ""} — Due: ₹{(c.currentBalanceCents / 100).toFixed(2)}
                            </option>
                          ))}
                        </select>
                        <button
                          type="button"
                          className="btn-icon"
                          onClick={() => setShowAddCustomerModal(true)}
                          title="Register New Customer"
                        >
                          +
                        </button>
                      </div>
                    </div>

                    <div className="alert-box-info" style={{ background: "rgba(245, 158, 11, 0.1)", borderColor: "var(--accent-amber)", color: "#fde68a" }}>
                      <strong>Credit Sale Notice:</strong> ₹{(posCartTotalCents / 100).toFixed(2)} will be debited to the customer's ledger. Payment method is disabled.
                    </div>
                  </>
                )}
              </div>

              {/* Action Buttons */}
              <div className="panel-actions" style={{ marginTop: "auto", display: "flex", gap: "0.75rem" }}>
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setPosCart([])}
                  disabled={posCart.length === 0}
                >
                  Clear Cart
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={handlePrepareSale}
                  disabled={posCart.length === 0 || preparingSale}
                  style={{ flex: 1 }}
                >
                  {preparingSale ? "Calculating..." : "Review & Checkout (F2)"}
                </button>
              </div>
            </section>
          </div>
        </main>
      )}

      {/* ======================================================================================== */}
      {/* 2. PURCHASES WORKSPACE */}
      {/* ======================================================================================== */}
      {activeTab === "purchases" && (
        <main className="purchase-workspace">
          <div className="purchase-layout-grid">
            {/* Left Column: Supplier & Items */}
            <section className="panel-card">
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Purchase Order Details</h2>
                  <p className="panel-desc">Record inbound goods, inventory restock, and supplier payable.</p>
                </div>
              </div>

              {/* Supplier Selector */}
              <div className="form-group" style={{ marginBottom: "1.25rem" }}>
                <label className="field-label">Supplier Entity</label>
                <div className="supplier-select-row">
                  <select
                    value={selectedSupplierId}
                    onChange={(e) => setSelectedSupplierId(e.target.value)}
                    className="select-field"
                  >
                    {suppliers.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name} {s.phone ? `(${s.phone})` : ""} — Outstanding: ₹{(s.currentOutstandingCents / 100).toFixed(2)}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={() => setShowAddSupplierModal(true)}
                  >
                    + New Supplier
                  </button>
                </div>
              </div>

              {/* Barcode Search Form */}
              <form onSubmit={handlePurchaseBarcodeSubmit} className="barcode-search-form" style={{ marginBottom: "1rem" }}>
                <input
                  type="text"
                  placeholder="Scan barcode or search product..."
                  value={purchaseBarcode}
                  onChange={(e) => setPurchaseBarcode(e.target.value)}
                  className="input-field barcode-input"
                />
                <button type="submit" className="btn btn-secondary">
                  Add Item
                </button>
              </form>

              {/* Products Fast Shelf */}
              <div className="catalog-shelf-container">
                {purchaseProducts.map((p) => (
                  <div
                    key={p.id}
                    className="catalog-chip"
                    onClick={() => addProductToPurchase(p)}
                  >
                    <span className="chip-title">{p.name}</span>
                    <div className="chip-meta">
                      <span className="chip-price">Cost: ₹{(p.costPriceCents / 100).toFixed(2)}</span>
                    </div>
                  </div>
                ))}
              </div>

              {/* Purchase Items Table */}
              <div className="items-table-container">
                <table className="items-table">
                  <thead>
                    <tr>
                      <th style={{ width: "38%" }}>Product</th>
                      <th style={{ width: "22%" }}>Quantity</th>
                      <th style={{ width: "22%" }}>Buying Price (₹)</th>
                      <th style={{ width: "12%" }}>Total (₹)</th>
                      <th style={{ width: "6%", textAlign: "center" }}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {purchaseItems.length === 0 ? (
                      <tr>
                        <td colSpan={5} className="empty-table-state">
                          <div className="empty-text">No items in purchase order</div>
                          <div className="empty-hint">Scan barcode or click a product above</div>
                        </td>
                      </tr>
                    ) : (
                      purchaseItems.map((item) => {
                        const lineTotal = ((item.quantityMillie * item.unitCostCents) / 100000).toFixed(2);
                        return (
                          <tr key={item.id}>
                            <td>
                              <div className="item-name-cell">
                                <span className="item-name">{item.productName}</span>
                                <span className="type-badge">{item.productType}</span>
                              </div>
                            </td>
                            <td>
                              <div className="qty-input-group">
                                <input
                                  type="number"
                                  step="0.001"
                                  min="0.001"
                                  value={item.quantityDisplay}
                                  onChange={(e) => handlePurchaseQuantityChange(item.id, e.target.value)}
                                  className="qty-input"
                                />
                                <span className="qty-unit">{item.unit}</span>
                              </div>
                            </td>
                            <td>
                              <input
                                type="number"
                                step="0.01"
                                min="0.01"
                                value={item.unitCostRupees}
                                onChange={(e) => handlePurchaseUnitCostChange(item.id, e.target.value)}
                                className="cost-input"
                              />
                            </td>
                            <td>
                              <span className="line-total">₹{lineTotal}</span>
                            </td>
                            <td style={{ textAlign: "center" }}>
                              <button
                                type="button"
                                className="delete-row-btn"
                                onClick={() => removePurchaseItem(item.id)}
                              >
                                &times;
                              </button>
                            </td>
                          </tr>
                        );
                      })
                    )}
                  </tbody>
                </table>
              </div>
            </section>

            {/* Right Column: Purchase Settlement Card */}
            <section className="panel-card settlement-card">
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Payment & Ledger</h2>
                  <p className="panel-desc">Record cash/bank outflow or supplier credit payable.</p>
                </div>
              </div>

              <div className="totals-summary-card">
                <div className="totals-row">
                  <span className="totals-label">Order Subtotal</span>
                  <span className="totals-value">₹{(estimatedPurchaseSubtotalCents / 100).toFixed(2)}</span>
                </div>
                <div className="totals-row">
                  <span className="totals-label">Amount Paid</span>
                  <span className="totals-value text-emerald">₹{(purchasePaidCents / 100).toFixed(2)}</span>
                </div>
                <div className="totals-divider" />
                <div className="totals-row totals-highlight">
                  <span className="totals-label">Payable Credit Due</span>
                  <span className="totals-value text-amber">₹{(estimatedPurchaseDueCents / 100).toFixed(2)}</span>
                </div>
              </div>

              <div className="settlement-controls">
                <div className="form-group">
                  <label className="field-label">Amount Paid Now (₹)</label>
                  <input
                    type="number"
                    step="0.01"
                    min="0"
                    value={paidAmountRupees}
                    onChange={(e) => setPaidAmountRupees(e.target.value)}
                    className="input-field"
                  />
                </div>

                {purchasePaidCents > 0 && (
                  <div className="form-group">
                    <label className="field-label">Payment Method</label>
                    <select
                      value={purchasePaymentMethod}
                      onChange={(e) => setPurchasePaymentMethod(e.target.value as PaymentMethod)}
                      className="select-field"
                    >
                      <option value="CASH">CASH (Physical Currency)</option>
                      <option value="UPI">UPI (Online Direct)</option>
                      <option value="BANK_TRANSFER">BANK TRANSFER (IMPS / NEFT)</option>
                      <option value="CARD">CARD (Debit / Credit)</option>
                      <option value="OTHER">OTHER</option>
                    </select>
                  </div>
                )}
              </div>

              <div className="panel-actions" style={{ marginTop: "auto" }}>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={handlePreparePurchase}
                  disabled={purchaseItems.length === 0 || preparingPurchase}
                  style={{ width: "100%" }}
                >
                  {preparingPurchase ? "Validating..." : "Review & Prepare Purchase"}
                </button>
              </div>
            </section>
          </div>
        </main>
      )}

      {/* ======================================================================================== */}
      {/* 3. CUSTOMER CREDITS (KHATA) WORKSPACE (BUILD 08) */}
      {/* ======================================================================================== */}
      {activeTab === "credits" && (
        <main className="purchase-workspace">
          <div className="purchase-layout-grid">
            {/* Left Column: Customer Khata Directory & Search */}
            <section className="panel-card" style={{ display: "flex", flexDirection: "column" }}>
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Customer Credits (Khata) Directory</h2>
                  <p className="panel-desc">Track customer balances, credit sales, and record settlements.</p>
                </div>
                <button
                  type="button"
                  className="btn btn-secondary"
                  style={{ fontSize: "0.8rem", padding: "0.35rem 0.75rem" }}
                  onClick={() => void loadCustomerCredits()}
                  disabled={loadingCredits}
                >
                  {loadingCredits ? "Refreshing..." : "Refresh"}
                </button>
              </div>

              {/* Total Store-wide Outstanding Metric Banner */}
              <div className="khata-summary-banner">
                <div className="banner-metric">
                  <span className="banner-label">Total Store Khata Outstanding</span>
                  <div className="banner-amount">
                    ₹{(totalOutstandingCents / 100).toLocaleString("en-IN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                  </div>
                </div>
                <div className="banner-badge-group">
                  <div className="banner-sub-pill">
                    {creditCustomers.filter((c) => c.currentCreditCents > 0).length} Due Accounts
                  </div>
                  <div className="banner-sub-pill">
                    {creditCustomers.length} Total Accounts
                  </div>
                </div>
              </div>

              {/* Search Bar */}
              <div style={{ marginBottom: "1rem" }}>
                <input
                  type="text"
                  placeholder="Search customers by name or phone..."
                  value={creditSearchQuery}
                  onChange={(e) => setCreditSearchQuery(e.target.value)}
                  className="input-field barcode-input"
                  style={{ width: "100%" }}
                />
              </div>

              {/* Customer Cards List */}
              <div className="khata-list-container" style={{ flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: "0.6rem" }}>
                {creditCustomers
                  .filter((c) => {
                    const q = creditSearchQuery.toLowerCase().trim();
                    if (!q) return true;
                    return (
                      c.name.toLowerCase().includes(q) ||
                      (c.phone && c.phone.toLowerCase().includes(q))
                    );
                  })
                  .map((customer) => {
                    const isSelected = selectedCreditCustomer?.id === customer.id;
                    const hasDue = customer.currentCreditCents > 0;
                    return (
                      <div
                        key={customer.id}
                        className={`khata-customer-card ${isSelected ? "active-card" : ""}`}
                        onClick={() => handleSelectCustomer(customer)}
                      >
                        <div className="khata-card-header">
                          <span className="khata-customer-name">{customer.name}</span>
                          <span className={`khata-status-badge ${hasDue ? "badge-due" : "badge-settled"}`}>
                            {hasDue ? "DUE" : "SETTLED"}
                          </span>
                        </div>
                        <div className="khata-card-meta">
                          {customer.phone && <span className="khata-phone">{customer.phone}</span>}
                          {customer.address && <span className="khata-address">{customer.address}</span>}
                        </div>
                        <div className="khata-card-footer">
                          <span className="khata-balance-label">Balance Due:</span>
                          <span className={`khata-balance-amount ${hasDue ? "text-due" : "text-settled"}`}>
                            ₹{(customer.currentCreditCents / 100).toLocaleString("en-IN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                          </span>
                        </div>
                      </div>
                    );
                  })}
                {creditCustomers.length === 0 && !loadingCredits && (
                  <div className="empty-table-state">
                    <div className="empty-text">No Customers Found</div>
                    <div className="empty-hint">Customers with credit or activity will appear here</div>
                  </div>
                )}
              </div>
            </section>

            {/* Right Column: Customer Detail & Deterministic Ledger History */}
            <section className="panel-card" style={{ display: "flex", flexDirection: "column" }}>
              {!selectedCreditCustomer ? (
                <div className="khata-empty-selection">
                  <div className="khata-empty-icon">📖</div>
                  <h3 className="panel-title" style={{ fontSize: "1.15rem", marginBottom: "0.4rem" }}>
                    Select a Customer Khata Account
                  </h3>
                  <p className="panel-desc" style={{ maxWidth: "380px", textAlign: "center" }}>
                    Click on any customer from the left directory to inspect their complete append-only ledger history and record settlements.
                  </p>
                </div>
              ) : (
                <>
                  {/* Selected Customer Header Banner */}
                  <div className="khata-detail-header">
                    <div>
                      <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
                        <h2 className="panel-title" style={{ fontSize: "1.35rem" }}>
                          {selectedCreditCustomer.name}
                        </h2>
                        <span className={`khata-status-badge ${selectedCreditCustomer.currentCreditCents > 0 ? "badge-due" : "badge-settled"}`}>
                          {selectedCreditCustomer.currentCreditCents > 0 ? "DUE" : "SETTLED"}
                        </span>
                      </div>
                      <div className="panel-desc" style={{ marginTop: "0.2rem" }}>
                        {selectedCreditCustomer.phone && <span>Phone: {selectedCreditCustomer.phone} • </span>}
                        {selectedCreditCustomer.address && <span>Address: {selectedCreditCustomer.address}</span>}
                      </div>
                    </div>

                    <div className="khata-header-action-block">
                      <div className="khata-balance-summary-box">
                        <span className="balance-box-label">Current Outstanding Due</span>
                        <div className="balance-box-val">
                          ₹{(selectedCreditCustomer.currentCreditCents / 100).toLocaleString("en-IN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                        </div>
                      </div>
                      <button
                        type="button"
                        className="btn btn-primary"
                        onClick={handleOpenPaymentModal}
                        disabled={selectedCreditCustomer.currentCreditCents <= 0}
                        style={{ height: "42px", padding: "0 1.25rem" }}
                      >
                        + Record Payment
                      </button>
                    </div>
                  </div>

                  {/* Ledger Table Section */}
                  <div style={{ marginTop: "1rem", flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "0.5rem" }}>
                      <span className="form-section-title" style={{ fontSize: "0.85rem", textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--text-secondary)" }}>
                        Deterministic Ledger Audit Trail ({customerLedgerHistory.length} entries)
                      </span>
                      {loadingLedger && <span style={{ fontSize: "0.75rem", color: "var(--accent-cyan)" }}>Loading ledger...</span>}
                    </div>

                    <div className="items-table-container" style={{ flex: 1, overflowY: "auto" }}>
                      <table className="items-table">
                        <thead>
                          <tr>
                            <th style={{ width: "16%" }}>Date & Time</th>
                            <th style={{ width: "14%" }}>Type</th>
                            <th style={{ width: "16%" }}>Reference</th>
                            <th style={{ width: "12%", textAlign: "right" }}>Debit (+)</th>
                            <th style={{ width: "12%", textAlign: "right" }}>Credit (-)</th>
                            <th style={{ width: "14%", textAlign: "right" }}>Balance</th>
                            <th style={{ width: "16%" }}>Notes / Actor</th>
                          </tr>
                        </thead>
                        <tbody>
                          {customerLedgerHistory.length === 0 ? (
                            <tr>
                              <td colSpan={7} className="empty-table-state">
                                <div className="empty-text">No Ledger Records</div>
                                <div className="empty-hint">Ledger records will appear as credit sales or payments occur</div>
                              </td>
                            </tr>
                          ) : (
                            customerLedgerHistory.map((entry) => {
                              const isDebit = entry.entryType === "SALE_CREDIT";
                              const isCredit = entry.entryType === "PAYMENT_RECEIVED";
                              const dateObj = new Date(entry.createdAt);
                              const formattedDate = dateObj.toLocaleDateString("en-IN", { month: "short", day: "numeric" });
                              const formattedTime = dateObj.toLocaleTimeString("en-IN", { hour: "2-digit", minute: "2-digit" });

                              return (
                                <tr key={entry.id}>
                                  <td>
                                    <div style={{ fontSize: "0.8rem", color: "var(--text-primary)" }}>{formattedDate}</div>
                                    <div style={{ fontSize: "0.7rem", color: "var(--text-muted)" }}>{formattedTime}</div>
                                  </td>
                                  <td>
                                    <span className={`ledger-type-badge ${isCredit ? "badge-payment" : "badge-sale"}`}>
                                      {entry.entryType === "SALE_CREDIT" ? "Sale Credit" : entry.entryType === "PAYMENT_RECEIVED" ? "Payment" : entry.entryType}
                                    </span>
                                  </td>
                                  <td>
                                    <span className="font-mono" style={{ fontSize: "0.78rem" }}>{entry.referenceId}</span>
                                    <div style={{ fontSize: "0.68rem", color: "var(--text-muted)" }}>{entry.referenceType}</div>
                                  </td>
                                  <td style={{ textAlign: "right" }}>
                                    {isDebit ? (
                                      <span className="ledger-debit-text">
                                        +₹{(entry.amountCents / 100).toFixed(2)}
                                      </span>
                                    ) : (
                                      <span style={{ color: "var(--text-muted)" }}>—</span>
                                    )}
                                  </td>
                                  <td style={{ textAlign: "right" }}>
                                    {isCredit ? (
                                      <span className="ledger-credit-text">
                                        -₹{(entry.amountCents / 100).toFixed(2)}
                                      </span>
                                    ) : (
                                      <span style={{ color: "var(--text-muted)" }}>—</span>
                                    )}
                                  </td>
                                  <td style={{ textAlign: "right" }}>
                                    <span className="font-mono" style={{ fontWeight: 600 }}>
                                      ₹{(entry.balanceAfterCents / 100).toFixed(2)}
                                    </span>
                                  </td>
                                  <td>
                                    <div style={{ fontSize: "0.78rem", color: "var(--text-secondary)" }}>{entry.notes || "—"}</div>
                                    {entry.userName && <div style={{ fontSize: "0.68rem", color: "var(--text-muted)" }}>By: {entry.userName}</div>}
                                  </td>
                                </tr>
                              );
                            })
                          )}
                        </tbody>
                      </table>
                    </div>
                  </div>
                </>
              )}
            </section>
          </div>
        </main>
      )}

      {/* ======================================================================================== */}
      {/* 4. CUSTOMER ORDERS WORKSPACE */}
      {/* ======================================================================================== */}
      {activeTab === "orders" && (
        <main className="purchase-workspace">
          <div className="purchase-layout-grid">
            {/* Left Column: Orders List & Filter */}
            <section className="panel-card">
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Customer Orders Management</h2>
                  <p className="panel-desc">Record, edit, and track customer intent before converting to finalized sales.</p>
                </div>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={openCreateOrderModal}
                >
                  + New Order
                </button>
              </div>

              {/* Status Filter Pills */}
              <div className="order-filter-pills">
                {(["ALL", "DRAFT", "CONVERTED", "CANCELLED"] as const).map((status) => {
                  const count = status === "ALL" ? ordersSummary.length : ordersSummary.filter((o) => o.status === status).length;
                  return (
                    <button
                      key={status}
                      type="button"
                      className={`filter-pill-btn ${ordersFilter === status ? "active" : ""}`}
                      onClick={() => {
                        setOrdersFilter(status);
                        void loadCustomerOrders(status);
                      }}
                    >
                      {status} ({count})
                    </button>
                  );
                })}
              </div>

              {/* Search Bar */}
              <div style={{ marginBottom: "0.85rem" }}>
                <input
                  type="text"
                  placeholder="Search by order number or customer name/phone..."
                  value={ordersSearchQuery}
                  onChange={(e) => setOrdersSearchQuery(e.target.value)}
                  className="input-field"
                />
              </div>

              {/* Orders List Cards */}
              <div className="khata-customer-list">
                {ordersLoading ? (
                  <div className="khata-empty-state">Loading customer orders...</div>
                ) : filteredOrders.length === 0 ? (
                  <div className="khata-empty-state">
                    <p>No orders found matching criteria.</p>
                    <button
                      type="button"
                      className="btn btn-secondary"
                      style={{ marginTop: "0.5rem" }}
                      onClick={openCreateOrderModal}
                    >
                      Create First Order
                    </button>
                  </div>
                ) : (
                  filteredOrders.map((order) => {
                    const isSelected = selectedOrderId === order.id;
                    const dateFormatted = new Date(order.createdAt).toLocaleDateString();
                    return (
                      <div
                        key={order.id}
                        className={`order-card ${isSelected ? "active-card" : ""}`}
                        onClick={() => {
                          setSelectedOrderId(order.id);
                          void loadOrderDetail(order.id);
                        }}
                      >
                        <div className="order-card-header">
                          <span className="order-number-title">{order.orderNumber}</span>
                          <span
                            className={`ledger-type-badge ${
                              order.status === "DRAFT"
                                ? "badge-order-draft"
                                : order.status === "CONVERTED"
                                ? "badge-order-converted"
                                : "badge-order-cancelled"
                            }`}
                          >
                            {order.status}
                          </span>
                        </div>
                        <div className="order-card-meta">
                          <span>{order.customerName ? order.customerName : "Walk-in Customer"}</span>
                          <span>{dateFormatted}</span>
                        </div>
                        <div className="order-card-footer">
                          <span className="order-items-count">
                            {order.itemCount} {order.itemCount === 1 ? "item" : "items"}
                          </span>
                          {order.notes && (
                            <span style={{ fontSize: "0.72rem", color: "var(--text-muted)", maxWidth: "160px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                              {order.notes}
                            </span>
                          )}
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </section>

            {/* Right Column: Order Detail & Actions */}
            <section className="panel-card khata-detail-pane">
              {loadingOrderDetail ? (
                <div className="khata-empty-state">Loading order details...</div>
              ) : !selectedOrderDetail ? (
                <div className="khata-empty-state">
                  <div style={{ fontSize: "2rem", marginBottom: "0.5rem" }}>📋</div>
                  <h3>No Order Selected</h3>
                  <p>Select an order from the list on the left or create a new order to view items, customer information, and conversion options.</p>
                </div>
              ) : (
                <>
                  <div className="khata-detail-header">
                    <div>
                      <div style={{ display: "flex", alignItems: "center", gap: "0.75rem", marginBottom: "0.25rem" }}>
                        <h2 className="panel-title font-mono">{selectedOrderDetail.orderNumber}</h2>
                        <span
                          className={`ledger-type-badge ${
                            selectedOrderDetail.status === "DRAFT"
                              ? "badge-order-draft"
                              : selectedOrderDetail.status === "CONVERTED"
                              ? "badge-order-converted"
                              : "badge-order-cancelled"
                          }`}
                        >
                          {selectedOrderDetail.status}
                        </span>
                      </div>
                      <p className="panel-desc">
                        Created {new Date(selectedOrderDetail.createdAt).toLocaleString()}
                        {selectedOrderDetail.updatedAt !== selectedOrderDetail.createdAt && (
                          <span> &bull; Updated {new Date(selectedOrderDetail.updatedAt).toLocaleString()}</span>
                        )}
                      </p>
                    </div>

                    {/* Action Buttons */}
                    <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
                      {selectedOrderDetail.status === "DRAFT" && (
                        <>
                          <button
                            type="button"
                            className="btn btn-secondary"
                            onClick={() => openEditOrderModal(selectedOrderDetail)}
                          >
                            Edit Draft
                          </button>
                          <button
                            type="button"
                            className="btn btn-danger"
                            onClick={() => handleCancelOrder(selectedOrderDetail.id)}
                          >
                            Cancel Order
                          </button>
                          <button
                            type="button"
                            className="btn btn-primary"
                            onClick={() => openConvertOrderModal(selectedOrderDetail)}
                          >
                            Convert to Sale &rarr;
                          </button>
                        </>
                      )}
                      {selectedOrderDetail.status === "CONVERTED" && selectedOrderDetail.convertedSaleId && (
                        <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
                          <span className="badge-order-converted" style={{ padding: "0.4rem 0.75rem", borderRadius: "0.5rem", fontSize: "0.82rem" }}>
                            &check; Converted to Sale: <span className="font-mono">{selectedOrderDetail.convertedSaleId}</span>
                          </span>
                        </div>
                      )}
                      {selectedOrderDetail.status === "CANCELLED" && (
                        <span className="badge-order-cancelled" style={{ padding: "0.4rem 0.75rem", borderRadius: "0.5rem", fontSize: "0.82rem" }}>
                          &cross; Order Cancelled
                        </span>
                      )}
                    </div>
                  </div>

                  {/* Customer Information Card */}
                  <div style={{ margin: "1rem 0", padding: "0.85rem 1rem", background: "var(--bg-card-hover)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                    <div>
                      <div style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)", letterSpacing: "0.05em" }}>Customer Information</div>
                      <div style={{ fontWeight: 600, fontSize: "0.95rem", color: "var(--text-primary)", marginTop: "0.2rem" }}>
                        {selectedOrderDetail.customerName || "Walk-in Customer (Unregistered)"}
                      </div>
                      {selectedOrderDetail.customerPhone && (
                        <div style={{ fontSize: "0.8rem", color: "var(--text-secondary)" }}>{selectedOrderDetail.customerPhone}</div>
                      )}
                    </div>
                    {selectedOrderDetail.customerId && (
                      <div style={{ textAlign: "right" }}>
                        <div style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)" }}>Registered Khata Customer</div>
                        <div className="font-mono" style={{ fontWeight: 600, color: "var(--accent-cyan)", fontSize: "0.85rem" }}>
                          ID: {selectedOrderDetail.customerId.slice(0, 12)}...
                        </div>
                      </div>
                    )}
                  </div>

                  {/* Order Items Table */}
                  <div className="khata-ledger-card" style={{ flex: 1 }}>
                    <div style={{ padding: "0.75rem 1rem", borderBottom: "1px solid var(--border-subtle)", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                      <h3 style={{ margin: 0, fontSize: "0.9rem", fontWeight: 600 }}>Order Items ({selectedOrderDetail.items.length})</h3>
                      <span style={{ fontSize: "0.75rem", color: "var(--text-muted)" }}>Stock levels shown are current catalog inventory</span>
                    </div>

                    <div className="items-table-container">
                      <table className="items-table">
                        <thead>
                          <tr>
                            <th style={{ width: "35%" }}>Product</th>
                            <th style={{ width: "20%" }}>Ordered Quantity</th>
                            <th style={{ width: "25%" }}>Catalog Price & Stock</th>
                            <th style={{ width: "20%" }}>Item Notes</th>
                          </tr>
                        </thead>
                        <tbody>
                          {selectedOrderDetail.items.map((item: CustomerOrderItemDetail) => {
                            const qtyDisplay = (item.quantity / 1000).toFixed(item.unit === "pcs" ? 0 : 3);
                            const stockAvailable = item.availableStock;
                            const hasEnoughStock = stockAvailable >= item.quantity;
                            const isOut = stockAvailable <= 0;

                            return (
                              <tr key={item.id}>
                                <td>
                                  <div className="item-name-cell">
                                    <span className="item-name">{item.productName}</span>
                                    <span className="type-badge">{item.productType}</span>
                                  </div>
                                </td>
                                <td>
                                  <span className="font-mono" style={{ fontWeight: 600 }}>
                                    {qtyDisplay} {item.unit}
                                  </span>
                                </td>
                                <td>
                                  <div style={{ display: "flex", flexDirection: "column", gap: "0.2rem" }}>
                                    <span style={{ fontSize: "0.85rem", fontWeight: 600, color: "var(--accent-cyan)" }}>
                                      ₹{(item.unitPriceCents / 100).toFixed(2)} / {item.unit}
                                    </span>
                                    <span
                                      className={`stock-badge ${
                                        isOut
                                          ? "stock-badge-outstock"
                                          : !hasEnoughStock
                                          ? "stock-badge-lowstock"
                                          : "stock-badge-instock"
                                      }`}
                                    >
                                      {isOut
                                        ? "Out of Stock"
                                        : `${(stockAvailable / 1000).toFixed(item.unit === "pcs" ? 0 : 1)} in stock`}
                                    </span>
                                  </div>
                                </td>
                                <td>
                                  <span style={{ fontSize: "0.8rem", color: "var(--text-secondary)" }}>
                                    {item.notes || "—"}
                                  </span>
                                </td>
                              </tr>
                            );
                          })}
                        </tbody>
                      </table>
                    </div>
                  </div>

                  {/* Order-level Notes */}
                  {selectedOrderDetail.notes && (
                    <div style={{ marginTop: "0.85rem", padding: "0.75rem 1rem", background: "rgba(255,255,255,0.02)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)" }}>
                      <span style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)", fontWeight: 600 }}>Internal Notes: </span>
                      <span style={{ fontSize: "0.85rem", color: "var(--text-secondary)" }}>{selectedOrderDetail.notes}</span>
                    </div>
                  )}
                </>
              )}
            </section>
          </div>
        </main>
      )}

      {/* ======================================================================================== */}
      {/* 5. RETURNS & STOCK REVERSAL WORKSPACE */}
      {/* ======================================================================================== */}
      {activeTab === "returns" && (
        <main className="purchase-workspace">
          <div className="purchase-layout-grid">
            {/* Left Column: Returns List & Actions */}
            <section className="panel-card">
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Returns & Stock Reversal</h2>
                  <p className="panel-desc">Manage customer return refunds and supplier stock reversal credit notes.</p>
                </div>
                <div style={{ display: "flex", gap: "0.5rem" }}>
                  <button
                    type="button"
                    className="btn btn-primary"
                    style={{ background: "linear-gradient(135deg, #059669 0%, #10b981 100%)", borderColor: "#10b981" }}
                    onClick={openCustomerReturnModal}
                  >
                    + Customer Return
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    style={{ borderColor: "rgba(245, 158, 11, 0.4)", color: "#fbbf24" }}
                    onClick={openSupplierReturnModal}
                  >
                    + Supplier Return
                  </button>
                </div>
              </div>

              {/* Status Filter Pills */}
              <div className="order-filter-pills">
                {(["ALL", "CUSTOMER_RETURN", "SUPPLIER_RETURN"] as const).map((filterVal) => {
                  const count =
                    filterVal === "ALL"
                      ? returnsSummary.length
                      : returnsSummary.filter((r) => r.returnType === filterVal).length;
                  const label =
                    filterVal === "ALL"
                      ? "ALL"
                      : filterVal === "CUSTOMER_RETURN"
                      ? "Customer Returns"
                      : "Supplier Returns";
                  return (
                    <button
                      key={filterVal}
                      type="button"
                      className={`filter-pill-btn ${returnsFilter === filterVal ? "active" : ""}`}
                      onClick={() => {
                        setReturnsFilter(filterVal);
                        void loadReturnsData(filterVal === "ALL" ? null : filterVal);
                      }}
                    >
                      {label} ({count})
                    </button>
                  );
                })}
              </div>

              {/* Search Bar */}
              <div style={{ marginBottom: "0.85rem" }}>
                <input
                  type="text"
                  placeholder="Search returns by number, party, or reason..."
                  value={returnsSearchQuery}
                  onChange={(e) => setReturnsSearchQuery(e.target.value)}
                  className="input-field"
                />
              </div>

              {/* Returns List Cards */}
              <div className="khata-customer-list">
                {returnsLoading ? (
                  <div className="khata-empty-state">Loading returns history...</div>
                ) : filteredReturns.length === 0 ? (
                  <div className="khata-empty-state">
                    <p>No returns found matching criteria.</p>
                    <div style={{ display: "flex", gap: "0.5rem", justifyContent: "center", marginTop: "0.75rem" }}>
                      <button
                        type="button"
                        className="btn btn-secondary"
                        onClick={openCustomerReturnModal}
                      >
                        New Customer Return
                      </button>
                      <button
                        type="button"
                        className="btn btn-secondary"
                        onClick={openSupplierReturnModal}
                      >
                        New Supplier Return
                      </button>
                    </div>
                  </div>
                ) : (
                  filteredReturns.map((ret) => {
                    const isSelected = selectedReturnId === ret.id;
                    const dateFormatted = new Date(ret.createdAt).toLocaleDateString("en-IN", {
                      month: "short",
                      day: "numeric",
                      year: "numeric",
                    });
                    const partyName =
                      ret.counterpartyName ||
                      (ret.returnType === "CUSTOMER_RETURN" ? "Walk-in Customer" : "Supplier");

                    return (
                      <div
                        key={ret.id}
                        className={`return-card ${isSelected ? "active-card" : ""}`}
                        onClick={() => {
                          setSelectedReturnId(ret.id);
                          void loadReturnDetail(ret.id);
                        }}
                      >
                        <div className="return-card-header">
                          <span className="font-mono" style={{ fontWeight: 700, fontSize: "0.92rem", color: "var(--text-primary)" }}>
                            {ret.returnNumber}
                          </span>
                          <span
                            className={
                              ret.returnType === "CUSTOMER_RETURN"
                                ? "badge-return-customer"
                                : "badge-return-supplier"
                            }
                          >
                            {ret.returnType === "CUSTOMER_RETURN" ? "Customer Return" : "Supplier Return"}
                          </span>
                        </div>
                        <div className="return-card-meta">
                          <span>{partyName}</span>
                          <span>{dateFormatted}</span>
                        </div>
                        <div className="return-card-footer">
                          <span className="order-items-count">
                            {ret.itemCount} {ret.itemCount === 1 ? "item" : "items"}
                          </span>
                          <span className="font-mono" style={{ fontWeight: 700, color: ret.returnType === "CUSTOMER_RETURN" ? "var(--accent-emerald)" : "var(--accent-amber)" }}>
                            ₹{(ret.totalAmountCents / 100).toFixed(2)}
                          </span>
                        </div>
                        {ret.reason && (
                          <div style={{ fontSize: "0.72rem", color: "var(--text-muted)", marginTop: "0.15rem", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                            Reason: {ret.reason}
                          </div>
                        )}
                      </div>
                    );
                  })
                )}
              </div>
            </section>

            {/* Right Column: Return Detail Pane */}
            <section className="panel-card khata-detail-pane">
              {loadingReturnDetail ? (
                <div className="khata-empty-state">Loading return details...</div>
              ) : !selectedReturnDetail ? (
                <div className="khata-empty-state">
                  <div style={{ fontSize: "2.5rem", marginBottom: "0.5rem" }}>🔄</div>
                  <h3>No Return Selected</h3>
                  <p>Select a return from the list on the left to inspect returned items, inventory adjustments, and authoritative financial balances.</p>
                </div>
              ) : (
                <>
                  <div className="khata-detail-header">
                    <div>
                      <div style={{ display: "flex", alignItems: "center", gap: "0.75rem", marginBottom: "0.25rem" }}>
                        <h2 className="panel-title font-mono">{selectedReturnDetail.returnNumber}</h2>
                        <span
                          className={
                            selectedReturnDetail.returnType === "CUSTOMER_RETURN"
                              ? "badge-return-customer"
                              : "badge-return-supplier"
                          }
                        >
                          {selectedReturnDetail.returnType === "CUSTOMER_RETURN" ? "Customer Return" : "Supplier Return"}
                        </span>
                      </div>
                      <p className="panel-desc">
                        Processed {new Date(selectedReturnDetail.createdAt).toLocaleString()}
                        {selectedReturnDetail.reason && (
                          <span> &bull; Reason: <em>{selectedReturnDetail.reason}</em></span>
                        )}
                      </p>
                    </div>
                  </div>

                  {/* Counterparty Information Card */}
                  <div style={{ margin: "1rem 0", padding: "0.85rem 1rem", background: "var(--bg-card-hover)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                    <div>
                      <div style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)", letterSpacing: "0.05em" }}>
                        {selectedReturnDetail.returnType === "CUSTOMER_RETURN" ? "Customer Information" : "Supplier Information"}
                      </div>
                      <div style={{ fontWeight: 600, fontSize: "0.95rem", color: "var(--text-primary)", marginTop: "0.2rem" }}>
                        {selectedReturnDetail.returnType === "CUSTOMER_RETURN"
                          ? selectedReturnDetail.customerName || "Walk-in Customer (Unregistered)"
                          : selectedReturnDetail.supplierName || "Supplier"}
                      </div>
                    </div>
                    {selectedReturnDetail.customerId ? (
                      <div style={{ textAlign: "right" }}>
                        <div style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)" }}>Khata Customer</div>
                        <div className="font-mono" style={{ fontWeight: 600, color: "var(--accent-cyan)", fontSize: "0.85rem" }}>
                          ID: {selectedReturnDetail.customerId.slice(0, 12)}...
                        </div>
                      </div>
                    ) : selectedReturnDetail.supplierId ? (
                      <div style={{ textAlign: "right" }}>
                        <div style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)" }}>Supplier Account</div>
                        <div className="font-mono" style={{ fontWeight: 600, color: "var(--accent-amber)", fontSize: "0.85rem" }}>
                          ID: {selectedReturnDetail.supplierId.slice(0, 12)}...
                        </div>
                      </div>
                    ) : null}
                  </div>

                  {/* Returned Items Table */}
                  <div className="khata-ledger-card" style={{ flex: 1, marginBottom: "1rem" }}>
                    <div style={{ padding: "0.75rem 1rem", borderBottom: "1px solid var(--border-subtle)", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                      <h3 style={{ margin: 0, fontSize: "0.9rem", fontWeight: 600 }}>Returned Items ({selectedReturnDetail.items.length})</h3>
                      <span style={{ fontSize: "0.75rem", color: "var(--text-muted)" }}>
                        {selectedReturnDetail.returnType === "CUSTOMER_RETURN" ? "Inventory restocked upon return" : "Inventory reversed & deducted"}
                      </span>
                    </div>

                    <div className="items-table-container">
                      <table className="items-table">
                        <thead>
                          <tr>
                            <th style={{ width: "40%" }}>Product</th>
                            <th style={{ width: "20%" }}>Returned Qty</th>
                            <th style={{ width: "20%" }}>Unit Rate</th>
                            <th style={{ width: "20%", textAlign: "right" }}>Line Total</th>
                          </tr>
                        </thead>
                        <tbody>
                          {selectedReturnDetail.items.map((item) => {
                            const qtyDisplay = (item.quantity / 1000).toFixed(item.unit === "pcs" ? 0 : 3);
                            return (
                              <tr key={item.productId}>
                                <td>
                                  <div className="item-name-cell">
                                    <span className="item-name">{item.productName}</span>
                                    <span className="type-badge">{item.productType}</span>
                                  </div>
                                </td>
                                <td>
                                  <span className="font-mono" style={{ fontWeight: 600 }}>
                                    {qtyDisplay} {item.unit}
                                  </span>
                                </td>
                                <td>
                                  <span style={{ fontSize: "0.85rem", color: "var(--text-secondary)" }}>
                                    ₹{(item.unitPriceCents / 100).toFixed(2)}
                                  </span>
                                </td>
                                <td style={{ textAlign: "right" }}>
                                  <span className="font-mono" style={{ fontWeight: 600 }}>
                                    ₹{(item.totalCents / 100).toFixed(2)}
                                  </span>
                                </td>
                              </tr>
                            );
                          })}
                        </tbody>
                      </table>
                    </div>
                  </div>

                  {/* Financial Settlement Breakdown Card */}
                  <div style={{ padding: "1rem", background: "rgba(255,255,255,0.03)", borderRadius: "0.65rem", border: "1px solid var(--border-subtle)", marginBottom: "0.75rem" }}>
                    <div style={{ fontSize: "0.8rem", textTransform: "uppercase", color: "var(--text-muted)", fontWeight: 700, marginBottom: "0.6rem" }}>
                      Financial Settlement Breakdown
                    </div>

                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "0.75rem" }}>
                      <div style={{ padding: "0.75rem", background: "var(--bg-card)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)" }}>
                        <span style={{ fontSize: "0.75rem", color: "var(--text-muted)", display: "block" }}>Total Return Value</span>
                        <strong className="font-mono" style={{ fontSize: "1.1rem", color: "var(--text-primary)" }}>
                          ₹{(selectedReturnDetail.totalAmountCents / 100).toFixed(2)}
                        </strong>
                      </div>
                      <div style={{ padding: "0.75rem", background: "var(--bg-card)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)" }}>
                        <span style={{ fontSize: "0.75rem", color: "var(--text-muted)", display: "block" }}>Returned Item Count</span>
                        <strong className="font-mono" style={{ fontSize: "1.1rem", color: "var(--accent-cyan)" }}>
                          {selectedReturnDetail.items.length} products
                        </strong>
                      </div>
                      <div style={{ padding: "0.75rem", background: "var(--bg-card)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)" }}>
                        <span style={{ fontSize: "0.75rem", color: "var(--text-muted)", display: "block" }}>Reversal Classification</span>
                        <strong className="font-mono" style={{ fontSize: "0.95rem", color: selectedReturnDetail.returnType === "CUSTOMER_RETURN" ? "var(--accent-emerald)" : "var(--accent-amber)" }}>
                          {selectedReturnDetail.returnType === "CUSTOMER_RETURN" ? "Restock & Refund/Khata" : "Stock Reversal & Debit Note"}
                        </strong>
                      </div>
                    </div>
                  </div>

                  {/* Guardrail Guarantees Notice */}
                  <div className="guardrail-notice-box">
                    {selectedReturnDetail.returnType === "CUSTOMER_RETURN" ? (
                      <div>
                        <strong>Guardrail 3 Validated:</strong> Authoritative live database revalidation confirmed catalog selling prices and customer Khata debt before committing. Inventory restocked atomically.
                      </div>
                    ) : (
                      <div>
                        <strong>Guardrails 1 & 2 Enforced:</strong> Atomic conditional inventory decrement (<code>WHERE current_quantity &gt;= ?1</code>) prevented negative stock. Signed trade balance accurately records credit notes without artificial zero-clamping.
                      </div>
                    )}
                  </div>
                </>
              )}
            </section>
          </div>
        </main>
      )}

      {/* ======================================================================================== */}
      {/* 6. BUILD 11: STOCK CORRECTIONS & PHYSICAL INVENTORY ADJUSTMENT WORKSPACE */}
      {/* ======================================================================================== */}
      {activeTab === "corrections" && (
        <main className="purchase-workspace">
          <div className="purchase-layout-grid">
            {/* Left Column: Stock Adjustment Form */}
            <section className="panel-card">
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Physical Stock Adjustment</h2>
                  <p className="panel-desc">
                    Reconcile physical stock discrepancies (miscount, shrinkage, damage, spoilage). Generates immutable audit movements.
                  </p>
                </div>
              </div>

              {/* Product Selection */}
              <div style={{ marginBottom: "1rem" }}>
                <label className="form-label" style={{ display: "block", marginBottom: "0.4rem", fontWeight: 600, fontSize: "0.85rem" }}>
                  Select Inventory Item:
                </label>
                <select
                  className="form-control"
                  style={{ width: "100%", padding: "0.6rem 0.8rem", borderRadius: "0.45rem", background: "var(--bg-input)", color: "var(--text-primary)", border: "1px solid var(--border-subtle)" }}
                  value={selectedCorrectionProductId}
                  onChange={(e) => {
                    setSelectedCorrectionProductId(e.target.value);
                    setPhysicalCountInput("");
                    setManualDeltaInput("");
                  }}
                >
                  <option value="">-- Choose Product --</option>
                  {correctionProducts.map((p) => {
                    const currentUnits = (p.currentQuantity / 1000).toFixed(p.unit === "pcs" ? 0 : 3);
                    return (
                      <option key={p.id} value={p.id}>
                        {p.name} — Current: {currentUnits} {p.unit} ({p.productType})
                      </option>
                    );
                  })}
                </select>
              </div>

              {/* Selected Product Informational Banner */}
              {selectedCorrectionProductId && (() => {
                const product = correctionProducts.find((p) => p.id === selectedCorrectionProductId);
                if (!product) return null;
                const currentQty = (product.currentQuantity / 1000).toFixed(product.unit === "pcs" ? 0 : 3);

                let computedDeltaDisplay = "0.000";
                let resultingQtyDisplay = currentQty;
                let isDeltaPositive = false;
                let isDeltaNegative = false;

                if (adjustmentMode === "PHYSICAL_COUNT" && physicalCountInput.trim() && !isNaN(Number(physicalCountInput))) {
                  const targetMillie = Math.round(Number(physicalCountInput) * 1000);
                  const deltaMillie = targetMillie - product.currentQuantity;
                  computedDeltaDisplay = (deltaMillie > 0 ? "+" : "") + (deltaMillie / 1000).toFixed(product.unit === "pcs" ? 0 : 3);
                  resultingQtyDisplay = (targetMillie / 1000).toFixed(product.unit === "pcs" ? 0 : 3);
                  isDeltaPositive = deltaMillie > 0;
                  isDeltaNegative = deltaMillie < 0;
                } else if (adjustmentMode === "MANUAL_DELTA" && manualDeltaInput.trim() && !isNaN(Number(manualDeltaInput))) {
                  const deltaMillie = Math.round(Number(manualDeltaInput) * 1000);
                  computedDeltaDisplay = (deltaMillie > 0 ? "+" : "") + (deltaMillie / 1000).toFixed(product.unit === "pcs" ? 0 : 3);
                  resultingQtyDisplay = ((product.currentQuantity + deltaMillie) / 1000).toFixed(product.unit === "pcs" ? 0 : 3);
                  isDeltaPositive = deltaMillie > 0;
                  isDeltaNegative = deltaMillie < 0;
                }

                return (
                  <div style={{ padding: "0.85rem", background: "rgba(255,255,255,0.03)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)", marginBottom: "1rem" }}>
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "0.5rem", textAlign: "center" }}>
                      <div>
                        <div style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)" }}>Current Stock</div>
                        <div className="font-mono" style={{ fontSize: "1.1rem", fontWeight: 700, color: "var(--text-primary)" }}>
                          {currentQty} {product.unit}
                        </div>
                      </div>
                      <div>
                        <div style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)" }}>Adjustment Delta</div>
                        <div
                          className="font-mono"
                          style={{
                            fontSize: "1.1rem",
                            fontWeight: 700,
                            color: isDeltaPositive ? "var(--accent-emerald)" : isDeltaNegative ? "var(--accent-rose)" : "var(--text-secondary)",
                          }}
                        >
                          {computedDeltaDisplay} {product.unit}
                        </div>
                      </div>
                      <div>
                        <div style={{ fontSize: "0.72rem", textTransform: "uppercase", color: "var(--text-muted)" }}>Resulting Stock</div>
                        <div className="font-mono" style={{ fontSize: "1.1rem", fontWeight: 700, color: "var(--accent-cyan)" }}>
                          {resultingQtyDisplay} {product.unit}
                        </div>
                      </div>
                    </div>
                  </div>
                );
              })()}

              {/* Adjustment Mode Switcher */}
              <div style={{ marginBottom: "1rem" }}>
                <label className="form-label" style={{ display: "block", marginBottom: "0.4rem", fontWeight: 600, fontSize: "0.85rem" }}>
                  Adjustment Method:
                </label>
                <div style={{ display: "flex", gap: "0.5rem" }}>
                  <button
                    type="button"
                    className={`filter-pill-btn ${adjustmentMode === "PHYSICAL_COUNT" ? "active" : ""}`}
                    onClick={() => setAdjustmentMode("PHYSICAL_COUNT")}
                    style={{ flex: 1, padding: "0.5rem" }}
                  >
                    Physical Count (Stocktake)
                  </button>
                  <button
                    type="button"
                    className={`filter-pill-btn ${adjustmentMode === "MANUAL_DELTA" ? "active" : ""}`}
                    onClick={() => setAdjustmentMode("MANUAL_DELTA")}
                    style={{ flex: 1, padding: "0.5rem" }}
                  >
                    Manual Adjustment (+ / -)
                  </button>
                </div>
              </div>

              {/* Quantity Inputs */}
              {adjustmentMode === "PHYSICAL_COUNT" ? (
                <div style={{ marginBottom: "1rem" }}>
                  <label className="form-label" style={{ display: "block", marginBottom: "0.4rem", fontWeight: 600, fontSize: "0.85rem" }}>
                    Actual Physical Count on Shelf:
                  </label>
                  <input
                    type="number"
                    step="any"
                    min="0"
                    placeholder="e.g. 15.000"
                    value={physicalCountInput}
                    onChange={(e) => setPhysicalCountInput(e.target.value)}
                    className="form-control font-mono"
                    style={{ width: "100%", padding: "0.6rem 0.8rem", borderRadius: "0.45rem", background: "var(--bg-input)", color: "var(--text-primary)", border: "1px solid var(--border-subtle)" }}
                  />
                  <span style={{ fontSize: "0.72rem", color: "var(--text-muted)", marginTop: "0.2rem", display: "block" }}>
                    Enter total measured stock. The system calculates the exact difference from recorded inventory.
                  </span>
                </div>
              ) : (
                <div style={{ marginBottom: "1rem" }}>
                  <label className="form-label" style={{ display: "block", marginBottom: "0.4rem", fontWeight: 600, fontSize: "0.85rem" }}>
                    Signed Delta (+ to add, - to remove):
                  </label>
                  <input
                    type="number"
                    step="any"
                    placeholder="e.g. -2 for damage, or +5 for found"
                    value={manualDeltaInput}
                    onChange={(e) => setManualDeltaInput(e.target.value)}
                    className="form-control font-mono"
                    style={{ width: "100%", padding: "0.6rem 0.8rem", borderRadius: "0.45rem", background: "var(--bg-input)", color: "var(--text-primary)", border: "1px solid var(--border-subtle)" }}
                  />
                </div>
              )}

              {/* Reason Selector */}
              <div style={{ marginBottom: "1rem" }}>
                <label className="form-label" style={{ display: "block", marginBottom: "0.4rem", fontWeight: 600, fontSize: "0.85rem" }}>
                  Discrepancy Classification (Reason):
                </label>
                <select
                  className="form-control"
                  style={{ width: "100%", padding: "0.6rem 0.8rem", borderRadius: "0.45rem", background: "var(--bg-input)", color: "var(--text-primary)", border: "1px solid var(--border-subtle)" }}
                  value={correctionReason}
                  onChange={(e) => setCorrectionReason(e.target.value as CorrectionReason)}
                >
                  <option value="MISCOUNT">MISCOUNT — Count discrepancy / Physical audit adjustment</option>
                  <option value="DAMAGED">DAMAGED — Broken, dented, packaging tear, spilled</option>
                  <option value="EXPIRED">EXPIRED — Past expiration date / spoiled perishable</option>
                  <option value="LOST">LOST — Shrinkage, theft, missing in storage</option>
                </select>
              </div>

              {/* Explanatory Audit Note */}
              <div style={{ marginBottom: "1.25rem" }}>
                <label className="form-label" style={{ display: "block", marginBottom: "0.4rem", fontWeight: 600, fontSize: "0.85rem" }}>
                  Audit Explanation (Mandatory):
                </label>
                <textarea
                  rows={2}
                  placeholder="Provide audit justification (e.g. Broken jar during unloading / Monthly stocktake variance)"
                  value={correctionNote}
                  onChange={(e) => setCorrectionNote(e.target.value)}
                  className="form-control"
                  style={{ width: "100%", padding: "0.6rem 0.8rem", borderRadius: "0.45rem", background: "var(--bg-input)", color: "var(--text-primary)", border: "1px solid var(--border-subtle)", resize: "none" }}
                />
              </div>

              {/* Prepare Action Button */}
              <button
                type="button"
                className="btn btn-primary"
                style={{ width: "100%", padding: "0.75rem", background: "linear-gradient(135deg, #d97706 0%, #f59e0b 100%)", borderColor: "#f59e0b", fontWeight: 600 }}
                onClick={handlePrepareStockCorrection}
                disabled={preparingCorrection || !selectedCorrectionProductId}
              >
                {preparingCorrection ? "Preparing Adjustment Quote..." : "Prepare Stock Adjustment"}
              </button>
            </section>

            {/* Right Column: Historical Audit Trail & Movement History */}
            <section className="panel-card" style={{ display: "flex", flexDirection: "column" }}>
              <div className="panel-header">
                <div>
                  <h2 className="panel-title">Stock Adjustments Audit Log</h2>
                  <p className="panel-desc">Authoritative audit ledger of all physical adjustments and shrinkage records.</p>
                </div>
                <div className="font-mono" style={{ fontSize: "0.85rem", color: "var(--accent-amber)" }}>
                  {correctionHistory.length} total adjustments
                </div>
              </div>

              {/* Filter Search */}
              <div style={{ marginBottom: "0.85rem" }}>
                <input
                  type="text"
                  placeholder="Search by product, reason, note, or admin..."
                  value={correctionsSearchQuery}
                  onChange={(e) => setCorrectionsSearchQuery(e.target.value)}
                  className="form-control"
                  style={{ width: "100%", padding: "0.5rem 0.75rem", borderRadius: "0.4rem", background: "var(--bg-input)", color: "var(--text-primary)", border: "1px solid var(--border-subtle)" }}
                />
              </div>

              {/* Corrections Table */}
              <div className="items-table-container" style={{ flex: 1, maxHeight: "560px", overflowY: "auto" }}>
                {filteredCorrectionHistory.length === 0 ? (
                  <div style={{ padding: "3rem", textAlign: "center", color: "var(--text-muted)" }}>
                    No stock adjustments recorded yet.
                  </div>
                ) : (
                  <table className="items-table">
                    <thead>
                      <tr>
                        <th style={{ width: "25%" }}>Product</th>
                        <th style={{ width: "15%" }}>Reason</th>
                        <th style={{ width: "18%" }}>Adjustment</th>
                        <th style={{ width: "20%" }}>Stock Before → After</th>
                        <th style={{ width: "22%" }}>Audit Note & Admin</th>
                      </tr>
                    </thead>
                    <tbody>
                      {filteredCorrectionHistory.map((c) => {
                        const deltaUnits = (c.quantityChange / 1000).toFixed(c.unit === "pcs" ? 0 : 3);
                        const isPos = c.quantityChange > 0;
                        const beforeUnits = (c.quantityBefore / 1000).toFixed(c.unit === "pcs" ? 0 : 3);
                        const afterUnits = (c.quantityAfter / 1000).toFixed(c.unit === "pcs" ? 0 : 3);

                        const reasonColorMap: Record<string, { bg: string; color: string }> = {
                          DAMAGED: { bg: "rgba(244, 63, 94, 0.15)", color: "#fb7185" },
                          EXPIRED: { bg: "rgba(249, 115, 22, 0.15)", color: "#fb923c" },
                          LOST: { bg: "rgba(168, 85, 247, 0.15)", color: "#c084fc" },
                          MISCOUNT: { bg: "rgba(56, 189, 248, 0.15)", color: "#38bdf8" },
                        };
                        const badgeStyle = reasonColorMap[c.reason] || { bg: "rgba(255,255,255,0.1)", color: "#fff" };

                        return (
                          <tr key={c.id}>
                            <td>
                              <div style={{ fontWeight: 600, color: "var(--text-primary)" }}>{c.productName}</div>
                              <div style={{ fontSize: "0.7rem", color: "var(--text-muted)" }}>
                                {new Date(c.createdAt).toLocaleString()}
                              </div>
                            </td>
                            <td>
                              <span
                                style={{
                                  padding: "0.2rem 0.5rem",
                                  borderRadius: "0.3rem",
                                  fontSize: "0.72rem",
                                  fontWeight: 700,
                                  background: badgeStyle.bg,
                                  color: badgeStyle.color,
                                }}
                              >
                                {c.reason}
                              </span>
                            </td>
                            <td>
                              <span
                                className="font-mono"
                                style={{
                                  fontWeight: 700,
                                  color: isPos ? "var(--accent-emerald)" : "var(--accent-rose)",
                                }}
                              >
                                {isPos ? `+${deltaUnits}` : deltaUnits} {c.unit}
                              </span>
                            </td>
                            <td>
                              <span className="font-mono" style={{ fontSize: "0.85rem", color: "var(--text-secondary)" }}>
                                {beforeUnits} → <strong style={{ color: "var(--text-primary)" }}>{afterUnits}</strong> {c.unit}
                              </span>
                            </td>
                            <td>
                              <div style={{ fontSize: "0.8rem", color: "var(--text-primary)" }}>{c.note}</div>
                              <div style={{ fontSize: "0.7rem", color: "var(--accent-amber)" }}>by {c.adminUsername}</div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                )}
              </div>
            </section>
          </div>
        </main>
      )}

      {/* ======================================================================================== */}
      {/* BUILD 12: OFFLINE SYNC & BACKUP WORKSPACE */}
      {/* ======================================================================================== */}
      {activeTab === "backups" && (
        <main className="purchase-workspace">
          {/* Top Metrics Cards */}
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))", gap: "1rem", marginBottom: "1.25rem" }}>
            {/* Auto Backup Card */}
            <div className="panel-card" style={{ padding: "1.2rem", display: "flex", flexDirection: "column", justifyContent: "space-between" }}>
              <div>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "0.5rem" }}>
                  <span className="panel-tag">Automated Protection</span>
                  <span
                    className="font-mono"
                    style={{
                      fontSize: "0.72rem",
                      fontWeight: 700,
                      padding: "0.2rem 0.5rem",
                      borderRadius: "0.3rem",
                      background: backupSettings?.autoBackupEnabled ? "rgba(16, 185, 129, 0.15)" : "rgba(148, 163, 184, 0.15)",
                      color: backupSettings?.autoBackupEnabled ? "var(--accent-emerald)" : "var(--text-muted)",
                    }}
                  >
                    {backupSettings?.autoBackupEnabled ? "ENABLED" : "PAUSED"}
                  </span>
                </div>
                <div style={{ fontSize: "1.1rem", fontWeight: 700, color: "var(--text-primary)", marginBottom: "0.25rem" }}>
                  Auto-Backup
                </div>
                <div style={{ fontSize: "0.78rem", color: "var(--text-muted)", marginBottom: "0.85rem" }}>
                  Maintains a rolling 7-backup retention limit. Oldest auto-backups are automatically cycled.
                </div>
              </div>
              <button
                type="button"
                className={`btn ${backupSettings?.autoBackupEnabled ? "btn-secondary" : "btn-primary"}`}
                style={{ width: "100%", padding: "0.5rem", fontSize: "0.82rem" }}
                disabled={togglingAutoBackup}
                onClick={() => void handleToggleAutoBackup(!backupSettings?.autoBackupEnabled)}
              >
                {togglingAutoBackup ? "Updating..." : backupSettings?.autoBackupEnabled ? "Disable Auto-Backup" : "Enable Auto-Backup"}
              </button>
            </div>

            {/* Catalog Breakdown Card */}
            <div className="panel-card" style={{ padding: "1.2rem", display: "flex", flexDirection: "column", justifyContent: "space-between" }}>
              <div>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "0.5rem" }}>
                  <span className="panel-tag">Snapshot Archives</span>
                  <span className="font-mono" style={{ fontSize: "0.72rem", color: "var(--accent-cyan)", fontWeight: 700 }}>
                    {backupsList.length} Total
                  </span>
                </div>
                <div style={{ fontSize: "1.1rem", fontWeight: 700, color: "var(--text-primary)", marginBottom: "0.25rem" }}>
                  {backupSettings?.manualBackupsCount ?? 0} Manual <span style={{ color: "var(--text-muted)", fontWeight: 400, fontSize: "0.9rem" }}>/</span> {backupSettings?.autoBackupsCount ?? 0} Auto
                </div>
                <div style={{ fontSize: "0.78rem", color: "var(--text-muted)" }}>
                  Manual checkpoints are never purged by retention policies.
                </div>
              </div>
              <div style={{ fontSize: "0.75rem", color: "var(--text-secondary)", marginTop: "0.85rem" }}>
                Retention: <strong style={{ color: "var(--text-primary)" }}>7 auto-snapshots</strong> max
              </div>
            </div>

            {/* Latest Backup Card */}
            <div className="panel-card" style={{ padding: "1.2rem", display: "flex", flexDirection: "column", justifyContent: "space-between" }}>
              <div>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "0.5rem" }}>
                  <span className="panel-tag">Freshness Check</span>
                  <span className="font-mono" style={{ fontSize: "0.72rem", color: "var(--accent-emerald)" }}>LIVE</span>
                </div>
                <div style={{ fontSize: "1.1rem", fontWeight: 700, color: "var(--text-primary)", marginBottom: "0.25rem" }}>
                  {backupSettings?.lastBackupAt ? new Date(backupSettings.lastBackupAt).toLocaleDateString() : "No Backups Yet"}
                </div>
                <div style={{ fontSize: "0.78rem", color: "var(--text-muted)" }}>
                  {backupSettings?.lastBackupAt ? new Date(backupSettings.lastBackupAt).toLocaleTimeString() : "Create a baseline snapshot"}
                </div>
              </div>
              <div style={{ fontSize: "0.75rem", color: "var(--text-secondary)", marginTop: "0.85rem" }}>
                Auto Cycle: <strong style={{ color: "var(--text-primary)" }}>{backupSettings?.lastAutoBackupAt ? new Date(backupSettings.lastAutoBackupAt).toLocaleTimeString() : "None Recorded"}</strong>
              </div>
            </div>

            {/* Architecture Sovereignty Card */}
            <div className="panel-card" style={{ padding: "1.2rem", display: "flex", flexDirection: "column", justifyContent: "space-between" }}>
              <div>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "0.5rem" }}>
                  <span className="panel-tag">Security & Integrity</span>
                  <span className="font-mono" style={{ fontSize: "0.72rem", color: "var(--accent-emerald)" }}>SHA-256</span>
                </div>
                <div style={{ fontSize: "1.1rem", fontWeight: 700, color: "var(--accent-emerald)", marginBottom: "0.25rem" }}>
                  100% Offline Sovereign
                </div>
                <div style={{ fontSize: "0.78rem", color: "var(--text-muted)" }}>
                  Atomic page snapshotting via SQLite Online Backup API. Zero cloud data leaks.
                </div>
              </div>
              <div style={{ fontSize: "0.75rem", color: "var(--accent-cyan)", marginTop: "0.85rem" }}>
                Durable Rollback Safe
              </div>
            </div>
          </div>

          {/* Backup Catalog Section */}
          <section className="panel-card" style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
            <div className="panel-header" style={{ marginBottom: "0.75rem" }}>
              <div>
                <h2 className="panel-title">Backup Catalog & Disaster Recovery</h2>
                <p className="panel-desc" style={{ fontSize: "0.78rem", color: "var(--text-muted)" }}>
                  Auditable SQLite database checkpoints. Each archive is protected with raw SHA-256 checksums and validated in sandbox prior to restore.
                </p>
              </div>
              <div style={{ display: "flex", gap: "0.75rem", alignItems: "center" }}>
                <button
                  type="button"
                  className="filter-pill-btn"
                  style={{ padding: "0.45rem 0.85rem", fontSize: "0.8rem" }}
                  onClick={() => void loadBackupStatus()}
                >
                  {loadingBackups ? "Loading..." : "↻ Refresh"}
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  style={{ padding: "0.5rem 1rem", fontSize: "0.85rem", display: "inline-flex", alignItems: "center", gap: "0.4rem" }}
                  onClick={() => setShowCreateBackupModal(true)}
                >
                  <span>+</span> Create Backup Now
                </button>
              </div>
            </div>

            {/* Filter & Search Toolbar */}
            <div style={{ display: "flex", gap: "0.75rem", marginBottom: "0.85rem", alignItems: "center" }}>
              <div style={{ display: "flex", gap: "0.35rem" }}>
                <button
                  type="button"
                  className={`filter-pill-btn ${backupFilter === "ALL" ? "active" : ""}`}
                  onClick={() => setBackupFilter("ALL")}
                >
                  All ({backupsList.length})
                </button>
                <button
                  type="button"
                  className={`filter-pill-btn ${backupFilter === "MANUAL" ? "active" : ""}`}
                  onClick={() => setBackupFilter("MANUAL")}
                >
                  Manual ({backupsList.filter((b) => b.backupType === "MANUAL").length})
                </button>
                <button
                  type="button"
                  className={`filter-pill-btn ${backupFilter === "AUTO" ? "active" : ""}`}
                  onClick={() => setBackupFilter("AUTO")}
                >
                  Auto ({backupsList.filter((b) => b.backupType === "AUTO").length})
                </button>
              </div>
              <div style={{ flex: 1 }}>
                <input
                  type="text"
                  className="form-control"
                  placeholder="Search by file name, date, checksum..."
                  value={backupSearchQuery}
                  onChange={(e) => setBackupSearchQuery(e.target.value)}
                  style={{ width: "100%", padding: "0.45rem 0.75rem", fontSize: "0.82rem", borderRadius: "0.4rem", background: "var(--bg-input)", border: "1px solid var(--border-subtle)", color: "var(--text-primary)" }}
                />
              </div>
            </div>

            {/* Backups Table */}
            <div style={{ flex: 1, overflowY: "auto", minHeight: 0, border: "1px solid var(--border-subtle)", borderRadius: "0.5rem" }}>
              {filteredBackups.length === 0 ? (
                <div style={{ padding: "3rem 1.5rem", textAlign: "center", color: "var(--text-muted)" }}>
                  <div style={{ fontSize: "1.1rem", fontWeight: 600, color: "var(--text-secondary)", marginBottom: "0.4rem" }}>
                    No Backup Snapshots Found
                  </div>
                  <div style={{ fontSize: "0.82rem" }}>
                    {backupSearchQuery
                      ? "No backups matched your search query."
                      : "Create your first manual backup snapshot or turn on automatic protection."}
                  </div>
                </div>
              ) : (
                <table className="items-table" style={{ width: "100%", borderCollapse: "collapse" }}>
                  <thead>
                    <tr style={{ background: "rgba(255,255,255,0.02)", textAlign: "left", fontSize: "0.75rem", color: "var(--text-muted)", textTransform: "uppercase" }}>
                      <th style={{ padding: "0.75rem 1rem" }}>Snapshot / File</th>
                      <th style={{ padding: "0.75rem 0.75rem" }}>Type</th>
                      <th style={{ padding: "0.75rem 0.75rem" }}>Size & Records</th>
                      <th style={{ padding: "0.75rem 0.75rem" }}>SHA-256 Checksum</th>
                      <th style={{ padding: "0.75rem 1rem", textAlign: "right" }}>Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {filteredBackups.map((b) => {
                      const isManual = b.backupType === "MANUAL";
                      const sizeKb = (b.fileSizeBytes / 1024).toFixed(1);
                      const dateStr = new Date(b.createdAt).toLocaleString();

                      return (
                        <tr key={b.id} style={{ borderTop: "1px solid var(--border-subtle)", fontSize: "0.82rem" }}>
                          <td style={{ padding: "0.75rem 1rem" }}>
                            <div className="font-mono" style={{ fontWeight: 600, color: "var(--text-primary)", fontSize: "0.82rem" }}>
                              {b.fileName}
                            </div>
                            <div style={{ fontSize: "0.72rem", color: "var(--text-muted)" }}>{dateStr}</div>
                          </td>
                          <td style={{ padding: "0.75rem 0.75rem" }}>
                            <span
                              style={{
                                padding: "0.2rem 0.55rem",
                                borderRadius: "0.3rem",
                                fontSize: "0.7rem",
                                fontWeight: 700,
                                background: isManual ? "rgba(16, 185, 129, 0.15)" : "rgba(99, 102, 241, 0.15)",
                                color: isManual ? "#34d399" : "#a5b4fc",
                              }}
                            >
                              {b.backupType}
                            </span>
                          </td>
                          <td style={{ padding: "0.75rem 0.75rem" }}>
                            <div className="font-mono" style={{ color: "var(--text-primary)" }}>{sizeKb} KB</div>
                            <div style={{ fontSize: "0.72rem", color: "var(--text-muted)" }}>{b.totalRecords} records</div>
                          </td>
                          <td style={{ padding: "0.75rem 0.75rem" }}>
                            <span className="font-mono" style={{ fontSize: "0.75rem", color: "var(--text-secondary)" }} title={b.checksumSha256}>
                              {b.checksumSha256.slice(0, 16)}...
                            </span>
                          </td>
                          <td style={{ padding: "0.75rem 1rem", textAlign: "right" }}>
                            <div style={{ display: "inline-flex", gap: "0.5rem" }}>
                              <button
                                type="button"
                                className="btn btn-secondary"
                                style={{ padding: "0.35rem 0.65rem", fontSize: "0.75rem" }}
                                onClick={() => void handleValidateBackup(b)}
                              >
                                Inspect
                              </button>
                              <button
                                type="button"
                                className="btn"
                                style={{
                                  padding: "0.35rem 0.65rem",
                                  fontSize: "0.75rem",
                                  background: "rgba(239, 68, 68, 0.12)",
                                  color: "#f87171",
                                  border: "1px solid rgba(239, 68, 68, 0.25)",
                                }}
                                onClick={() => void handleOpenRestoreModal(b)}
                              >
                                Restore
                              </button>
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </div>
          </section>
        </main>
      )}

      {/* ======================================================================================== */}
      {/* 4. MODALS: SALE QUOTE CONFIRMATION */}
      {/* ======================================================================================== */}
      {preparedSaleQuote && (
        <div className="modal-backdrop">
          <div className="modal-card">
            <div className="modal-header">
              <h3 className="modal-title">Confirm Sale & Generate Invoice</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setPreparedSaleQuote(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="quote-metadata-banner">
                <div>
                  <span className="meta-label">Invoice:</span>
                  <span className="meta-val font-mono">{preparedSaleQuote.saleNumber}</span>
                </div>
                <div>
                  <span className="meta-label">Customer:</span>
                  <span className="meta-val">{preparedSaleQuote.customerName || "Walk-in Customer"}</span>
                </div>
                <div>
                  <span className="meta-label">Preparation Token:</span>
                  <span className="meta-token">{preparedSaleQuote.preparationToken.slice(0, 16)}...</span>
                </div>
              </div>

              <div className="quote-items-preview">
                <table className="items-table">
                  <thead>
                    <tr>
                      <th>Product</th>
                      <th>Quantity</th>
                      <th>Selling Price</th>
                      <th>Line Total</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preparedSaleQuote.items.map((it) => (
                      <tr key={it.productId}>
                        <td>{it.productName}</td>
                        <td>{(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit}</td>
                        <td>₹{(it.unitPriceCents / 100).toFixed(2)}</td>
                        <td>₹{(it.lineTotalCents / 100).toFixed(2)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="quote-totals-grid">
                <div className="quote-total-box">
                  <span className="box-title">Total Sale</span>
                  <span className="box-amount">₹{(preparedSaleQuote.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Settlement Mode</span>
                  <span className="box-amount" style={{ color: preparedSaleQuote.settlementMode === "PAID" ? "var(--accent-emerald)" : "var(--accent-amber)" }}>
                    {preparedSaleQuote.settlementMode}
                  </span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">{preparedSaleQuote.settlementMode === "PAID" ? "Method" : "Khata Balance"}</span>
                  <span className="box-amount">
                    {preparedSaleQuote.settlementMode === "PAID" ? (preparedSaleQuote.paymentMethod || "CASH") : `Due: ₹${(preparedSaleQuote.creditAmountCents / 100).toFixed(2)}`}
                  </span>
                </div>
              </div>

              <div className="alert-box-info">
                <strong>Zero Oversell Stock Protection:</strong> Confirming this sale executes an atomic conditional stock decrement on SQLite and immediately commits the transaction.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setPreparedSaleQuote(null)}
                disabled={confirmingSale}
              >
                Back to Cart
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleConfirmSale}
                disabled={confirmingSale}
              >
                {confirmingSale ? "Committing..." : "Confirm & Issue Receipt"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 4. MODALS: SALE COMPLETED RECEIPT */}
      {/* ======================================================================================== */}
      {completedSaleReceipt && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "480px" }}>
            <div className="modal-header">
              <div className="receipt-header">
                <div className="receipt-check-icon">&#10003;</div>
                <div>
                  <h3 className="modal-title">Sale Complete & Committed</h3>
                  <div className="header-subtitle">Invoice #{completedSaleReceipt.saleNumber}</div>
                </div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setCompletedSaleReceipt(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="receipt-paper">
                <div style={{ textAlign: "center", marginBottom: "0.75rem" }}>
                  <strong style={{ fontSize: "1rem" }}>MERCHANT OS STORE</strong>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Local Sovereign Retail Outlet</div>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Tax Invoice / Bill of Sale</div>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>Invoice No:</span>
                  <strong>{completedSaleReceipt.saleNumber}</strong>
                </div>
                <div className="receipt-row">
                  <span>Date:</span>
                  <span>{new Date().toLocaleDateString()} {new Date().toLocaleTimeString()}</span>
                </div>
                {completedSaleReceipt.customerName && (
                  <div className="receipt-row">
                    <span>Customer:</span>
                    <span>{completedSaleReceipt.customerName} {completedSaleReceipt.customerPhone || ""}</span>
                  </div>
                )}
                <div className="receipt-row">
                  <span>Settlement:</span>
                  <strong>{completedSaleReceipt.settlementMode} {completedSaleReceipt.paymentMethod ? `(${completedSaleReceipt.paymentMethod})` : ""}</strong>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-items-list">
                  {completedSaleReceipt.items.map((it) => (
                    <div key={it.productId} className="receipt-item-line">
                      <div>
                        <div>{it.productName}</div>
                        <span className="receipt-sub">
                          {(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit} &times; ₹{(it.unitPriceCents / 100).toFixed(2)}
                        </span>
                      </div>
                      <div>₹{(it.totalCents / 100).toFixed(2)}</div>
                    </div>
                  ))}
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row receipt-total-row">
                  <span>TOTAL AMOUNT:</span>
                  <span>₹{(completedSaleReceipt.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="receipt-row">
                  <span>Paid:</span>
                  <span>₹{(completedSaleReceipt.paidAmountCents / 100).toFixed(2)}</span>
                </div>
                {completedSaleReceipt.creditAmountCents > 0 && (
                  <div className="receipt-row" style={{ color: "#d97706" }}>
                    <span>Khata Due:</span>
                    <span>₹{(completedSaleReceipt.creditAmountCents / 100).toFixed(2)}</span>
                  </div>
                )}

                <div className="divider-dashed" />
                <div style={{ textAlign: "center", fontSize: "0.7rem", color: "#64748b" }}>
                  Thank You for Your Business!
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => window.print()}
              >
                Print Receipt
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => setCompletedSaleReceipt(null)}
              >
                New Sale
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 5. MODALS: INLINE CREATE CUSTOMER */}
      {/* ======================================================================================== */}
      {showAddCustomerModal && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "420px" }}>
            <div className="modal-header">
              <h3 className="modal-title">Register Customer</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowAddCustomerModal(false)}
              >
                &times;
              </button>
            </div>

            <form onSubmit={handleCreateCustomer}>
              <div className="modal-body">
                <div className="form-group">
                  <label className="field-label">Customer Name *</label>
                  <input
                    type="text"
                    required
                    placeholder="e.g. Ramesh Sharma"
                    value={newCustomerName}
                    onChange={(e) => setNewCustomerName(e.target.value)}
                    className="input-field"
                    autoFocus
                  />
                </div>
                <div className="form-group">
                  <label className="field-label">Phone Number (Optional)</label>
                  <input
                    type="text"
                    placeholder="+919811100001"
                    value={newCustomerPhone}
                    onChange={(e) => setNewCustomerPhone(e.target.value)}
                    className="input-field"
                  />
                </div>
              </div>

              <div className="modal-footer">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowAddCustomerModal(false)}
                  disabled={addingCustomer}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="btn btn-primary"
                  disabled={addingCustomer}
                >
                  {addingCustomer ? "Saving..." : "Create & Select"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 6. MODALS: PURCHASE QUOTE CONFIRMATION */}
      {/* ======================================================================================== */}
      {preparedPurchaseQuote && (
        <div className="modal-backdrop">
          <div className="modal-card">
            <div className="modal-header">
              <h3 className="modal-title">Review & Confirm Inbound Purchase</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setPreparedPurchaseQuote(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="quote-metadata-banner">
                <div>
                  <span className="meta-label">PO Number:</span>
                  <span className="meta-val font-mono">{preparedPurchaseQuote.purchaseNumber}</span>
                </div>
                <div>
                  <span className="meta-label">Supplier:</span>
                  <span className="meta-val">{preparedPurchaseQuote.supplierName}</span>
                </div>
                <div>
                  <span className="meta-label">Preparation Token:</span>
                  <span className="meta-token">{preparedPurchaseQuote.preparationToken.slice(0, 16)}...</span>
                </div>
              </div>

              <div className="quote-items-preview">
                <table className="items-table">
                  <thead>
                    <tr>
                      <th>Product</th>
                      <th>Quantity</th>
                      <th>Buying Cost</th>
                      <th>Total</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preparedPurchaseQuote.items.map((it) => (
                      <tr key={it.productId}>
                        <td>{it.productName}</td>
                        <td>{(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit}</td>
                        <td>₹{(it.unitCostCents / 100).toFixed(2)}</td>
                        <td>₹{(it.lineTotalCents / 100).toFixed(2)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="quote-totals-grid">
                <div className="quote-total-box">
                  <span className="box-title">Total PO Amount</span>
                  <span className="box-amount">₹{(preparedPurchaseQuote.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Paid Amount</span>
                  <span className="box-amount text-emerald">₹{(preparedPurchaseQuote.paidAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Supplier Payable Due</span>
                  <span className="box-amount text-amber">₹{(preparedPurchaseQuote.creditAmountCents / 100).toFixed(2)}</span>
                </div>
              </div>

              <div className="alert-box-info">
                <strong>Two-Stage Confirmation:</strong> Stock quantities and supplier accounts will only be updated after confirmation.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setPreparedPurchaseQuote(null)}
                disabled={confirmingPurchase}
              >
                Back to Edit
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleConfirmPurchase}
                disabled={confirmingPurchase}
              >
                {confirmingPurchase ? "Committing..." : "Confirm & Update Inventory"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 7. MODALS: PURCHASE COMPLETED RECEIPT */}
      {/* ======================================================================================== */}
      {completedPurchaseReceipt && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "480px" }}>
            <div className="modal-header">
              <div className="receipt-header">
                <div className="receipt-check-icon">&#10003;</div>
                <div>
                  <h3 className="modal-title">Purchase Order Committed</h3>
                  <div className="header-subtitle">PO #{completedPurchaseReceipt.purchaseNumber}</div>
                </div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setCompletedPurchaseReceipt(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="receipt-paper">
                <div style={{ textAlign: "center", marginBottom: "0.75rem" }}>
                  <strong style={{ fontSize: "1rem" }}>MERCHANT OS GOODS INWARD</strong>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Inventory Receipt & Stock Voucher</div>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>PO Number:</span>
                  <strong>{completedPurchaseReceipt.purchaseNumber}</strong>
                </div>
                <div className="receipt-row">
                  <span>Supplier:</span>
                  <span>{completedPurchaseReceipt.supplierName}</span>
                </div>
                <div className="receipt-row">
                  <span>Status:</span>
                  <strong>{completedPurchaseReceipt.paymentStatus}</strong>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-items-list">
                  {completedPurchaseReceipt.items.map((it) => (
                    <div key={it.productId} className="receipt-item-line">
                      <div>
                        <div>{it.productName}</div>
                        <span className="receipt-sub">
                          {(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit} &times; ₹{(it.unitCostCents / 100).toFixed(2)}
                        </span>
                      </div>
                      <div>₹{(it.totalCents / 100).toFixed(2)}</div>
                    </div>
                  ))}
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row receipt-total-row">
                  <span>TOTAL COST:</span>
                  <span>₹{(completedPurchaseReceipt.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="receipt-row">
                  <span>Amount Paid:</span>
                  <span>₹{(completedPurchaseReceipt.paidAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="receipt-row">
                  <span>Payable Due:</span>
                  <span>₹{(completedPurchaseReceipt.creditAmountCents / 100).toFixed(2)}</span>
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => window.print()}
              >
                Print Voucher
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => setCompletedPurchaseReceipt(null)}
              >
                Close & New Order
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 8. MODALS: INLINE CREATE SUPPLIER */}
      {/* ======================================================================================== */}
      {showAddSupplierModal && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "420px" }}>
            <div className="modal-header">
              <h3 className="modal-title">Create New Supplier</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowAddSupplierModal(false)}
              >
                &times;
              </button>
            </div>

            <form onSubmit={handleCreateSupplier}>
              <div className="modal-body">
                <div className="form-group">
                  <label className="field-label">Supplier Name *</label>
                  <input
                    type="text"
                    required
                    placeholder="e.g. Punjab Agro Wholesalers"
                    value={newSupplierName}
                    onChange={(e) => setNewSupplierName(e.target.value)}
                    className="input-field"
                    autoFocus
                  />
                </div>
                <div className="form-group">
                  <label className="field-label">Phone Number (Optional)</label>
                  <input
                    type="text"
                    placeholder="+919811122233"
                    value={newSupplierPhone}
                    onChange={(e) => setNewSupplierPhone(e.target.value)}
                    className="input-field"
                  />
                </div>
              </div>

              <div className="modal-footer">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowAddSupplierModal(false)}
                  disabled={addingSupplier}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="btn btn-primary"
                  disabled={addingSupplier}
                >
                  {addingSupplier ? "Creating..." : "Save Supplier"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 9. MODALS: RECORD CUSTOMER PAYMENT (BUILD 08) */}
      {/* ======================================================================================== */}
      {showRecordPaymentModal && selectedCreditCustomer && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "460px" }}>
            <div className="modal-header">
              <div>
                <h3 className="modal-title">Record Khata Payment</h3>
                <div className="header-subtitle">{selectedCreditCustomer.name} {selectedCreditCustomer.phone ? `(${selectedCreditCustomer.phone})` : ""}</div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowRecordPaymentModal(false)}
              >
                &times;
              </button>
            </div>

            <form onSubmit={handlePreparePayment}>
              <div className="modal-body">
                {/* Current Outstanding Due Notice Box */}
                <div className="alert-box-info" style={{ background: "rgba(245, 158, 11, 0.1)", borderColor: "var(--accent-amber)", color: "#fde68a", marginBottom: "1rem" }}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                    <span>Current Outstanding Khata Due:</span>
                    <strong style={{ fontSize: "1.05rem" }}>
                      ₹{(selectedCreditCustomer.currentCreditCents / 100).toLocaleString("en-IN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                    </strong>
                  </div>
                </div>

                {/* Amount Input */}
                <div className="form-group">
                  <label className="field-label">
                    Settlement Amount (₹) <span className="label-required">* Mandatory</span>
                  </label>
                  <input
                    type="number"
                    step="0.01"
                    min="0.01"
                    max={(selectedCreditCustomer.currentCreditCents / 100).toFixed(2)}
                    required
                    placeholder="0.00"
                    value={paymentAmountRupees}
                    onChange={(e) => setPaymentAmountRupees(e.target.value)}
                    className="input-field"
                    style={{ fontSize: "1.2rem", fontWeight: 700 }}
                    autoFocus
                  />
                </div>

                {/* Preset Amount Buttons */}
                <div style={{ display: "flex", gap: "0.4rem", flexWrap: "wrap", marginBottom: "1rem" }}>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    style={{ fontSize: "0.75rem", padding: "0.25rem 0.6rem" }}
                    onClick={() => setPaymentAmountRupees((selectedCreditCustomer.currentCreditCents / 100).toFixed(2))}
                  >
                    Full Due (₹{(selectedCreditCustomer.currentCreditCents / 100).toFixed(2)})
                  </button>
                  {[500, 1000, 2000].map((preset) => {
                    const presetCents = preset * 100;
                    if (presetCents >= selectedCreditCustomer.currentCreditCents) return null;
                    return (
                      <button
                        key={preset}
                        type="button"
                        className="btn btn-secondary"
                        style={{ fontSize: "0.75rem", padding: "0.25rem 0.6rem" }}
                        onClick={() => setPaymentAmountRupees(preset.toFixed(2))}
                      >
                        ₹{preset.toLocaleString("en-IN")}
                      </button>
                    );
                  })}
                </div>

                {/* Payment Method */}
                <div className="form-group">
                  <label className="field-label">Payment Method *</label>
                  <select
                    value={paymentMethod}
                    onChange={(e) => setPaymentMethod(e.target.value as PaymentMethod)}
                    className="select-field"
                  >
                    <option value="CASH">CASH (Physical Currency)</option>
                    <option value="UPI">UPI (Google Pay, PhonePe, Paytm)</option>
                    <option value="BANK_TRANSFER">BANK TRANSFER (IMPS / NEFT)</option>
                    <option value="CARD">CARD (Debit / Credit POS)</option>
                  </select>
                </div>

                {/* Free-text Merchant Notes */}
                <div className="form-group">
                  <label className="field-label">Merchant Note (Optional)</label>
                  <input
                    type="text"
                    placeholder="e.g. GPay ref #9123847, Paid in shop by brother"
                    value={paymentNotes}
                    onChange={(e) => setPaymentNotes(e.target.value)}
                    className="input-field"
                  />
                </div>
              </div>

              <div className="modal-footer">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowRecordPaymentModal(false)}
                  disabled={preparingPayment}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="btn btn-primary"
                  disabled={preparingPayment || !paymentAmountRupees || parseFloat(paymentAmountRupees) <= 0}
                >
                  {preparingPayment ? "Preparing Quote..." : "Review Settlement"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 10. MODALS: REVIEW CUSTOMER PAYMENT QUOTE (CONFIRMATION BOUNDARY) */}
      {/* ======================================================================================== */}
      {preparedCustomerPaymentQuote && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "480px" }}>
            <div className="modal-header">
              <div>
                <h3 className="modal-title">Confirm Khata Settlement</h3>
                <div className="header-subtitle">Authoritative Merchant Confirmation Boundary</div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setPreparedCustomerPaymentQuote(null)}
                disabled={confirmingPayment}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="quote-metadata-banner">
                <div>
                  <span className="meta-label">Customer:</span>
                  <span className="meta-val">{preparedCustomerPaymentQuote.customerName}</span>
                </div>
                {preparedCustomerPaymentQuote.customerPhone && (
                  <div>
                    <span className="meta-label">Phone:</span>
                    <span className="meta-val">{preparedCustomerPaymentQuote.customerPhone}</span>
                  </div>
                )}
                <div>
                  <span className="meta-label">Token:</span>
                  <span className="meta-token font-mono">{preparedCustomerPaymentQuote.preparationToken.slice(0, 16)}...</span>
                </div>
              </div>

              <div className="quote-totals-grid" style={{ gridTemplateColumns: "1fr 1fr", margin: "1rem 0" }}>
                <div className="quote-total-box">
                  <span className="box-title">Balance Before</span>
                  <span className="box-amount" style={{ color: "var(--accent-amber)" }}>
                    ₹{(preparedCustomerPaymentQuote.balanceBeforeCents / 100).toFixed(2)}
                  </span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Settlement Amount</span>
                  <span className="box-amount" style={{ color: "var(--accent-emerald)" }}>
                    ₹{(preparedCustomerPaymentQuote.amountCents / 100).toFixed(2)}
                  </span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Balance After (Remaining)</span>
                  <span className="box-amount" style={{ color: preparedCustomerPaymentQuote.balanceAfterCents === 0 ? "var(--accent-emerald)" : "var(--accent-amber)" }}>
                    ₹{(preparedCustomerPaymentQuote.balanceAfterCents / 100).toFixed(2)}
                  </span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Payment Method</span>
                  <span className="box-amount">
                    {preparedCustomerPaymentQuote.paymentMethod}
                  </span>
                </div>
              </div>

              {preparedCustomerPaymentQuote.notes && (
                <div style={{ padding: "0.6rem 0.8rem", background: "rgba(255,255,255,0.03)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)", marginBottom: "1rem", fontSize: "0.82rem" }}>
                  <span style={{ color: "var(--text-muted)" }}>Merchant Note: </span>
                  <span>{preparedCustomerPaymentQuote.notes}</span>
                </div>
              )}

              <div className="alert-box-info">
                <strong>Atomic Conditional Decrement:</strong> Confirming this payment will atomically decrement the customer's credit balance in SQLite and append an immutable entry to the customer ledger.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => {
                  setPreparedCustomerPaymentQuote(null);
                  setShowRecordPaymentModal(true);
                }}
                disabled={confirmingPayment}
              >
                Back to Edit
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleConfirmPayment}
                disabled={confirmingPayment}
              >
                {confirmingPayment ? "Committing..." : "Confirm & Commit Payment"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 11. MODALS: COMPLETED PAYMENT RECEIPT */}
      {/* ======================================================================================== */}
      {completedCustomerPaymentReceipt && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "440px" }}>
            <div className="modal-header">
              <div className="receipt-header">
                <div className="receipt-check-icon">&#10003;</div>
                <div>
                  <h3 className="modal-title">Payment Settled</h3>
                  <div className="header-subtitle">Voucher #{completedCustomerPaymentReceipt.paymentId}</div>
                </div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setCompletedCustomerPaymentReceipt(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="receipt-paper">
                <div style={{ textAlign: "center", marginBottom: "0.75rem" }}>
                  <strong style={{ fontSize: "1rem" }}>MERCHANT OS STORE</strong>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Local Sovereign Retail Outlet</div>
                  <div style={{ fontSize: "0.75rem", fontWeight: 600, color: "#0f172a", marginTop: "0.2rem" }}>KHATA PAYMENT RECEIPT</div>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>Payment ID:</span>
                  <strong className="font-mono">{completedCustomerPaymentReceipt.paymentId}</strong>
                </div>
                <div className="receipt-row">
                  <span>Date & Time:</span>
                  <span>{new Date(completedCustomerPaymentReceipt.paymentDate).toLocaleDateString()} {new Date(completedCustomerPaymentReceipt.paymentDate).toLocaleTimeString()}</span>
                </div>
                <div className="receipt-row">
                  <span>Customer:</span>
                  <span>{completedCustomerPaymentReceipt.customerName} {completedCustomerPaymentReceipt.customerPhone || ""}</span>
                </div>
                <div className="receipt-row">
                  <span>Method:</span>
                  <strong>{completedCustomerPaymentReceipt.paymentMethod}</strong>
                </div>
                {completedCustomerPaymentReceipt.notes && (
                  <div className="receipt-row">
                    <span>Note:</span>
                    <span>{completedCustomerPaymentReceipt.notes}</span>
                  </div>
                )}

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>Previous Balance Due:</span>
                  <span>₹{(completedCustomerPaymentReceipt.balanceBeforeCents / 100).toFixed(2)}</span>
                </div>
                <div className="receipt-row receipt-total-row">
                  <span>AMOUNT RECEIVED:</span>
                  <span style={{ color: "var(--accent-emerald)" }}>₹{(completedCustomerPaymentReceipt.amountCents / 100).toFixed(2)}</span>
                </div>
                <div className="receipt-row" style={{ fontWeight: 600 }}>
                  <span>Remaining Due:</span>
                  <span style={{ color: completedCustomerPaymentReceipt.balanceAfterCents === 0 ? "#10b981" : "#d97706" }}>
                    ₹{(completedCustomerPaymentReceipt.balanceAfterCents / 100).toFixed(2)}
                  </span>
                </div>

                <div className="divider-dashed" />
                <div style={{ textAlign: "center", fontSize: "0.7rem", color: "#64748b" }}>
                  Payment Received with Thanks!
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => window.print()}
              >
                Print Receipt
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => setCompletedCustomerPaymentReceipt(null)}
              >
                Done
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 9. MODALS: CREATE / EDIT CUSTOMER ORDER */}
      {/* ======================================================================================== */}
      {showOrderModal && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "640px" }}>
            <div className="modal-header">
              <h3 className="modal-title">
                {editingOrderId ? "Edit Draft Customer Order" : "Create New Customer Order"}
              </h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowOrderModal(false)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              {/* Customer Selector */}
              <div className="form-group" style={{ marginBottom: "1rem" }}>
                <label className="field-label">Customer (Optional)</label>
                <select
                  value={orderFormCustomerId}
                  onChange={(e) => setOrderFormCustomerId(e.target.value)}
                  className="select-field"
                  disabled={loadingOrdersFormData}
                >
                  <option value="">Walk-in / Unregistered Customer</option>
                  {ordersFormData?.customers.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name} {c.phone ? `(${c.phone})` : ""} {c.currentBalanceCents > 0 ? `— Due: ₹${(c.currentBalanceCents / 100).toFixed(2)}` : ""}
                    </option>
                  ))}
                </select>
                <span style={{ fontSize: "0.72rem", color: "var(--text-muted)", marginTop: "0.2rem", display: "block" }}>
                  Orders can be created for walk-in shoppers or registered Khata customers.
                </span>
              </div>

              {/* Add Product Selector */}
              <div className="form-group" style={{ marginBottom: "1rem" }}>
                <label className="field-label">Add Product to Order</label>
                <select
                  onChange={(e) => {
                    if (e.target.value) {
                      addOrderFormItem(e.target.value);
                      e.target.value = "";
                    }
                  }}
                  className="select-field"
                  defaultValue=""
                  disabled={loadingOrdersFormData}
                >
                  <option value="" disabled>-- Select a product to add --</option>
                  {ordersFormData?.products.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} ({p.unit}) — ₹{(p.sellingPriceCents / 100).toFixed(2)} | Stock: {(p.currentQuantity / 1000).toFixed(p.productType === "LOOSE" ? 2 : 0)}
                    </option>
                  ))}
                </select>
              </div>

              {/* Order Items Table */}
              <div className="items-table-container" style={{ maxHeight: "240px", overflowY: "auto", marginBottom: "1rem" }}>
                <table className="items-table">
                  <thead>
                    <tr>
                      <th style={{ width: "40%" }}>Product</th>
                      <th style={{ width: "25%" }}>Quantity</th>
                      <th style={{ width: "25%" }}>Notes</th>
                      <th style={{ width: "10%", textAlign: "center" }}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {orderFormItems.length === 0 ? (
                      <tr>
                        <td colSpan={4} style={{ textAlign: "center", color: "var(--text-muted)", padding: "1.5rem" }}>
                          No products added yet. Select a product above to add to this order.
                        </td>
                      </tr>
                    ) : (
                      orderFormItems.map((item) => {
                        const prod = ordersFormData?.products.find((p) => p.id === item.productId);
                        const unitName = prod?.unit || "units";
                        return (
                          <tr key={item.id}>
                            <td>
                              <div className="item-name-cell">
                                <span className="item-name">{prod?.name || item.productId}</span>
                                {prod && <span className="type-badge">{prod.productType}</span>}
                              </div>
                            </td>
                            <td>
                              <div className="qty-input-group">
                                <input
                                  type="number"
                                  step={prod?.productType === "LOOSE" ? "0.001" : "1"}
                                  min="0.001"
                                  value={item.quantityDisplay}
                                  onChange={(e) => {
                                    const val = e.target.value;
                                    const parsed = parseFloat(val);
                                    const milli = isNaN(parsed) || parsed <= 0 ? 0 : Math.round(parsed * 1000);
                                    setOrderFormItems((prev) =>
                                      prev.map((x) => (x.id === item.id ? { ...x, quantityDisplay: val, quantityMillie: milli } : x))
                                    );
                                  }}
                                  className="qty-input"
                                />
                                <span className="qty-unit">{unitName}</span>
                              </div>
                            </td>
                            <td>
                              <input
                                type="text"
                                placeholder="Special instruction..."
                                value={item.notes}
                                onChange={(e) => {
                                  const val = e.target.value;
                                  setOrderFormItems((prev) =>
                                    prev.map((x) => (x.id === item.id ? { ...x, notes: val } : x))
                                  );
                                }}
                                className="input-field"
                                style={{ padding: "0.3rem 0.5rem", fontSize: "0.8rem" }}
                              />
                            </td>
                            <td style={{ textAlign: "center" }}>
                              <button
                                type="button"
                                className="delete-row-btn"
                                onClick={() => removeOrderFormItem(item.id)}
                              >
                                &times;
                              </button>
                            </td>
                          </tr>
                        );
                      })
                    )}
                  </tbody>
                </table>
              </div>

              {/* Order Notes */}
              <div className="form-group">
                <label className="field-label">Order Notes (Optional)</label>
                <textarea
                  rows={2}
                  placeholder="e.g., Hold for pickup at 5 PM, deliver to door..."
                  value={orderFormNotes}
                  onChange={(e) => setOrderFormNotes(e.target.value)}
                  className="input-field"
                />
              </div>

              <div className="alert-box-info" style={{ marginTop: "0.75rem" }}>
                <strong>Non-Mutating Draft Guarantee:</strong> Creating or editing an order does NOT mutate stock inventory, record a sale, or affect customer khata balance until converted.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setShowOrderModal(false)}
                disabled={savingOrder}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleSaveOrder}
                disabled={savingOrder || orderFormItems.length === 0}
              >
                {savingOrder ? "Saving..." : editingOrderId ? "Update Order" : "Create Draft Order"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 9. MODALS: CONVERT ORDER SETTLEMENT SELECTION */}
      {/* ======================================================================================== */}
      {showConvertOrderModal && convertOrderTarget && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "500px" }}>
            <div className="modal-header">
              <h3 className="modal-title">Convert Order to Sale</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowConvertOrderModal(false)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="quote-metadata-banner" style={{ marginBottom: "1rem" }}>
                <div>
                  <span className="meta-label">Order:</span>
                  <span className="meta-val font-mono">{convertOrderTarget.orderNumber}</span>
                </div>
                <div>
                  <span className="meta-label">Customer:</span>
                  <span className="meta-val">{convertOrderTarget.customerName || "Walk-in Customer"}</span>
                </div>
              </div>

              {/* Settlement Mode Selection */}
              <div className="form-group" style={{ marginBottom: "1rem" }}>
                <label className="field-label">Settlement Mode</label>
                <div className="mode-toggle-group">
                  <div
                    className={`mode-card ${convertSettlementMode === "PAID" ? "active-mode" : ""}`}
                    onClick={() => setConvertSettlementMode("PAID")}
                  >
                    <div className="mode-card-title">PAID (Full Settlement)</div>
                    <div className="mode-card-desc">Customer pays immediately via Cash/UPI/Card</div>
                  </div>
                  <div
                    className={`mode-card ${convertSettlementMode === "CREDIT" ? "active-mode" : ""} ${
                      !convertOrderTarget.customerId ? "disabled-mode" : ""
                    }`}
                    onClick={() => {
                      if (convertOrderTarget.customerId) {
                        setConvertSettlementMode("CREDIT");
                      }
                    }}
                    style={{
                      opacity: convertOrderTarget.customerId ? 1 : 0.4,
                      cursor: convertOrderTarget.customerId ? "pointer" : "not-allowed",
                    }}
                  >
                    <div className="mode-card-title">CREDIT (Khata Due)</div>
                    <div className="mode-card-desc">
                      {convertOrderTarget.customerId ? "Charge full amount to customer khata ledger" : "Requires registered customer"}
                    </div>
                  </div>
                </div>
              </div>

              {convertSettlementMode === "PAID" && (
                <div className="form-group" style={{ marginBottom: "1rem" }}>
                  <label className="field-label">Payment Method</label>
                  <select
                    value={convertPaymentMethod}
                    onChange={(e) => setConvertPaymentMethod(e.target.value as PaymentMethod)}
                    className="select-field"
                  >
                    <option value="CASH">CASH (Physical Currency)</option>
                    <option value="UPI">UPI (QR / Digital Payment)</option>
                    <option value="CARD">CARD (Debit / Credit Terminal)</option>
                    <option value="BANK_TRANSFER">BANK TRANSFER (Direct Ledger / IMPS)</option>
                  </select>
                </div>
              )}

              <div className="alert-box-info">
                <strong>Price Authority:</strong> Conversion locks in current catalog pricing from the database and verifies stock availability across all line items.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setShowConvertOrderModal(false)}
                disabled={preparingConversion}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handlePrepareConversion}
                disabled={preparingConversion}
              >
                {preparingConversion ? "Preparing Quote..." : "Review Quote & Convert \u2192"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 9. MODALS: REVIEW & CONFIRM ORDER CONVERSION QUOTE */}
      {/* ======================================================================================== */}
      {preparedOrderConversionQuote && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "600px" }}>
            <div className="modal-header">
              <h3 className="modal-title">Confirm Order Conversion</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setPreparedOrderConversionQuote(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="quote-metadata-banner">
                <div>
                  <span className="meta-label">Order:</span>
                  <span className="meta-val font-mono">{preparedOrderConversionQuote.orderNumber}</span>
                </div>
                <div>
                  <span className="meta-label">Customer:</span>
                  <span className="meta-val">{preparedOrderConversionQuote.customerName || "Walk-in Customer"}</span>
                </div>
                <div>
                  <span className="meta-label">Session Token:</span>
                  <span className="meta-token font-mono">{preparedOrderConversionQuote.preparationToken.slice(0, 16)}...</span>
                </div>
              </div>

              <div className="quote-items-preview">
                <table className="items-table">
                  <thead>
                    <tr>
                      <th>Product</th>
                      <th>Quantity</th>
                      <th>Catalog Price</th>
                      <th>Line Total</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preparedOrderConversionQuote.items.map((it) => (
                      <tr key={it.productId}>
                        <td>{it.productName}</td>
                        <td>{(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit}</td>
                        <td>₹{(it.unitPriceCents / 100).toFixed(2)}</td>
                        <td>₹{(it.lineTotalCents / 100).toFixed(2)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="quote-totals-grid">
                <div className="quote-total-box">
                  <span className="box-title">Total Sale</span>
                  <span className="box-amount">₹{(preparedOrderConversionQuote.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Settlement Mode</span>
                  <span className="box-amount" style={{ color: preparedOrderConversionQuote.settlementMode === "PAID" ? "var(--accent-emerald)" : "var(--accent-amber)" }}>
                    {preparedOrderConversionQuote.settlementMode}
                  </span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">{preparedOrderConversionQuote.settlementMode === "PAID" ? "Method" : "Khata Balance"}</span>
                  <span className="box-amount">
                    {preparedOrderConversionQuote.settlementMode === "PAID" ? (preparedOrderConversionQuote.paymentMethod || "CASH") : `Due: ₹${(preparedOrderConversionQuote.creditAmountCents / 100).toFixed(2)}`}
                  </span>
                </div>
              </div>

              <div className="alert-box-info" style={{ marginTop: "0.85rem" }}>
                <strong>Atomic Single-Execution Guarantee:</strong> Confirming executes an atomic conditional status transition (WHERE status = 'DRAFT'), decrements inventory, creates the sale invoice, and links the order. Cannot be converted twice.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setPreparedOrderConversionQuote(null)}
                disabled={confirmingConversion}
              >
                Back
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleConfirmConversion}
                disabled={confirmingConversion}
              >
                {confirmingConversion ? "Converting & Committing..." : "Confirm & Finalize Sale"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 10. MODALS: NEW CUSTOMER RETURN */}
      {/* ======================================================================================== */}
      {showCustomerReturnModal && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "720px" }}>
            <div className="modal-header">
              <div>
                <h3 className="modal-title">New Customer Return</h3>
                <div className="header-subtitle">Restock goods and apply Khata debt reduction or cash refund</div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowCustomerReturnModal(false)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              {/* Customer Selection Row */}
              <div className="form-group" style={{ marginBottom: "1rem" }}>
                <label className="field-label">Customer Account</label>
                <select
                  value={custReturnCustomerId}
                  onChange={(e) => setCustReturnCustomerId(e.target.value)}
                  className="select-field"
                >
                  <option value="">Walk-in Customer (No Khata — Direct Cash/UPI Refund)</option>
                  {returnsFormData?.customers.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name} {c.phone ? `(${c.phone})` : ""} — Due: ₹{(c.currentBalanceCents / 100).toFixed(2)}
                    </option>
                  ))}
                </select>
              </div>

              {/* Live Customer Debt Banner */}
              {custReturnCustomerId && (() => {
                const c = returnsFormData?.customers.find((cust) => cust.id === custReturnCustomerId);
                if (!c) return null;
                return (
                  <div
                    className="alert-box-info"
                    style={{
                      background: c.currentBalanceCents > 0 ? "rgba(245, 158, 11, 0.12)" : "rgba(16, 185, 129, 0.12)",
                      borderColor: c.currentBalanceCents > 0 ? "rgba(245, 158, 11, 0.3)" : "rgba(16, 185, 129, 0.3)",
                      color: c.currentBalanceCents > 0 ? "#fbbf24" : "#34d399",
                      marginBottom: "1rem",
                    }}
                  >
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                      <span>
                        Current Khata Outstanding Debt: <strong>₹{(c.currentBalanceCents / 100).toFixed(2)}</strong>
                      </span>
                      <span style={{ fontSize: "0.78rem" }}>
                        {c.currentBalanceCents > 0
                          ? "Return value automatically offsets debt first"
                          : "Zero debt — full amount paid out"}
                      </span>
                    </div>
                  </div>
                );
              })()}

              {/* Add Products Section */}
              <div style={{ marginBottom: "1rem", padding: "0.85rem", background: "var(--bg-card)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)" }}>
                <label className="field-label" style={{ marginBottom: "0.4rem" }}>Select Product to Return</label>
                <div style={{ display: "flex", gap: "0.5rem" }}>
                  <select
                    id="customer-return-prod-select"
                    className="select-field"
                    style={{ flex: 1 }}
                    defaultValue=""
                    onChange={(e) => {
                      if (e.target.value) {
                        addCustomerReturnProduct(e.target.value);
                        e.target.value = "";
                      }
                    }}
                  >
                    <option value="">-- Choose product from catalog --</option>
                    {returnsFormData?.products.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} ({p.unit}) — ₹{(p.sellingPriceCents / 100).toFixed(2)}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              {/* Returned Items Table */}
              <div className="items-table-container" style={{ maxHeight: "240px", overflowY: "auto", marginBottom: "1rem" }}>
                <table className="items-table">
                  <thead>
                    <tr>
                      <th style={{ width: "38%" }}>Product</th>
                      <th style={{ width: "22%" }}>Return Quantity</th>
                      <th style={{ width: "18%" }}>Catalog Price</th>
                      <th style={{ width: "16%" }}>Line Total</th>
                      <th style={{ width: "6%", textAlign: "center" }}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {custReturnItems.length === 0 ? (
                      <tr>
                        <td colSpan={5} className="empty-table-state">
                          <div className="empty-text">No items added to return</div>
                          <div className="empty-hint">Select a product from the catalog dropdown above</div>
                        </td>
                      </tr>
                    ) : (
                      custReturnItems.map((item) => {
                        const lineTotal = ((item.quantityMillie * item.unitPriceCents) / 100000).toFixed(2);
                        return (
                          <tr key={item.id}>
                            <td>
                              <div className="item-name-cell">
                                <span className="item-name">{item.productName}</span>
                                <span className="type-badge">{item.productType}</span>
                              </div>
                            </td>
                            <td>
                              <div className="qty-input-group">
                                <input
                                  type="number"
                                  step={item.productType === "LOOSE" ? "0.05" : "1"}
                                  min="0"
                                  value={item.quantityDisplay}
                                  onChange={(e) => handleCustomerReturnQtyChange(item.id, e.target.value)}
                                  className="qty-input"
                                />
                                <span className="qty-unit">{item.unit}</span>
                              </div>
                            </td>
                            <td>
                              <span className="readonly-price">₹{(item.unitPriceCents / 100).toFixed(2)}</span>
                            </td>
                            <td>
                              <span className="readonly-price" style={{ color: "var(--accent-emerald)" }}>₹{lineTotal}</span>
                            </td>
                            <td style={{ textAlign: "center" }}>
                              <button
                                type="button"
                                className="btn-table-del"
                                onClick={() => removeCustomerReturnItem(item.id)}
                                title="Remove item"
                              >
                                &times;
                              </button>
                            </td>
                          </tr>
                        );
                      })
                    )}
                  </tbody>
                </table>
              </div>

              {/* Payout Refund Method */}
              <div className="form-group" style={{ marginBottom: "1rem" }}>
                <label className="field-label">Refund Payment Method (for cash/digital payout)</label>
                <select
                  value={custReturnRefundMethod}
                  onChange={(e) => setCustReturnRefundMethod(e.target.value as PaymentMethod)}
                  className="select-field"
                >
                  <option value="CASH">CASH (Physical Currency)</option>
                  <option value="UPI">UPI (GPay / PhonePe / Paytm)</option>
                  <option value="CARD">Debit / Credit Card</option>
                  <option value="BANK_TRANSFER">Bank IMPS / NEFT</option>
                  <option value="OTHER">Other Method</option>
                </select>
              </div>

              {/* Estimated Totals Preview */}
              {(() => {
                const selectedCust = returnsFormData?.customers.find((c) => c.id === custReturnCustomerId);
                const custDebtCents = selectedCust ? selectedCust.currentBalanceCents : 0;
                const totalCents = custReturnItems.reduce(
                  (sum, it) => sum + Math.round((it.quantityMillie * it.unitPriceCents) / 1000),
                  0
                );
                const debtOffsetCents = Math.min(custDebtCents, totalCents);
                const cashRefundCents = Math.max(0, totalCents - debtOffsetCents);

                return (
                  <div className="quote-totals-grid" style={{ marginBottom: "0.5rem" }}>
                    <div className="quote-total-box">
                      <span className="box-title">Estimated Total</span>
                      <span className="box-amount">₹{(totalCents / 100).toFixed(2)}</span>
                    </div>
                    <div className="quote-total-box">
                      <span className="box-title">Debt Offset</span>
                      <span className="box-amount" style={{ color: "var(--accent-cyan)" }}>
                        ₹{(debtOffsetCents / 100).toFixed(2)}
                      </span>
                    </div>
                    <div className="quote-total-box">
                      <span className="box-title">Payout Refund</span>
                      <span className="box-amount" style={{ color: "var(--accent-emerald)" }}>
                        ₹{(cashRefundCents / 100).toFixed(2)}
                      </span>
                    </div>
                  </div>
                );
              })()}
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setShowCustomerReturnModal(false)}
                disabled={preparingCustReturn}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handlePrepareCustomerReturn}
                disabled={preparingCustReturn || custReturnItems.length === 0}
              >
                {preparingCustReturn ? "Preparing Quote..." : "Review Quote & Prepare Return \u2192"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 11. MODALS: REVIEW & CONFIRM CUSTOMER RETURN QUOTE */}
      {/* ======================================================================================== */}
      {preparedCustomerReturnQuote && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "620px" }}>
            <div className="modal-header">
              <h3 className="modal-title">Confirm Customer Return</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setPreparedCustomerReturnQuote(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="quote-metadata-banner">
                <div>
                  <span className="meta-label">Customer:</span>
                  <span className="meta-val">{preparedCustomerReturnQuote.customerName || "Walk-in Customer"}</span>
                </div>
                <div>
                  <span className="meta-label">Session Token:</span>
                  <span className="meta-token font-mono">{preparedCustomerReturnQuote.preparationToken.slice(0, 16)}...</span>
                </div>
              </div>

              <div className="quote-items-preview">
                <table className="items-table">
                  <thead>
                    <tr>
                      <th>Product</th>
                      <th>Quantity</th>
                      <th>Catalog Selling Price</th>
                      <th>Line Total</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preparedCustomerReturnQuote.items.map((it) => (
                      <tr key={it.productId}>
                        <td>{it.productName}</td>
                        <td>{(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit}</td>
                        <td>₹{(it.unitPriceCents / 100).toFixed(2)}</td>
                        <td>₹{(it.lineTotalCents / 100).toFixed(2)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="quote-totals-grid">
                <div className="quote-total-box">
                  <span className="box-title">Total Return Value</span>
                  <span className="box-amount">₹{(preparedCustomerReturnQuote.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Khata Debt Reduced</span>
                  <span className="box-amount" style={{ color: "var(--accent-cyan)" }}>
                    ₹{(preparedCustomerReturnQuote.debtReductionCents / 100).toFixed(2)}
                  </span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Refund Payout</span>
                  <span className="box-amount" style={{ color: "var(--accent-emerald)" }}>
                    ₹{(preparedCustomerReturnQuote.refundAmountCents / 100).toFixed(2)}
                  </span>
                  <span style={{ fontSize: "0.68rem", color: "var(--text-muted)" }}>
                    via {preparedCustomerReturnQuote.refundPaymentMethod || "CASH"}
                  </span>
                </div>
              </div>

              <div className="alert-box-info" style={{ marginTop: "0.85rem" }}>
                <strong>Guardrail 3 Revalidation Guarantee:</strong> The prepared quote is an interim review snapshot. When confirming, the backend revalidates live catalog prices, active customer status, and live customer debt. Client quotes are never stored blindly.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setPreparedCustomerReturnQuote(null)}
                disabled={confirmingCustReturn}
              >
                Back
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleConfirmCustomerReturn}
                disabled={confirmingCustReturn}
              >
                {confirmingCustReturn ? "Confirming & Restocking..." : "Confirm & Finalize Return"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 12. MODALS: CUSTOMER RETURN RECEIPT */}
      {/* ======================================================================================== */}
      {completedCustomerReturnReceipt && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "480px" }}>
            <div className="modal-header">
              <div className="receipt-header">
                <div className="receipt-check-icon">&#10003;</div>
                <div>
                  <h3 className="modal-title">Customer Return Processed</h3>
                  <div className="header-subtitle">Voucher #{completedCustomerReturnReceipt.returnNumber}</div>
                </div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setCompletedCustomerReturnReceipt(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="receipt-paper">
                <div style={{ textAlign: "center", marginBottom: "0.75rem" }}>
                  <strong style={{ fontSize: "1rem" }}>MERCHANT OS STORE</strong>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Local Sovereign Retail Outlet</div>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Customer Return Credit Voucher</div>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>Voucher No:</span>
                  <strong>{completedCustomerReturnReceipt.returnNumber}</strong>
                </div>
                <div className="receipt-row">
                  <span>Date:</span>
                  <span>{new Date().toLocaleDateString()} {new Date().toLocaleTimeString()}</span>
                </div>
                {completedCustomerReturnReceipt.customerName && (
                  <div className="receipt-row">
                    <span>Customer:</span>
                    <span>{completedCustomerReturnReceipt.customerName}</span>
                  </div>
                )}

                <div className="divider-dashed" />

                <div className="receipt-items-list">
                  {completedCustomerReturnReceipt.items.map((it) => (
                    <div key={it.productId} className="receipt-item-line">
                      <div>
                        <div>{it.productName}</div>
                        <span className="receipt-sub">
                          {(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit} &times; ₹{(it.unitPriceCents / 100).toFixed(2)}
                        </span>
                      </div>
                      <div>₹{(it.lineTotalCents / 100).toFixed(2)}</div>
                    </div>
                  ))}
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row receipt-total-row">
                  <span>TOTAL RETURN VALUE:</span>
                  <span>₹{(completedCustomerReturnReceipt.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                {completedCustomerReturnReceipt.debtReductionCents > 0 && (
                  <div className="receipt-row" style={{ color: "#38bdf8" }}>
                    <span>Khata Debt Reduced:</span>
                    <span>-₹{(completedCustomerReturnReceipt.debtReductionCents / 100).toFixed(2)}</span>
                  </div>
                )}
                {completedCustomerReturnReceipt.refundAmountCents > 0 && (
                  <div className="receipt-row" style={{ color: "#10b981" }}>
                    <span>Refund Payout ({completedCustomerReturnReceipt.refundPaymentMethod || "CASH"}):</span>
                    <span>₹{(completedCustomerReturnReceipt.refundAmountCents / 100).toFixed(2)}</span>
                  </div>
                )}

                <div className="divider-dashed" />
                <div style={{ textAlign: "center", fontSize: "0.7rem", color: "#64748b" }}>
                  Goods Restocked to Sovereign Inventory
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => window.print()}
              >
                Print Voucher
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => setCompletedCustomerReturnReceipt(null)}
              >
                Done
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 13. MODALS: NEW SUPPLIER RETURN */}
      {/* ======================================================================================== */}
      {showSupplierReturnModal && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "720px" }}>
            <div className="modal-header">
              <div>
                <h3 className="modal-title">New Supplier Return</h3>
                <div className="header-subtitle">Reverse stock back to vendor and create Supplier Credit Note</div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowSupplierReturnModal(false)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              {/* Supplier Selection Row */}
              <div className="form-group" style={{ marginBottom: "1rem" }}>
                <label className="field-label">
                  Supplier Account <span className="label-required">* Mandatory</span>
                </label>
                <select
                  value={suppReturnSupplierId}
                  onChange={(e) => setSuppReturnSupplierId(e.target.value)}
                  className="select-field"
                >
                  <option value="">-- Select Supplier --</option>
                  {returnsFormData?.suppliers.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name} {s.phone ? `(${s.phone})` : ""} — Balance: ₹{(s.currentOutstandingCents / 100).toFixed(2)}
                    </option>
                  ))}
                </select>
              </div>

              {/* Live Supplier Balance Banner */}
              {suppReturnSupplierId && (() => {
                const s = returnsFormData?.suppliers.find((supp) => supp.id === suppReturnSupplierId);
                if (!s) return null;
                const bal = s.currentOutstandingCents;
                return (
                  <div
                    className="alert-box-info"
                    style={{
                      background: bal < 0 ? "rgba(99, 102, 241, 0.12)" : "rgba(245, 158, 11, 0.12)",
                      borderColor: bal < 0 ? "rgba(99, 102, 241, 0.3)" : "rgba(245, 158, 11, 0.3)",
                      color: bal < 0 ? "#a5b4fc" : "#fbbf24",
                      marginBottom: "1rem",
                    }}
                  >
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                      <span>
                        Current Trade Balance:{" "}
                        <strong>
                          {bal < 0
                            ? `Credit Note / Advance: -₹${(Math.abs(bal) / 100).toFixed(2)}`
                            : `Due to Supplier: ₹${(bal / 100).toFixed(2)}`}
                        </strong>
                      </span>
                      <span style={{ fontSize: "0.78rem" }}>
                        Guardrail 2: Signed balance allowed without clamping
                      </span>
                    </div>
                  </div>
                );
              })()}

              {/* Add Products Section */}
              <div style={{ marginBottom: "1rem", padding: "0.85rem", background: "var(--bg-card)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)" }}>
                <label className="field-label" style={{ marginBottom: "0.4rem" }}>Select Product from Inventory</label>
                <div style={{ display: "flex", gap: "0.5rem" }}>
                  <select
                    id="supplier-return-prod-select"
                    className="select-field"
                    style={{ flex: 1 }}
                    defaultValue=""
                    onChange={(e) => {
                      if (e.target.value) {
                        addSupplierReturnProduct(e.target.value);
                        e.target.value = "";
                      }
                    }}
                  >
                    <option value="">-- Choose in-stock product from catalog --</option>
                    {returnsFormData?.products.map((p) => (
                      <option key={p.id} value={p.id} disabled={p.currentQuantity <= 0}>
                        {p.name} — Stock: {(p.currentQuantity / 1000).toFixed(p.productType === "LOOSE" ? 2 : 0)} {p.unit} — Cost: ₹{(p.costPriceCents / 100).toFixed(2)}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              {/* Return Items Table */}
              <div className="items-table-container" style={{ maxHeight: "240px", overflowY: "auto", marginBottom: "1rem" }}>
                <table className="items-table">
                  <thead>
                    <tr>
                      <th style={{ width: "36%" }}>Product & Stock</th>
                      <th style={{ width: "24%" }}>Return Quantity</th>
                      <th style={{ width: "18%" }}>Cost Price</th>
                      <th style={{ width: "16%" }}>Line Total</th>
                      <th style={{ width: "6%", textAlign: "center" }}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {suppReturnItems.length === 0 ? (
                      <tr>
                        <td colSpan={5} className="empty-table-state">
                          <div className="empty-text">No items added to supplier return</div>
                          <div className="empty-hint">Select an in-stock product from the dropdown above</div>
                        </td>
                      </tr>
                    ) : (
                      suppReturnItems.map((item) => {
                        const lineTotal = ((item.quantityMillie * item.unitPriceCents) / 100000).toFixed(2);
                        const isExceeded = item.quantityMillie > item.availableStockMillie;
                        return (
                          <tr key={item.id} className={isExceeded ? "row-error" : ""}>
                            <td>
                              <div className="item-name-cell">
                                <span className="item-name">{item.productName}</span>
                                <div className="item-badges">
                                  <span className="type-badge">{item.productType}</span>
                                  <span className={`stock-badge ${isExceeded ? "badge-danger" : ""}`}>
                                    Available: {(item.availableStockMillie / 1000).toFixed(item.productType === "LOOSE" ? 2 : 0)} {item.unit}
                                  </span>
                                </div>
                              </div>
                            </td>
                            <td>
                              <div className="qty-input-group">
                                <input
                                  type="number"
                                  step={item.productType === "LOOSE" ? "0.05" : "1"}
                                  min="0"
                                  max={(item.availableStockMillie / 1000).toString()}
                                  value={item.quantityDisplay}
                                  onChange={(e) => handleSupplierReturnQtyChange(item.id, e.target.value)}
                                  className={`qty-input ${isExceeded ? "input-error" : ""}`}
                                />
                                <span className="qty-unit">{item.unit}</span>
                              </div>
                            </td>
                            <td>
                              <span className="readonly-price">₹{(item.unitPriceCents / 100).toFixed(2)}</span>
                            </td>
                            <td>
                              <span className="readonly-price" style={{ color: "var(--accent-amber)" }}>₹{lineTotal}</span>
                            </td>
                            <td style={{ textAlign: "center" }}>
                              <button
                                type="button"
                                className="btn-table-del"
                                onClick={() => removeSupplierReturnItem(item.id)}
                                title="Remove item"
                              >
                                &times;
                              </button>
                            </td>
                          </tr>
                        );
                      })
                    )}
                  </tbody>
                </table>
              </div>

              {/* Return Reason Field */}
              <div className="form-group" style={{ marginBottom: "1rem" }}>
                <label className="field-label">Reason / Return Notes (Optional)</label>
                <input
                  type="text"
                  placeholder="e.g. Expired batch, packaging defect, vendor recall..."
                  value={suppReturnReason}
                  onChange={(e) => setSuppReturnReason(e.target.value)}
                  className="input-field"
                />
              </div>

              {/* Estimated Totals Preview */}
              {(() => {
                const selectedSupp = returnsFormData?.suppliers.find((s) => s.id === suppReturnSupplierId);
                const balBefore = selectedSupp ? selectedSupp.currentOutstandingCents : 0;
                const totalCents = suppReturnItems.reduce(
                  (sum, it) => sum + Math.round((it.quantityMillie * it.unitPriceCents) / 1000),
                  0
                );
                const balAfter = balBefore - totalCents;

                return (
                  <div className="quote-totals-grid" style={{ marginBottom: "0.5rem" }}>
                    <div className="quote-total-box">
                      <span className="box-title">Total Return Value</span>
                      <span className="box-amount">₹{(totalCents / 100).toFixed(2)}</span>
                    </div>
                    <div className="quote-total-box">
                      <span className="box-title">Balance Before</span>
                      <span className="box-amount">₹{(balBefore / 100).toFixed(2)}</span>
                    </div>
                    <div className="quote-total-box">
                      <span className="box-title">Resulting Balance</span>
                      <span className="box-amount" style={{ color: balAfter < 0 ? "var(--accent-indigo)" : "var(--accent-amber)" }}>
                        {balAfter < 0 ? `-₹${(Math.abs(balAfter) / 100).toFixed(2)}` : `₹${(balAfter / 100).toFixed(2)}`}
                      </span>
                    </div>
                  </div>
                );
              })()}
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setShowSupplierReturnModal(false)}
                disabled={preparingSuppReturn}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handlePrepareSupplierReturn}
                disabled={preparingSuppReturn || suppReturnItems.length === 0 || !suppReturnSupplierId}
              >
                {preparingSuppReturn ? "Preparing Quote..." : "Review Quote & Prepare Return \u2192"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 14. MODALS: REVIEW & CONFIRM SUPPLIER RETURN QUOTE */}
      {/* ======================================================================================== */}
      {preparedSupplierReturnQuote && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "620px" }}>
            <div className="modal-header">
              <h3 className="modal-title">Confirm Supplier Return & Stock Reversal</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setPreparedSupplierReturnQuote(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="quote-metadata-banner">
                <div>
                  <span className="meta-label">Supplier:</span>
                  <span className="meta-val">{preparedSupplierReturnQuote.supplierName}</span>
                </div>
                <div>
                  <span className="meta-label">Session Token:</span>
                  <span className="meta-token font-mono">{preparedSupplierReturnQuote.preparationToken.slice(0, 16)}...</span>
                </div>
              </div>

              <div className="quote-items-preview">
                <table className="items-table">
                  <thead>
                    <tr>
                      <th>Product</th>
                      <th>Quantity</th>
                      <th>Catalog Cost Price</th>
                      <th>Line Total</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preparedSupplierReturnQuote.items.map((it) => (
                      <tr key={it.productId}>
                        <td>{it.productName}</td>
                        <td>{(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit}</td>
                        <td>₹{(it.unitCostCents / 100).toFixed(2)}</td>
                        <td>₹{(it.lineTotalCents / 100).toFixed(2)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="quote-totals-grid">
                <div className="quote-total-box">
                  <span className="box-title">Total Return Value</span>
                  <span className="box-amount">₹{(preparedSupplierReturnQuote.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Balance Before</span>
                  <span className="box-amount">₹{(preparedSupplierReturnQuote.balanceBeforeCents / 100).toFixed(2)}</span>
                </div>
                <div className="quote-total-box">
                  <span className="box-title">Balance After</span>
                  {preparedSupplierReturnQuote.balanceAfterCents < 0 ? (
                    <span className="credit-note-pill" style={{ marginTop: "0.25rem" }}>
                      Credit Note: ₹{(Math.abs(preparedSupplierReturnQuote.balanceAfterCents) / 100).toFixed(2)}
                    </span>
                  ) : (
                    <span className="box-amount" style={{ color: "var(--accent-amber)" }}>
                      ₹{(preparedSupplierReturnQuote.balanceAfterCents / 100).toFixed(2)}
                    </span>
                  )}
                </div>
              </div>

              <div className="alert-box-info" style={{ marginTop: "0.85rem" }}>
                <strong>Guardrails 1 & 2 Guarantees:</strong> Inventory will be atomically decremented with strict conditional protection (<code>WHERE current_quantity &gt;= ?1</code>). Negative supplier balance represents an authoritative Credit Note that naturally offsets future credit purchases.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setPreparedSupplierReturnQuote(null)}
                disabled={confirmingSuppReturn}
              >
                Back
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleConfirmSupplierReturn}
                disabled={confirmingSuppReturn}
              >
                {confirmingSuppReturn ? "Decrementing Stock..." : "Confirm & Reverse Stock"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 15. MODALS: SUPPLIER RETURN RECEIPT */}
      {/* ======================================================================================== */}
      {completedSupplierReturnReceipt && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "480px" }}>
            <div className="modal-header">
              <div className="receipt-header">
                <div className="receipt-check-icon">&#10003;</div>
                <div>
                  <h3 className="modal-title">Supplier Return Finalized</h3>
                  <div className="header-subtitle">Debit Note #{completedSupplierReturnReceipt.returnNumber}</div>
                </div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setCompletedSupplierReturnReceipt(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="receipt-paper">
                <div style={{ textAlign: "center", marginBottom: "0.75rem" }}>
                  <strong style={{ fontSize: "1rem" }}>MERCHANT OS STORE</strong>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Local Sovereign Retail Outlet</div>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Supplier Return Debit Note</div>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>Debit Note No:</span>
                  <strong>{completedSupplierReturnReceipt.returnNumber}</strong>
                </div>
                <div className="receipt-row">
                  <span>Date:</span>
                  <span>{new Date().toLocaleDateString()} {new Date().toLocaleTimeString()}</span>
                </div>
                <div className="receipt-row">
                  <span>Supplier:</span>
                  <span>{completedSupplierReturnReceipt.supplierName}</span>
                </div>
                {completedSupplierReturnReceipt.reason && (
                  <div className="receipt-row">
                    <span>Reason:</span>
                    <span>{completedSupplierReturnReceipt.reason}</span>
                  </div>
                )}

                <div className="divider-dashed" />

                <div className="receipt-items-list">
                  {completedSupplierReturnReceipt.items.map((it) => (
                    <div key={it.productId} className="receipt-item-line">
                      <div>
                        <div>{it.productName}</div>
                        <span className="receipt-sub">
                          {(it.quantity / 1000).toFixed(it.unit === "pcs" ? 0 : 3)} {it.unit} &times; ₹{(it.unitCostCents / 100).toFixed(2)}
                        </span>
                      </div>
                      <div>₹{(it.lineTotalCents / 100).toFixed(2)}</div>
                    </div>
                  ))}
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row receipt-total-row">
                  <span>TOTAL RETURN VALUE:</span>
                  <span>₹{(completedSupplierReturnReceipt.totalAmountCents / 100).toFixed(2)}</span>
                </div>
                <div className="receipt-row">
                  <span>Balance Before:</span>
                  <span>₹{(completedSupplierReturnReceipt.balanceBeforeCents / 100).toFixed(2)}</span>
                </div>
                <div className="receipt-row" style={{ color: completedSupplierReturnReceipt.balanceAfterCents < 0 ? "#818cf8" : "#fbbf24" }}>
                  <span>Trade Balance After:</span>
                  <span>
                    {completedSupplierReturnReceipt.balanceAfterCents < 0
                      ? `Credit Note: -₹${(Math.abs(completedSupplierReturnReceipt.balanceAfterCents) / 100).toFixed(2)}`
                      : `Due: ₹${(completedSupplierReturnReceipt.balanceAfterCents / 100).toFixed(2)}`}
                  </span>
                </div>

                <div className="divider-dashed" />
                <div style={{ textAlign: "center", fontSize: "0.7rem", color: "#64748b" }}>
                  Inventory Stock Decremented Atomically
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => window.print()}
              >
                Print Debit Note
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => setCompletedSupplierReturnReceipt(null)}
              >
                Done
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 16. MODALS: BUILD 11 STOCK CORRECTION QUOTE CONFIRMATION */}
      {/* ======================================================================================== */}
      {preparedCorrectionQuote && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "520px" }}>
            <div className="modal-header">
              <div>
                <h3 className="modal-title">Authorize Stock Adjustment</h3>
                <div className="header-subtitle">Admin Confirmation Boundary</div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setPreparedCorrectionQuote(null)}
                disabled={confirmingCorrection}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div style={{ padding: "0.75rem", background: "rgba(245, 158, 11, 0.1)", borderRadius: "0.5rem", border: "1px solid rgba(245, 158, 11, 0.3)", marginBottom: "1rem" }}>
                <span style={{ fontSize: "0.75rem", color: "#fbbf24", fontWeight: 600, display: "block" }}>
                  ADMINISTRATIVE AUDIT MANDATE
                </span>
                <span style={{ fontSize: "0.8rem", color: "var(--text-secondary)" }}>
                  Confirming this quote permanently updates SQLite inventory, records a signed movement entry in <code>stock_movements</code>, and logs an immutable record in <code>audit_logs</code>.
                </span>
              </div>

              <div className="receipt-paper" style={{ padding: "1rem" }}>
                <div className="receipt-row">
                  <span>Product:</span>
                  <strong>{preparedCorrectionQuote.productName}</strong>
                </div>
                <div className="receipt-row">
                  <span>Category/Type:</span>
                  <span>{preparedCorrectionQuote.productType}</span>
                </div>
                <div className="receipt-row">
                  <span>Reason:</span>
                  <span style={{ fontWeight: 700, color: "var(--accent-amber)" }}>{preparedCorrectionQuote.reason}</span>
                </div>
                <div className="receipt-row">
                  <span>Authorized By:</span>
                  <span>{preparedCorrectionQuote.adminUsername} (Administrator)</span>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>Recorded Stock Before:</span>
                  <span className="font-mono">
                    {(preparedCorrectionQuote.quantityBefore / 1000).toFixed(preparedCorrectionQuote.unit === "pcs" ? 0 : 3)} {preparedCorrectionQuote.unit}
                  </span>
                </div>
                <div className="receipt-row">
                  <span>Adjustment Delta:</span>
                  <span
                    className="font-mono"
                    style={{
                      fontWeight: 700,
                      color: preparedCorrectionQuote.quantityChange > 0 ? "var(--accent-emerald)" : "var(--accent-rose)",
                    }}
                  >
                    {preparedCorrectionQuote.quantityChange > 0 ? "+" : ""}
                    {(preparedCorrectionQuote.quantityChange / 1000).toFixed(preparedCorrectionQuote.unit === "pcs" ? 0 : 3)} {preparedCorrectionQuote.unit}
                  </span>
                </div>
                <div className="receipt-row receipt-total-row">
                  <span>RESULTING NEW STOCK:</span>
                  <span className="font-mono" style={{ color: "var(--accent-cyan)" }}>
                    {(preparedCorrectionQuote.quantityAfter / 1000).toFixed(preparedCorrectionQuote.unit === "pcs" ? 0 : 3)} {preparedCorrectionQuote.unit}
                  </span>
                </div>

                <div className="divider-dashed" />

                <div style={{ marginTop: "0.5rem" }}>
                  <div style={{ fontSize: "0.75rem", color: "var(--text-muted)", textTransform: "uppercase" }}>Audit Note:</div>
                  <div style={{ fontSize: "0.85rem", color: "var(--text-primary)", marginTop: "0.2rem", fontStyle: "italic" }}>
                    &ldquo;{preparedCorrectionQuote.note}&rdquo;
                  </div>
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setPreparedCorrectionQuote(null)}
                disabled={confirmingCorrection}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-primary"
                style={{ background: "linear-gradient(135deg, #d97706 0%, #f59e0b 100%)", borderColor: "#f59e0b" }}
                onClick={handleConfirmStockCorrection}
                disabled={confirmingCorrection}
              >
                {confirmingCorrection ? "Committing to Database..." : "Confirm & Commit to Inventory"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* 17. MODALS: BUILD 11 STOCK CORRECTION RECEIPT */}
      {/* ======================================================================================== */}
      {completedCorrectionReceipt && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "480px" }}>
            <div className="modal-header">
              <div className="receipt-header">
                <div className="receipt-check-icon">&#10003;</div>
                <div>
                  <h3 className="modal-title">Stock Adjustment Applied</h3>
                  <div className="header-subtitle">Movement #{completedCorrectionReceipt.correctionId.slice(0, 18)}</div>
                </div>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setCompletedCorrectionReceipt(null)}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              <div className="receipt-paper">
                <div style={{ textAlign: "center", marginBottom: "0.75rem" }}>
                  <strong style={{ fontSize: "1rem" }}>MERCHANT OS STORE</strong>
                  <div style={{ fontSize: "0.7rem", color: "#64748b" }}>Inventory Audit & Reconciliation Slip</div>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>Correction ID:</span>
                  <span className="font-mono">{completedCorrectionReceipt.correctionId.slice(0, 16)}</span>
                </div>
                <div className="receipt-row">
                  <span>Product:</span>
                  <strong>{completedCorrectionReceipt.productName}</strong>
                </div>
                <div className="receipt-row">
                  <span>Reason:</span>
                  <span style={{ fontWeight: 700 }}>{completedCorrectionReceipt.reason}</span>
                </div>
                <div className="receipt-row">
                  <span>Admin User:</span>
                  <span>{completedCorrectionReceipt.adminUsername}</span>
                </div>
                <div className="receipt-row">
                  <span>Timestamp:</span>
                  <span>{new Date().toLocaleDateString()} {new Date().toLocaleTimeString()}</span>
                </div>

                <div className="divider-dashed" />

                <div className="receipt-row">
                  <span>Stock Before:</span>
                  <span>{(completedCorrectionReceipt.quantityBefore / 1000).toFixed(completedCorrectionReceipt.unit === "pcs" ? 0 : 3)} {completedCorrectionReceipt.unit}</span>
                </div>
                <div className="receipt-row">
                  <span>Adjustment Applied:</span>
                  <strong style={{ color: completedCorrectionReceipt.quantityChange > 0 ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                    {completedCorrectionReceipt.quantityChange > 0 ? "+" : ""}
                    {(completedCorrectionReceipt.quantityChange / 1000).toFixed(completedCorrectionReceipt.unit === "pcs" ? 0 : 3)} {completedCorrectionReceipt.unit}
                  </strong>
                </div>
                <div className="receipt-row receipt-total-row">
                  <span>CURRENT INVENTORY:</span>
                  <span>{(completedCorrectionReceipt.quantityAfter / 1000).toFixed(completedCorrectionReceipt.unit === "pcs" ? 0 : 3)} {completedCorrectionReceipt.unit}</span>
                </div>

                <div className="divider-dashed" />

                <div style={{ fontSize: "0.75rem", color: "#64748b" }}>
                  <div><strong>Note:</strong> {completedCorrectionReceipt.note}</div>
                  <div style={{ marginTop: "0.4rem", textAlign: "center" }}>
                    Atomic movement logged in SQLite <code>stock_movements</code>.
                  </div>
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => window.print()}
              >
                Print Slip
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => setCompletedCorrectionReceipt(null)}
              >
                Done
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* BUILD 12: CREATE MANUAL BACKUP MODAL */}
      {/* ======================================================================================== */}
      {showCreateBackupModal && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "500px" }}>
            <div className="modal-header">
              <h3 className="modal-title">Create Manual Backup Snapshot</h3>
              <button type="button" className="modal-close" onClick={() => setShowCreateBackupModal(false)}>
                &times;
              </button>
            </div>

            <div className="modal-body">
              <p style={{ fontSize: "0.82rem", color: "var(--text-muted)", marginBottom: "1rem" }}>
                Creates a point-in-time snapshot of the authoritative SQLite database. Manual snapshots are permanent and excluded from automatic rotation limits.
              </p>

              <div style={{ marginBottom: "1rem" }}>
                <label className="form-label" style={{ display: "block", marginBottom: "0.4rem", fontWeight: 600, fontSize: "0.85rem" }}>
                  Optional Audit Note:
                </label>
                <input
                  type="text"
                  className="form-control"
                  placeholder="e.g. Pre-stocktake baseline, month-end close..."
                  value={manualBackupNote}
                  onChange={(e) => setManualBackupNote(e.target.value)}
                  style={{ width: "100%", padding: "0.6rem 0.8rem", borderRadius: "0.45rem", background: "var(--bg-input)", color: "var(--text-primary)", border: "1px solid var(--border-subtle)" }}
                />
              </div>

              <div className="alert-box-info" style={{ fontSize: "0.78rem" }}>
                <strong>Zero Downtime:</strong> The snapshot is generated using the SQLite Online Backup API, ensuring active POS operations remain uninterrupted.
              </div>
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setShowCreateBackupModal(false)}
                disabled={creatingBackup}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => void handleCreateManualBackup()}
                disabled={creatingBackup}
              >
                {creatingBackup ? "Generating Snapshot..." : "Create Snapshot Now"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* BUILD 12: BACKUP INSPECTION & VALIDATION REPORT MODAL */}
      {/* ======================================================================================== */}
      {showValidationModal && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "600px" }}>
            <div className="modal-header">
              <h3 className="modal-title">Backup Integrity Inspection</h3>
              <button type="button" className="modal-close" onClick={() => setShowValidationModal(false)}>
                &times;
              </button>
            </div>

            <div className="modal-body">
              {validatingBackup || !validationReport ? (
                <div style={{ padding: "2rem", textAlign: "center", color: "var(--text-muted)" }}>
                  <div>Inspecting archive and computing SHA-256 checksums...</div>
                </div>
              ) : (
                <>
                  <div style={{ marginBottom: "1rem" }}>
                    <div style={{ fontSize: "0.72rem", color: "var(--text-muted)", textTransform: "uppercase" }}>Inspected Target</div>
                    <div className="font-mono" style={{ fontWeight: 700, fontSize: "0.95rem", color: "var(--text-primary)" }}>
                      {selectedBackupForAction?.fileName}
                    </div>
                  </div>

                  {/* Verification Grid */}
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "0.6rem", marginBottom: "1.25rem" }}>
                    <div style={{ padding: "0.75rem", borderRadius: "0.45rem", background: "rgba(255,255,255,0.02)", border: "1px solid var(--border-subtle)" }}>
                      <div style={{ fontSize: "0.7rem", color: "var(--text-muted)" }}>SHA-256 Checksum & File</div>
                      <div style={{ fontWeight: 700, fontSize: "0.85rem", color: validationReport.integrityCheckPassed ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                        {validationReport.integrityCheckPassed ? "✓ MATCHED & INTACT" : "✗ CORRUPTED"}
                      </div>
                    </div>

                    <div style={{ padding: "0.75rem", borderRadius: "0.45rem", background: "rgba(255,255,255,0.02)", border: "1px solid var(--border-subtle)" }}>
                      <div style={{ fontSize: "0.7rem", color: "var(--text-muted)" }}>Foreign Key Constraints</div>
                      <div style={{ fontWeight: 700, fontSize: "0.85rem", color: validationReport.foreignKeyCheckPassed ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                        {validationReport.foreignKeyCheckPassed ? "✓ CONSTRAINTS SATISFIED" : "✗ ORPHANS DETECTED"}
                      </div>
                    </div>

                    <div style={{ padding: "0.75rem", borderRadius: "0.45rem", background: "rgba(255,255,255,0.02)", border: "1px solid var(--border-subtle)" }}>
                      <div style={{ fontSize: "0.7rem", color: "var(--text-muted)" }}>Foundational Schema Tables</div>
                      <div style={{ fontWeight: 700, fontSize: "0.85rem", color: validationReport.schemaTablesPassed ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                        {validationReport.schemaTablesPassed ? "✓ ALL TABLES VERIFIED" : "✗ MISSING CORE TABLES"}
                      </div>
                    </div>

                    <div style={{ padding: "0.75rem", borderRadius: "0.45rem", background: "rgba(255,255,255,0.02)", border: "1px solid var(--border-subtle)" }}>
                      <div style={{ fontSize: "0.7rem", color: "var(--text-muted)" }}>Compatibility & Invariants</div>
                      <div style={{ fontWeight: 700, fontSize: "0.85rem", color: validationReport.compatibilityStatus === "COMPATIBLE" ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                        {validationReport.compatibilityStatus === "COMPATIBLE" ? "✓ FULLY COMPATIBLE" : "✗ INCOMPATIBLE"}
                      </div>
                    </div>
                  </div>

                  {/* Tables Found List */}
                  {validationReport.manifest?.tableCounts && (
                    <div style={{ marginBottom: "1rem" }}>
                      <div style={{ fontSize: "0.75rem", fontWeight: 600, color: "var(--text-secondary)", marginBottom: "0.4rem" }}>
                        Archive Record Breakdown:
                      </div>
                      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(140px, 1fr))", gap: "0.4rem" }}>
                        {Object.entries(validationReport.manifest.tableCounts).map(([tbl, cnt]) => (
                          <div key={tbl} style={{ padding: "0.4rem 0.6rem", background: "rgba(0,0,0,0.2)", borderRadius: "0.3rem", fontSize: "0.72rem" }}>
                            <span style={{ color: "var(--text-muted)" }}>{tbl}:</span>{" "}
                            <strong className="font-mono" style={{ color: "var(--text-primary)" }}>{cnt}</strong>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {validationReport.errors.length > 0 && (
                    <div className="alert-box-error" style={{ fontSize: "0.78rem", color: "var(--accent-rose)", background: "rgba(244,63,94,0.1)", padding: "0.6rem", borderRadius: "0.4rem" }}>
                      {validationReport.errors.map((err, i) => (
                        <div key={i}>• {err}</div>
                      ))}
                    </div>
                  )}
                </>
              )}
            </div>

            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setShowValidationModal(false)}
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ======================================================================================== */}
      {/* BUILD 12: AUTHORITATIVE DATABASE RESTORE MODAL (WITH SAFETY GUARD) */}
      {/* ======================================================================================== */}
      {showRestoreModal && restoreCandidate && (
        <div className="modal-backdrop">
          <div className="modal-card" style={{ maxWidth: "620px" }}>
            <div className="modal-header">
              <h3 className="modal-title" style={{ color: "var(--accent-rose)" }}>
                Authoritative Database Restoration
              </h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => {
                  setShowRestoreModal(false);
                  setRestoreCandidate(null);
                  setRestoreConfirmedCheck(false);
                }}
              >
                &times;
              </button>
            </div>

            <div className="modal-body">
              {/* Target Snapshot Details */}
              <div style={{ padding: "0.85rem", background: "rgba(255,255,255,0.03)", borderRadius: "0.5rem", border: "1px solid var(--border-subtle)", marginBottom: "1rem" }}>
                <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "0.25rem" }}>
                  <span style={{ fontSize: "0.72rem", color: "var(--text-muted)" }}>RESTORING SNAPSHOT:</span>
                  <span className="font-mono" style={{ fontSize: "0.75rem", color: "var(--accent-cyan)" }}>
                    {restoreCandidate.backupType}
                  </span>
                </div>
                <div className="font-mono" style={{ fontWeight: 700, fontSize: "0.95rem", color: "var(--text-primary)", marginBottom: "0.25rem" }}>
                  {restoreCandidate.fileName}
                </div>
                <div style={{ fontSize: "0.75rem", color: "var(--text-muted)" }}>
                  Captured: {new Date(restoreCandidate.createdAt).toLocaleString()} • {restoreCandidate.totalRecords} total records
                </div>
              </div>

              {/* Requirement 8: Pre-Restore Integrity Checklist */}
              <div style={{ marginBottom: "1rem" }}>
                <div style={{ fontSize: "0.75rem", fontWeight: 600, color: "var(--text-secondary)", marginBottom: "0.4rem" }}>
                  Pre-Flight Sandbox Validation:
                </div>
                {loadingRestoreValidation ? (
                  <div style={{ padding: "0.75rem", textAlign: "center", color: "var(--text-muted)", fontSize: "0.8rem" }}>
                    Validating SHA-256 checksum and foreign key invariants...
                  </div>
                ) : restoreValidationReport ? (
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "0.5rem" }}>
                    <div style={{ padding: "0.5rem 0.75rem", borderRadius: "0.4rem", background: "rgba(0,0,0,0.2)", fontSize: "0.75rem" }}>
                      <span style={{ color: "var(--text-muted)" }}>Validation Status: </span>
                      <strong style={{ color: restoreValidationReport.isValid ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                        {restoreValidationReport.isValid ? "VALID" : "INVALID"}
                      </strong>
                    </div>
                    <div style={{ padding: "0.5rem 0.75rem", borderRadius: "0.4rem", background: "rgba(0,0,0,0.2)", fontSize: "0.75rem" }}>
                      <span style={{ color: "var(--text-muted)" }}>Integrity / Checksum: </span>
                      <strong style={{ color: restoreValidationReport.integrityCheckPassed ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                        {restoreValidationReport.integrityCheckPassed ? "PASSED" : "FAILED"}
                      </strong>
                    </div>
                    <div style={{ padding: "0.5rem 0.75rem", borderRadius: "0.4rem", background: "rgba(0,0,0,0.2)", fontSize: "0.75rem" }}>
                      <span style={{ color: "var(--text-muted)" }}>Foreign-Key Check: </span>
                      <strong style={{ color: restoreValidationReport.foreignKeyCheckPassed ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                        {restoreValidationReport.foreignKeyCheckPassed ? "PASSED" : "FAILED"}
                      </strong>
                    </div>
                    <div style={{ padding: "0.5rem 0.75rem", borderRadius: "0.4rem", background: "rgba(0,0,0,0.2)", fontSize: "0.75rem" }}>
                      <span style={{ color: "var(--text-muted)" }}>Compatibility: </span>
                      <strong style={{ color: restoreValidationReport.compatibilityStatus === "COMPATIBLE" ? "var(--accent-emerald)" : "var(--accent-rose)" }}>
                        {restoreValidationReport.compatibilityStatus}
                      </strong>
                    </div>
                  </div>
                ) : null}
              </div>

              {/* Requirement 8: Warning that restore replaces current business data */}
              <div
                style={{
                  padding: "0.85rem",
                  borderRadius: "0.5rem",
                  background: "rgba(239, 68, 68, 0.1)",
                  border: "1px solid rgba(239, 68, 68, 0.3)",
                  marginBottom: "1rem",
                }}
              >
                <div style={{ fontWeight: 700, fontSize: "0.82rem", color: "#f87171", marginBottom: "0.3rem" }}>
                  ⚠️ Critical Warning: Current Business Data Will Be Replaced
                </div>
                <div style={{ fontSize: "0.75rem", color: "#fca5a5", lineHeight: 1.5 }}>
                  Restoring will atomically replace active SQLite database pages with the snapshot state. Transactions, sales, inventory adjustments, or khata payments recorded after this snapshot was created will be overwritten.
                </div>
                <div style={{ fontSize: "0.72rem", color: "var(--text-muted)", marginTop: "0.4rem" }}>
                  (The engine automatically retains a rollback checkpoint and rolls back immediately if post-replacement verification fails.)
                </div>
              </div>

              {/* Requirement 8: Explicit Confirmation Checkbox */}
              <label
                style={{
                  display: "flex",
                  alignItems: "flex-start",
                  gap: "0.6rem",
                  padding: "0.65rem 0.8rem",
                  borderRadius: "0.45rem",
                  background: "rgba(255,255,255,0.02)",
                  border: "1px solid var(--border-subtle)",
                  cursor: "pointer",
                  fontSize: "0.78rem",
                  color: "var(--text-primary)",
                }}
              >
                <input
                  type="checkbox"
                  checked={restoreConfirmedCheck}
                  onChange={(e) => setRestoreConfirmedCheck(e.target.checked)}
                  style={{ marginTop: "0.15rem" }}
                />
                <span>
                  I explicitly authorize the replacement of the current store database with the <strong>{restoreCandidate.fileName}</strong> snapshot.
                </span>
              </label>
            </div>

            {/* Requirement 8: Cancellation with zero side effects */}
            <div className="modal-footer">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => {
                  setShowRestoreModal(false);
                  setRestoreCandidate(null);
                  setRestoreConfirmedCheck(false);
                }}
                disabled={restoringBackup}
              >
                Cancel (Zero Side Effects)
              </button>
              <button
                type="button"
                className="btn"
                style={{
                  background: restoreConfirmedCheck && restoreValidationReport?.isValid ? "#ef4444" : "rgba(239, 68, 68, 0.4)",
                  color: "#fff",
                  fontWeight: 700,
                  cursor: restoreConfirmedCheck && restoreValidationReport?.isValid ? "pointer" : "not-allowed",
                }}
                onClick={() => void handleExecuteRestore()}
                disabled={!restoreConfirmedCheck || !restoreValidationReport?.isValid || restoringBackup}
              >
                {restoringBackup ? "Restoring Database Pages..." : "Execute Authoritative Restore"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
